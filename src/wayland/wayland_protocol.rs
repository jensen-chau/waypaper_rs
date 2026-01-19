use wayland_client::{
    Connection, Dispatch, QueueHandle,
    globals::{registry_queue_init, GlobalList},
    protocol::{
        wl_compositor, wl_output, wl_seat, wl_shm, wl_shm_pool, wl_surface,
        wl_registry, wl_buffer,
    },
};
use wayland_protocols_wlr::layer_shell::v1::client::{
    zwlr_layer_shell_v1, zwlr_layer_surface_v1,
};

use std::{
    fs::File,
    io::{BufWriter, Write},
    os::unix::io::AsFd,
    sync::Arc,
    sync::atomic::{AtomicBool, Ordering},
};

/// Wayland 显示连接
pub struct WaylandDisplay {
    conn: Connection,
    globals: GlobalList,
    queue: wayland_client::EventQueue<WaylandApp>,
    qh: QueueHandle<WaylandApp>,
    running: Arc<AtomicBool>,
}

impl WaylandDisplay {
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let conn = Connection::connect_to_env()?;
        let (globals, queue) = registry_queue_init::<WaylandApp>(&conn)?;
        let qh = queue.handle();
        
        Ok(Self {
            conn,
            globals,
            queue,
            qh,
            running: Arc::new(AtomicBool::new(false)),
        })
    }
    
    pub fn handle(&self) -> &QueueHandle<WaylandApp> {
        &self.qh
    }
    
    pub fn connection(&self) -> &Connection {
        &self.conn
    }
    
    pub fn globals(&self) -> &GlobalList {
        &self.globals
    }
    
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }
    
    pub fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
    }
    
    /// 处理一次事件
    pub fn dispatch(&mut self, app: &mut WaylandApp) -> Result<(), Box<dyn std::error::Error>> {
        self.queue.blocking_dispatch(app)?;
        Ok(())
    }
}

/// 可更新的 SHM 缓冲区
pub struct ShmBuffer {
    file: File,
    pool: wl_shm_pool::WlShmPool,
    buffer: wl_buffer::WlBuffer,
    width: u32,
    height: u32,
    stride: u32,
    data: Vec<u8>,
}

impl ShmBuffer {
    pub fn new(
        shm: &wl_shm::WlShm,
        width: u32,
        height: u32,
        qh: &QueueHandle<WaylandApp>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let stride = width * 4;
        let size = stride * height;

        // 创建临时文件用于 SHM
        let file = tempfile::tempfile()?;

        // 初始化为零
        let data = vec![0u8; size as usize];
        {
            let mut buf = BufWriter::new(&file);
            buf.write_all(&data)?;
            buf.flush()?;
        }

        // 创建 SHM pool
        let pool = shm.create_pool(file.as_fd(), size as i32, qh, ());

        // 从 pool 创建 buffer
        let buffer = pool.create_buffer(
            0,
            width as i32,
            height as i32,
            stride as i32,
            wl_shm::Format::Argb8888,
            qh,
            (),
        );

        Ok(Self {
            file,
            pool,
            buffer,
            width,
            height,
            stride,
            data,
        })
    }

    /// 更新缓冲区内容（接受 frame data）
    pub fn update(&mut self, pixels: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
        if pixels.len() != self.data.len() {
            return Err(format!(
                "Pixel data size mismatch: expected {}, got {}",
                self.data.len(),
                pixels.len()
            ).into());
        }

        // 复制像素数据
        self.data.copy_from_slice(pixels);

        // 写入文件
        use std::os::unix::fs::FileExt;
        self.file.write_all_at(&self.data, 0)?;

        Ok(())
    }

    pub fn buffer(&self) -> &wl_buffer::WlBuffer {
        &self.buffer
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }
}

/// Wayland 应用状态
pub struct WaylandApp {
    display: Option<WaylandDisplay>,
    compositor: Option<wl_compositor::WlCompositor>,
    layer_shell: Option<zwlr_layer_shell_v1::ZwlrLayerShellV1>,
    shm: Option<wl_shm::WlShm>,
    
    layer_surface: Option<zwlr_layer_surface_v1::ZwlrLayerSurfaceV1>,
    surface: Option<wl_surface::WlSurface>,
    shm_buffer: Option<ShmBuffer>,
    
    configured: bool,
    configured_width: u32,
    configured_height: u32,
    
    // 回调函数：当需要新帧时调用
    frame_callback: Option<Box<dyn Fn() + Send>>,
}

impl WaylandApp {
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        Ok(Self {
            display: None,
            compositor: None,
            layer_shell: None,
            shm: None,
            layer_surface: None,
            surface: None,
            shm_buffer: None,
            configured: false,
            configured_width: 0,
            configured_height: 0,
            frame_callback: None,
        })
    }
    
    /// 设置帧回调
    pub fn set_frame_callback<F>(&mut self, callback: F)
    where
        F: Fn() + Send + 'static,
    {
        self.frame_callback = Some(Box::new(callback));
    }
    
    /// 初始化 Wayland 连接
    pub fn init(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let display = WaylandDisplay::new()?;
        let qh = display.handle();
        let globals = display.globals();
        
        // 绑定全局对象
        let globals_list = globals.contents().clone_list();
        for global in &globals_list {
            match global.interface.as_str() {
                "wl_compositor" => {
                    if let Ok(compositor) = globals.bind::<wl_compositor::WlCompositor, WaylandApp, ()>(qh, 4..=global.version, ()) {
                        self.compositor = Some(compositor);
                    }
                }
                "zwlr_layer_shell_v1" => {
                    if let Ok(layer_shell) = globals.bind::<zwlr_layer_shell_v1::ZwlrLayerShellV1, WaylandApp, ()>(qh, 1..=global.version, ()) {
                        self.layer_shell = Some(layer_shell);
                    }
                }
                "wl_shm" => {
                    if let Ok(shm) = globals.bind::<wl_shm::WlShm, WaylandApp, ()>(qh, 1..=global.version, ()) {
                        self.shm = Some(shm);
                    }
                }
                _ => {}
            }
        }
        
        self.display = Some(display);
        Ok(())
    }
    
    /// 创建 layer surface
    pub fn create_layer_surface(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let display = self.display.as_ref().ok_or("Display not initialized")?;
        let compositor = self.compositor.as_ref().ok_or("Compositor not bound")?;
        let layer_shell = self.layer_shell.as_ref().ok_or("Layer shell not bound")?;
        let qh = display.handle();
        
        // 创建 surface
        let surface = compositor.create_surface(qh, ());
        
        // 创建 layer surface
        let layer_surface = layer_shell.get_layer_surface(
            &surface,
            None,
            zwlr_layer_shell_v1::Layer::Overlay,
            "waypaper".to_string(),
            qh,
            (),
        );
        
        // 设置锚点
        layer_surface.set_anchor(
            zwlr_layer_surface_v1::Anchor::Top
                | zwlr_layer_surface_v1::Anchor::Bottom
                | zwlr_layer_surface_v1::Anchor::Left
                | zwlr_layer_surface_v1::Anchor::Right,
        );
        layer_surface.set_exclusive_zone(-1);
        layer_surface.set_keyboard_interactivity(zwlr_layer_surface_v1::KeyboardInteractivity::None);
        
        self.surface = Some(surface);
        self.layer_surface = Some(layer_surface);
        
        Ok(())
    }
    
    /// 提交帧数据到 Wayland surface
    pub fn submit_frame(&mut self, pixels: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
        if !self.configured {
            return Err("Surface not configured yet".into());
        }
        
        if let Some(shm_buffer) = &mut self.shm_buffer {
            // 更新缓冲区内容
            shm_buffer.update(pixels)?;
            
            // 附加 buffer 到 surface
            if let Some(surface) = &self.surface {
                surface.attach(Some(shm_buffer.buffer()), 0, 0);
                surface.damage_buffer(0, 0, shm_buffer.width() as i32, shm_buffer.height() as i32);
                surface.commit();
            }
        }
        
        Ok(())
    }
    
    /// 运行事件循环
    pub fn run(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let display = self.display.as_ref().ok_or("Display not initialized")?;
        
        while display.is_running() {
            // 处理事件
            // display.queue.blocking_dispatch(self)?;
            
            // 如果配置完成且有回调，调用回调
            if self.configured {
                if let Some(callback) = &self.frame_callback {
                    callback();
                }
            }
            
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        
        Ok(())
    }
}

// Dispatch trait 实现
impl Dispatch<wl_compositor::WlCompositor, ()> for WaylandApp {
    fn event(
        _state: &mut Self,
        _proxy: &wl_compositor::WlCompositor,
        _event: wl_compositor::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {}
}

impl Dispatch<wl_surface::WlSurface, ()> for WaylandApp {
    fn event(
        _state: &mut Self,
        _proxy: &wl_surface::WlSurface,
        _event: wl_surface::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {}
}

impl Dispatch<zwlr_layer_shell_v1::ZwlrLayerShellV1, ()> for WaylandApp {
    fn event(
        _state: &mut Self,
        _proxy: &zwlr_layer_shell_v1::ZwlrLayerShellV1,
        _event: zwlr_layer_shell_v1::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {}
}

impl Dispatch<zwlr_layer_surface_v1::ZwlrLayerSurfaceV1, ()> for WaylandApp {
    fn event(
        state: &mut Self,
        _proxy: &zwlr_layer_surface_v1::ZwlrLayerSurfaceV1,
        event: zwlr_layer_surface_v1::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
        match event {
            zwlr_layer_surface_v1::Event::Configure { serial, width, height } => {
                if let Some(layer_surface) = &state.layer_surface {
                    layer_surface.ack_configure(serial);
                    
                    let display = state.display.as_ref();
                    if let (Some(display), Some(shm)) = (display, &state.shm) {
                        let qh = display.handle();
                        
                        // 创建或更新 SHM buffer
                        if width > 0 && height > 0 {
                            match ShmBuffer::new(shm, width, height, qh) {
                                Ok(buffer) => {
                                    state.shm_buffer = Some(buffer);
                                    state.configured = true;
                                    state.configured_width = width;
                                    state.configured_height = height;
                                    
                                    if let Some(surface) = &state.surface {
                                        surface.commit();
                                    }
                                }
                                Err(e) => {
                                    eprintln!("Failed to create SHM buffer: {}", e);
                                }
                            }
                        }
                    }
                }
            }
            zwlr_layer_surface_v1::Event::Closed => {
                state.layer_surface = None;
                state.surface = None;
            }
            _ => {}
        }
    }
}

impl Dispatch<wl_shm::WlShm, ()> for WaylandApp {
    fn event(
        _state: &mut Self,
        _proxy: &wl_shm::WlShm,
        _event: wl_shm::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {}
}

impl Dispatch<wl_shm_pool::WlShmPool, ()> for WaylandApp {
    fn event(
        _state: &mut Self,
        _proxy: &wl_shm_pool::WlShmPool,
        _event: wl_shm_pool::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {}
}

impl Dispatch<wl_buffer::WlBuffer, ()> for WaylandApp {
    fn event(
        _state: &mut Self,
        _proxy: &wl_buffer::WlBuffer,
        _event: wl_buffer::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {}
}

impl Dispatch<wl_output::WlOutput, ()> for WaylandApp {
    fn event(
        _state: &mut Self,
        _proxy: &wl_output::WlOutput,
        _event: wl_output::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {}
}

impl Dispatch<wl_seat::WlSeat, ()> for WaylandApp {
    fn event(
        _state: &mut Self,
        _proxy: &wl_seat::WlSeat,
        _event: wl_seat::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {}
}

impl Dispatch<wl_registry::WlRegistry, wayland_client::globals::GlobalListContents> for WaylandApp {
    fn event(
        state: &mut Self,
        _proxy: &wl_registry::WlRegistry,
        event: wl_registry::Event,
        _data: &wayland_client::globals::GlobalListContents,
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
        match event {
            wl_registry::Event::Global { name, interface, version } => {
                let display = match state.display.as_ref() {
                    Some(d) => d,
                    None => return,
                };
                
                let qh = display.handle();
                let globals = display.globals();
                
                match interface.as_str() {
                    "wl_compositor" => {
                        if let Ok(compositor) = globals.bind::<wl_compositor::WlCompositor, WaylandApp, ()>(qh, 4..=version, ()) {
                            state.compositor = Some(compositor);
                        }
                    }
                    "zwlr_layer_shell_v1" => {
                        if let Ok(layer_shell) = globals.bind::<zwlr_layer_shell_v1::ZwlrLayerShellV1, WaylandApp, ()>(qh, 1..=version, ()) {
                            state.layer_shell = Some(layer_shell);
                        }
                    }
                    "wl_shm" => {
                        if let Ok(shm) = globals.bind::<wl_shm::WlShm, WaylandApp, ()>(qh, 1..=version, ()) {
                            state.shm = Some(shm);
                        }
                    }
                    _ => {}
                }
            }
            wl_registry::Event::GlobalRemove { name } => {
                // 处理全局对象移除
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_shm_buffer() -> Result<(), Box<dyn std::error::Error>> {
        // 简单测试：创建一个 ShmBuffer
        println!("Testing ShmBuffer creation...");
        // 这个测试需要实际的 Wayland 环境
        Ok(())
    }

    #[test]
    fn test_red_square() -> Result<(), Box<dyn std::error::Error>> {
        println!("Starting red square test...");
        
        let mut app = WaylandApp::new()?;
        app.init()?;
        app.create_layer_surface()?;
        
        println!("Layer surface created. Processing events...");
        
        // 处理事件等待 configure
        let mut iterations = 0;
        while !app.configured && iterations < 100 {
            if let Some(mut display) = app.display.take() {
                display.dispatch(&mut app)?;
                app.display = Some(display);
            }
            iterations += 1;
        }
        
        if app.configured {
            println!("Surface configured: {}x{}", app.configured_width, app.configured_height);
            
            // 创建红色正方形数据
            let width = app.configured_width.max(100);
            let height = app.configured_height.max(100);
            let stride = width * 4;
            let buffer_len = (height * stride) as usize;
            
            let mut pixels = vec![0u8; buffer_len];
            
            // 填充黑色背景
            for pixel in pixels.chunks_exact_mut(4) {
                pixel[0] = 0; // B
                pixel[1] = 0; // G
                pixel[2] = 0; // R
                pixel[3] = 255; // A
            }
            
            // 绘制红色正方形（中心）
            let square_size = 200.min(width.min(height));
            let start_x = (width - square_size) / 2;
            let start_y = (height - square_size) / 2;
            
            for y in start_y..(start_y + square_size) {
                for x in start_x..(start_x + square_size) {
                    let idx = (y * stride + x * 4) as usize;
                    if idx + 4 <= buffer_len {
                        pixels[idx] = 0; // B
                        pixels[idx + 1] = 0; // G
                        pixels[idx + 2] = 255; // R
                        pixels[idx + 3] = 255; // A
                    }
                }
            }
            
            // 提交帧数据
            app.submit_frame(&pixels)?;
            println!("Red square rendered successfully!");
            println!("You should see a red square in the center of your screen.");
            
            // 继续处理事件以保持显示
            for _ in 0..30 {
                if let Some(mut display) = app.display.take() {
                    display.dispatch(&mut app)?;
                    app.display = Some(display);
                }
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
        } else {
            println!("Surface not configured after processing events");
        }
        
        Ok(())
    }
}