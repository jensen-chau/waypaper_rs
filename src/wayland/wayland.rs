use anyhow::Result;
use std::fs::File;
use std::io::{Seek, Write};
use std::os::unix::io::AsFd;
use wayland_client::protocol::{
    wl_buffer, wl_compositor, wl_display, wl_output, wl_registry, wl_seat, wl_shm, wl_shm_pool,
    wl_surface,
};
use wayland_client::{
    Connection, Dispatch, QueueHandle,
    globals::{GlobalListContents, registry_queue_init},
};
use wayland_protocols_wlr::layer_shell::v1::client::{zwlr_layer_shell_v1, zwlr_layer_surface_v1};

/// Scaling mode for wallpaper/video
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScaleMode {
    /// Crop mode (cover): Scale to fill the entire output, cropping excess
    /// This is the default wallpaper behavior
    Crop,
    /// Fit mode (contain): Scale to fit within output, preserving aspect ratio
    /// May have black bars
    Fit,
    /// No scaling: Display at original size, centered
    No,
}

impl Default for ScaleMode {
    fn default() -> Self {
        ScaleMode::Crop
    }
}

pub struct WaylandApp {
    pub conn: Connection,
    pub display: wl_display::WlDisplay,
    pub compositor: Option<wl_compositor::WlCompositor>,
    pub layer_shell: Option<zwlr_layer_shell_v1::ZwlrLayerShellV1>,
    pub shm: Option<wl_shm::WlShm>,
    pub surface: Option<wl_surface::WlSurface>,
    pub layer_surface: Option<zwlr_layer_surface_v1::ZwlrLayerSurfaceV1>,
    pub buffer: Option<wl_buffer::WlBuffer>,
    pub shm_pool: Option<wl_shm_pool::WlShmPool>,
    pub shm_file: Option<File>,
    pub queue: Option<wayland_client::EventQueue<WaylandApp>>,
    pub configured: bool,
    pub configured_width: u32,
    pub configured_height: u32,
    pub frame_count: u64,
    pub pool_size: i32,
    pub output_width: u32,
    pub output_height: u32,
    pub scale_mode: ScaleMode,
}

impl WaylandApp {
    pub fn new() -> Result<Self> {
        let conn = Connection::connect_to_env()?;
        let conn_clone = conn.clone();
        let display = conn_clone.display();

        // Calculate pool size for 4K support (3840x2160 * 4 bytes/pixel)
        let pool_size = 3840 * 2160 * 4;

        let mut app = Self {
            conn,
            display: display.clone(),
            compositor: None,
            layer_shell: None,
            shm: None,
            surface: None,
            layer_surface: None,
            buffer: None,
            shm_pool: None,
            shm_file: None,
            queue: None,
            configured: false,
            configured_width: 0,
            configured_height: 0,
            frame_count: 0,
            pool_size,
            output_width: 1920, // Default to 1920x1080
            output_height: 1080,
            scale_mode: ScaleMode::default(),
        };
        
        // Create event queue
        let mut queue = conn_clone.new_event_queue::<WaylandApp>();
        let qh = queue.handle();
        
        // Get registry
        let _registry = display.get_registry(&qh, ());
        
        // Do initial roundtrip to receive globals
        queue.roundtrip(&mut app)?;
        
        // Wait for globals to be bound
        let mut iterations = 0;
        while (app.compositor.is_none() || app.shm.is_none() || app.layer_shell.is_none()) && iterations < 20 {
            queue.roundtrip(&mut app)?;
            iterations += 1;
        }

        if app.compositor.is_none() || app.shm.is_none() || app.layer_shell.is_none() {
            return Err(anyhow::anyhow!("Failed to bind Wayland globals"));
        }

        // Create reusable SHM pool
        let shm = app.shm.as_ref().unwrap();
        let mut shm_file = tempfile::tempfile()?;
        shm_file.set_len(app.pool_size as u64)?;
        let shm_pool = shm.create_pool(shm_file.as_fd(), app.pool_size, &qh, ());

        app.shm_file = Some(shm_file);
        app.shm_pool = Some(shm_pool);
        
        // Create surface and layer surface
        let compositor = app.compositor.as_ref().unwrap();
        let layer_shell = app.layer_shell.as_ref().unwrap();
        
        let surface = compositor.create_surface(&qh, ());
        app.surface = Some(surface.clone());
        
        let layer_surface = layer_shell.get_layer_surface(
            &surface,
            None,
            zwlr_layer_shell_v1::Layer::Background,
            "waypaper-rs".to_string(),
            &qh,
            (),
        );
        app.layer_surface = Some(layer_surface.clone());
        
        // Configure layer surface
        layer_surface.set_size(0, 0);
        layer_surface.set_anchor(
            zwlr_layer_surface_v1::Anchor::Top 
            | zwlr_layer_surface_v1::Anchor::Bottom 
            | zwlr_layer_surface_v1::Anchor::Left 
            | zwlr_layer_surface_v1::Anchor::Right
        );
        layer_surface.set_exclusive_zone(-1);
        layer_surface.set_keyboard_interactivity(zwlr_layer_surface_v1::KeyboardInteractivity::None);
        
        surface.commit();
        
        // Wait for configure
        iterations = 0;
        while !app.configured && iterations < 20 {
            queue.roundtrip(&mut app)?;
            iterations += 1;
        }
        
        app.queue = Some(queue);
        Ok(app)
    }

    pub fn render_frame(&mut self, frame_data: &[u8], width: u32, height: u32) -> Result<()> {
        if !self.configured {
            return Ok(());
        }

        let surface = self
            .surface
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Surface not available"))?;
        let shm_pool = self.shm_pool.as_ref().ok_or_else(|| anyhow::anyhow!("SHM pool not available"))?;

        // Check if scaling is needed (before any mutable borrows)
        let (render_data, render_width, render_height) = if width != self.output_width || height != self.output_height {
            if self.frame_count == 0 {
                log::info!("Scaling video from {}x{} to output {}x{}", width, height, self.output_width, self.output_height);
            }
            self.scale_frame_to_output(frame_data, width, height)
        } else {
            (frame_data.to_vec(), width, height)
        };

        let shm_file = self.shm_file.as_mut().ok_or_else(|| anyhow::anyhow!("SHM file not available"))?;
        let queue = self.queue.as_mut().ok_or_else(|| anyhow::anyhow!("Queue not available"))?;
        let qh = queue.handle();

        let stride = render_width * 4;
        let size = stride * render_height;

        // Check if pool size is sufficient
        if size as i32 > self.pool_size {
            return Err(anyhow::anyhow!("Frame size {} exceeds pool size {}", size, self.pool_size));
        }

        // Write frame data to SHM file
        let file_start = std::time::Instant::now();
        shm_file.seek(std::io::SeekFrom::Start(0))?;
        shm_file.write_all(&render_data)?;
        let file_time = file_start.elapsed();

        // Destroy old buffer if exists
        if let Some(old_buffer) = self.buffer.take() {
            old_buffer.destroy();
        }

        // Create new buffer from existing pool
        let buffer_start = std::time::Instant::now();
        let buffer = shm_pool.create_buffer(
            0,
            render_width as i32,
            render_height as i32,
            stride as i32,
            wl_shm::Format::Argb8888,
            &qh,
            (),
        );
        self.buffer = Some(buffer.clone());
        let buffer_time = buffer_start.elapsed();

        // Debug: log first few pixels (BGRA format) every 30 frames
        self.frame_count += 1;
        if self.frame_count % 30 == 0 {
            log::info!("Frame {} - First 2 pixels (BGRA): B={}, G={}, R={}, A={}, B={}, G={}, R={}, A={}",
                     self.frame_count, render_data[0], render_data[1], render_data[2], render_data[3],
                     render_data[4], render_data[5], render_data[6], render_data[7]);
        }

        // Attach and commit
        let commit_start = std::time::Instant::now();
        surface.attach(Some(&buffer), 0, 0);
        surface.damage(0, 0, render_width as i32, render_height as i32);
        surface.commit();
        let commit_time = commit_start.elapsed();

        // Log timing every 30 frames
        static COUNTER: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let count = COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
        if count % 30 == 0 {
            log::info!("Render timing: file_write={:.2}ms, buffer_create={:.2}ms, commit={:.2}ms",
                     file_time.as_secs_f64() * 1000.0,
                     buffer_time.as_secs_f64() * 1000.0,
                     commit_time.as_secs_f64() * 1000.0);
        }

        Ok(())
    }

    pub fn dispatch_events(&mut self) -> Result<()> {
        if self.queue.is_some() {
            // Take the queue temporarily to avoid borrow issues
            let mut queue = self.queue.take().unwrap();
            let result = queue.roundtrip(self);
            self.queue = Some(queue);
            result.map_err(|e| anyhow::anyhow!("Failed to dispatch events: {}", e))?;
        }
        Ok(())
    }

    pub fn set_scale_mode(&mut self, mode: ScaleMode) {
        log::info!("Setting scale mode to: {:?}", mode);
        self.scale_mode = mode;
    }

    /// Scale frame according to the configured scale mode
    pub fn scale_frame_to_output(
        &self,
        frame_data: &[u8],
        video_width: u32,
        video_height: u32,
    ) -> (Vec<u8>, u32, u32) {
        match self.scale_mode {
            ScaleMode::Crop => self.scale_crop(frame_data, video_width, video_height),
            ScaleMode::Fit => self.scale_fit(frame_data, video_width, video_height),
            ScaleMode::No => self.scale_no(frame_data, video_width, video_height),
        }
    }

    /// Crop mode (cover): Scale to fill the entire output, cropping excess
    /// This is the default wallpaper behavior
    fn scale_crop(
        &self,
        frame_data: &[u8],
        video_width: u32,
        video_height: u32,
    ) -> (Vec<u8>, u32, u32) {
        let output_width = self.output_width;
        let output_height = self.output_height;

        // Calculate scaling factors
        let scale_x = output_width as f64 / video_width as f64;
        let scale_y = output_height as f64 / video_height as f64;
        
        // Use the LARGER scale to cover the entire output (crop mode)
        // This ensures the output is completely filled
        let scale = scale_x.max(scale_y);
        
        let scaled_width = (video_width as f64 * scale) as u32;
        let scaled_height = (video_height as f64 * scale) as u32;
        
        // Calculate source crop offsets to center the content
        let src_offset_x_f64 = (scaled_width - output_width) as f64 / 2.0;
        let src_offset_y_f64 = (scaled_height - output_height) as f64 / 2.0;
        
        // Create output buffer
        let mut output_data = vec![0u8; (output_width * output_height * 4) as usize];
        
        // Perform scaling with nearest neighbor (fastest)
        let video_stride = video_width * 4;
        let output_stride = output_width * 4;
        let inv_scale = 1.0 / scale;
        
        unsafe {
            let src_ptr = frame_data.as_ptr();
            let dst_ptr = output_data.as_mut_ptr();
            
            for y in 0..output_height {
                // Pre-calculate source Y coordinate
                let src_y = ((y as f64 + src_offset_y_f64) * inv_scale) as u32;
                let src_row_start = (src_y as usize) * video_stride as usize;
                let dst_row_start = (y as usize) * output_stride as usize;
                
                for x in 0..output_width {
                    // Pre-calculate source X coordinate
                    let src_x = ((x as f64 + src_offset_x_f64) * inv_scale) as u32;
                    let src_idx = src_row_start + (src_x as usize * 4);
                    let dst_idx = dst_row_start + (x as usize * 4);
                    
                    // Copy BGRA pixels
                    *dst_ptr.add(dst_idx) = *src_ptr.add(src_idx);         // B
                    *dst_ptr.add(dst_idx + 1) = *src_ptr.add(src_idx + 1); // G
                    *dst_ptr.add(dst_idx + 2) = *src_ptr.add(src_idx + 2); // R
                    *dst_ptr.add(dst_idx + 3) = *src_ptr.add(src_idx + 3); // A
                }
            }
        }
        
        (output_data, output_width, output_height)
    }

    /// Fit mode (contain): Scale to fit within output, preserving aspect ratio
    /// May have black bars
    fn scale_fit(
        &self,
        frame_data: &[u8],
        video_width: u32,
        video_height: u32,
    ) -> (Vec<u8>, u32, u32) {
        let output_width = self.output_width;
        let output_height = self.output_height;

        // Calculate scaling factors
        let scale_x = output_width as f64 / video_width as f64;
        let scale_y = output_height as f64 / video_height as f64;
        
        // Use the smaller scale to preserve aspect ratio
        let scale = scale_x.min(scale_y);
        
        let scaled_width = (video_width as f64 * scale) as u32;
        let scaled_height = (video_height as f64 * scale) as u32;
        
        // Center the scaled image
        let offset_x = ((output_width - scaled_width) / 2) as u32;
        let offset_y = ((output_height - scaled_height) / 2) as u32;
        
        // Create output buffer (fill with black)
        let mut output_data = vec![0u8; (output_width * output_height * 4) as usize];
        
        // Perform scaling with nearest neighbor (fastest)
        let video_stride = video_width * 4;
        let output_stride = output_width * 4;
        let inv_scale = 1.0 / scale;
        
        unsafe {
            let src_ptr = frame_data.as_ptr();
            let dst_ptr = output_data.as_mut_ptr();
            
            for y in 0..scaled_height {
                let src_y = (y as f64 * inv_scale) as u32;
                let src_row_start = (src_y as usize) * video_stride as usize;
                let dst_row_start = ((offset_y + y) as usize) * output_stride as usize;
                
                for x in 0..scaled_width {
                    let src_x = (x as f64 * inv_scale) as u32;
                    let src_idx = src_row_start + (src_x as usize * 4);
                    let dst_idx = dst_row_start + ((offset_x + x) as usize * 4);
                    
                    // Copy BGRA pixels
                    *dst_ptr.add(dst_idx) = *src_ptr.add(src_idx);         // B
                    *dst_ptr.add(dst_idx + 1) = *src_ptr.add(src_idx + 1); // G
                    *dst_ptr.add(dst_idx + 2) = *src_ptr.add(src_idx + 2); // R
                    *dst_ptr.add(dst_idx + 3) = *src_ptr.add(src_idx + 3); // A
                }
            }
        }
        
        (output_data, output_width, output_height)
    }

    /// No scaling: Display at original size, centered
    fn scale_no(
        &self,
        frame_data: &[u8],
        video_width: u32,
        video_height: u32,
    ) -> (Vec<u8>, u32, u32) {
        let output_width = self.output_width;
        let output_height = self.output_height;

        // Center the image
        let offset_x = ((output_width - video_width) / 2).max(0) as u32;
        let offset_y = ((output_height - video_height) / 2).max(0) as u32;
        
        // Calculate actual dimensions to copy (don't exceed output)
        let copy_width = video_width.min(output_width);
        let copy_height = video_height.min(output_height);
        
        // Create output buffer (fill with black)
        let mut output_data = vec![0u8; (output_width * output_height * 4) as usize];
        
        let video_stride = video_width * 4;
        let output_stride = output_width * 4;
        
        unsafe {
            let src_ptr = frame_data.as_ptr();
            let dst_ptr = output_data.as_mut_ptr();
            
            for y in 0..copy_height {
                let src_row_start = (y as usize) * video_stride as usize;
                let dst_row_start = ((offset_y + y) as usize) * output_stride as usize;
                
                for x in 0..copy_width {
                    let src_idx = src_row_start + (x as usize * 4);
                    let dst_idx = dst_row_start + ((offset_x + x) as usize * 4);
                    
                    // Copy BGRA pixels
                    *dst_ptr.add(dst_idx) = *src_ptr.add(src_idx);         // B
                    *dst_ptr.add(dst_idx + 1) = *src_ptr.add(src_idx + 1); // G
                    *dst_ptr.add(dst_idx + 2) = *src_ptr.add(src_idx + 2); // R
                    *dst_ptr.add(dst_idx + 3) = *src_ptr.add(src_idx + 3); // A
                }
            }
        }
        
        (output_data, output_width, output_height)
    }
}

// Dispatch implementations
impl Dispatch<wl_compositor::WlCompositor, ()> for WaylandApp {
    fn event(
        _state: &mut Self,
        _proxy: &wl_compositor::WlCompositor,
        _event: wl_compositor::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<wl_surface::WlSurface, ()> for WaylandApp {
    fn event(
        _state: &mut Self,
        _proxy: &wl_surface::WlSurface,
        _event: wl_surface::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
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
    ) {
    }
}

impl Dispatch<wl_shm_pool::WlShmPool, ()> for WaylandApp {
    fn event(
        _state: &mut Self,
        _proxy: &wl_shm_pool::WlShmPool,
        _event: wl_shm_pool::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
        // wl_shm_pool has no events in the current protocol
    }
}

impl Dispatch<wl_buffer::WlBuffer, ()> for WaylandApp {
    fn event(
        _state: &mut Self,
        _proxy: &wl_buffer::WlBuffer,
        _event: wl_buffer::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<wl_output::WlOutput, ()> for WaylandApp {
    fn event(
        state: &mut Self,
        _proxy: &wl_output::WlOutput,
        event: wl_output::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
        match event {
            wl_output::Event::Mode {
                flags,
                width,
                height,
                refresh,
                ..
            } => {
                // Only consider current mode (not preferred)
                if flags == wayland_client::WEnum::Value(wl_output::Mode::Current) {
                    state.output_width = width as u32;
                    state.output_height = height as u32;
                    log::info!("Output size: {}x{}, refresh: {}mHz", width, height, refresh);
                }
            }
            wl_output::Event::Scale {
                factor,
                ..
            } => {
                log::info!("Output scale factor: {}", factor);
            }
            _ => {}
        }
    }
}

impl Dispatch<wl_seat::WlSeat, ()> for WaylandApp {
    fn event(
        _state: &mut Self,
        _proxy: &wl_seat::WlSeat,
        _event: wl_seat::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<zwlr_layer_shell_v1::ZwlrLayerShellV1, ()> for WaylandApp {
    fn event(
        _state: &mut Self,
        _proxy: &zwlr_layer_shell_v1::ZwlrLayerShellV1,
        _event: zwlr_layer_shell_v1::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
    }
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
            zwlr_layer_surface_v1::Event::Configure {
                serial,
                width,
                height,
            } => {
                _proxy.ack_configure(serial);
                state.configured = true;
                state.configured_width = width;
                state.configured_height = height;
            }
            zwlr_layer_surface_v1::Event::Closed => {
                std::process::exit(0);
            }
            _ => {}
        }
    }
}

impl Dispatch<wl_registry::WlRegistry, ()> for WaylandApp {
    fn event(
        state: &mut Self,
        registry: &wl_registry::WlRegistry,
        event: wl_registry::Event,
        _data: &(),
        _conn: &Connection,
        qhandle: &QueueHandle<Self>,
    ) {
        match event {
            wl_registry::Event::Global {
                name,
                interface,
                version: _,
            } => {
                log::info!("Global: {} (name: {})", interface, name);
                match interface.as_str() {
                    "wl_compositor" => {
                        state.compositor = Some(registry.bind::<wl_compositor::WlCompositor, _, _>(
                            name,
                            4,
                            qhandle,
                            (),
                        ));
                        log::info!("Bound wl_compositor");
                    }
                    "wl_shm" => {
                        state.shm = Some(registry.bind::<wl_shm::WlShm, _, _>(name, 1, qhandle, ()));
                        log::info!("Bound wl_shm");
                    }
                    "zwlr_layer_shell_v1" => {
                        state.layer_shell = Some(
                            registry.bind::<zwlr_layer_shell_v1::ZwlrLayerShellV1, _, _>(
                                name,
                                1,
                                qhandle,
                                (),
                            ),
                        );
                        log::info!("Bound zwlr_layer_shell_v1");
                    }
                    "wl_output" => {
                        // Bind output to get display size information
                        let _output = registry.bind::<wl_output::WlOutput, _, _>(name, 4, qhandle, ());
                        log::info!("Bound wl_output");
                    }
                    _ => {}
                }
            }
            wl_registry::Event::GlobalRemove { name: _ } => {}
            _ => {}
        }
    }
}

