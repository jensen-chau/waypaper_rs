use wayland_client::{Connection, Dispatch, QueueHandle};
use wayland_client::protocol::{wl_compositor, wl_output, wl_seat, wl_shm, wl_shm_pool, wl_surface, wl_buffer, wl_registry};
use wayland_protocols_wlr::layer_shell::v1::client::{zwlr_layer_shell_v1, zwlr_layer_surface_v1};
use std::io::Write;
use std::os::unix::io::AsFd;

struct App {
    compositor: Option<wl_compositor::WlCompositor>,
    shm: Option<wl_shm::WlShm>,
    layer_shell: Option<zwlr_layer_shell_v1::ZwlrLayerShellV1>,
    surface: Option<wl_surface::WlSurface>,
    layer_surface: Option<zwlr_layer_surface_v1::ZwlrLayerSurfaceV1>,
    configured: bool,
    configured_width: u32,
    configured_height: u32,
}

impl Dispatch<wl_compositor::WlCompositor, ()> for App {
    fn event(
        _state: &mut Self,
        _proxy: &wl_compositor::WlCompositor,
        _event: wl_compositor::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {}
}

impl Dispatch<wl_surface::WlSurface, ()> for App {
    fn event(
        _state: &mut Self,
        _proxy: &wl_surface::WlSurface,
        _event: wl_surface::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {}
}

impl Dispatch<wl_shm::WlShm, ()> for App {
    fn event(
        _state: &mut Self,
        _proxy: &wl_shm::WlShm,
        _event: wl_shm::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {}
}

impl Dispatch<wl_shm_pool::WlShmPool, ()> for App {
    fn event(
        _state: &mut Self,
        _proxy: &wl_shm_pool::WlShmPool,
        _event: wl_shm_pool::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {}
}

impl Dispatch<wl_buffer::WlBuffer, ()> for App {
    fn event(
        _state: &mut Self,
        _proxy: &wl_buffer::WlBuffer,
        _event: wl_buffer::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {}
}

impl Dispatch<wl_output::WlOutput, ()> for App {
    fn event(
        _state: &mut Self,
        _proxy: &wl_output::WlOutput,
        _event: wl_output::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {}
}

impl Dispatch<wl_seat::WlSeat, ()> for App {
    fn event(
        _state: &mut Self,
        _proxy: &wl_seat::WlSeat,
        _event: wl_seat::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {}
}

impl Dispatch<zwlr_layer_shell_v1::ZwlrLayerShellV1, ()> for App {
    fn event(
        _state: &mut Self,
        _proxy: &zwlr_layer_shell_v1::ZwlrLayerShellV1,
        _event: zwlr_layer_shell_v1::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {}
}

impl Dispatch<zwlr_layer_surface_v1::ZwlrLayerSurfaceV1, ()> for App {
    fn event(
        state: &mut Self,
        proxy: &zwlr_layer_surface_v1::ZwlrLayerSurfaceV1,
        event: zwlr_layer_surface_v1::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
        match event {
            zwlr_layer_surface_v1::Event::Configure { serial, width, height } => {
                println!("Configure event received: {}x{}", width, height);
                proxy.ack_configure(serial);
                state.configured = true;
                state.configured_width = width;
                state.configured_height = height;
            }
            zwlr_layer_surface_v1::Event::Closed => {
                println!("Layer surface closed");
                std::process::exit(0);
            }
            _ => {}
        }
    }
}

impl Dispatch<wl_registry::WlRegistry, ()> for App {
    fn event(
        state: &mut Self,
        registry: &wl_registry::WlRegistry,
        event: wl_registry::Event,
        _data: &(),
        _conn: &Connection,
        qhandle: &QueueHandle<Self>,
    ) {
        match event {
            wl_registry::Event::Global { name, interface, version: _ } => {
                match interface.as_str() {
                    "wl_compositor" => {
                        state.compositor = Some(
                            registry.bind::<wl_compositor::WlCompositor, _, _>(name, 4, qhandle, ())
                        );
                    }
                    "wl_shm" => {
                        state.shm = Some(
                            registry.bind::<wl_shm::WlShm, _, _>(name, 1, qhandle, ())
                        );
                    }
                    "zwlr_layer_shell_v1" => {
                        state.layer_shell = Some(
                            registry.bind::<zwlr_layer_shell_v1::ZwlrLayerShellV1, _, _>(name, 1, qhandle, ())
                        );
                    }
                    _ => {}
                }
            }
            wl_registry::Event::GlobalRemove { name: _ } => {}
            _ => {}
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Starting Wayland red square example with layer shell...");
    
    // Connect to Wayland display
    let conn = Connection::connect_to_env()?;
    println!("Connected to Wayland display");
    
    // Get display and create event queue
    let display = conn.display();
    let mut queue = conn.new_event_queue::<App>();
    let qh = queue.handle();
    println!("Created event queue");
    
    // Get registry
    display.get_registry(&qh, ());
    
    // Initialize app state
    let mut app = App {
        compositor: None,
        shm: None,
        layer_shell: None,
        surface: None,
        layer_surface: None,
        configured: false,
        configured_width: 0,
        configured_height: 0,
    };
    
    // Initial roundtrip to bind globals
    println!("Starting initial roundtrip...");
    match queue.roundtrip(&mut app) {
        Ok(_) => println!("Initial roundtrip successful"),
        Err(e) => {
            eprintln!("Initial roundtrip failed: {}", e);
            return Err(e.into());
        }
    }
    
    // Wait for all globals to be bound
    let mut iterations = 0;
    while app.compositor.is_none() || app.shm.is_none() || app.layer_shell.is_none() {
        iterations += 1;
        println!("Roundtrip {} - compositor: {}, shm: {}, layer_shell: {}", 
                 iterations, 
                 app.compositor.is_some(), 
                 app.shm.is_some(), 
                 app.layer_shell.is_some());
        queue.roundtrip(&mut app)?;
        if iterations > 20 {
            eprintln!("Error: Timeout waiting for globals");
            return Err("Timeout waiting for globals".into());
        }
    }
    
    let compositor = app.compositor.as_ref().unwrap().clone();
    let shm = app.shm.as_ref().unwrap().clone();
    let layer_shell = app.layer_shell.as_ref().unwrap().clone();
    
    println!("Creating surface and layer surface...");
    
    // Create surface
    let surface = compositor.create_surface(&qh, ());
    app.surface = Some(surface.clone());
    
    // Create layer surface (overlay layer)
    let layer_surface = layer_shell.get_layer_surface(
        &surface,
        None, // output - let compositor decide
        zwlr_layer_shell_v1::Layer::Overlay,
        "waypaper-rs".to_string(),
        &qh,
        (),
    );
    app.layer_surface = Some(layer_surface.clone());
    
    // Configure layer surface
    layer_surface.set_size(200, 200); // 200x200 red square
    // No anchor to center it
    layer_surface.set_exclusive_zone(-1); // Don't affect other windows
    layer_surface.set_keyboard_interactivity(zwlr_layer_surface_v1::KeyboardInteractivity::None);
    
    // Commit surface to trigger configure event
    surface.commit();
    
    println!("Waiting for configure event...");
    
    // Wait for configure event
    let mut iterations = 0;
    while !app.configured && iterations < 20 {
        iterations += 1;
        println!("Roundtrip {} waiting for configure...", iterations);
        queue.roundtrip(&mut app)?;
    }
    
    if app.configured {
        println!("Configured: {}x{}", app.configured_width, app.configured_height);
        
        let width = 200;
        let height = 200;
        let stride = width * 4;
        let size = stride * height;
        
        // Create pixel data (transparent by default)
        let mut pixels = vec![0u8; size as usize];
        
        // Draw red square (200x200)
        for y in 0..height {
            for x in 0..width {
                let idx = (y * stride + x * 4) as usize;
                pixels[idx] = 0;     // B
                pixels[idx + 1] = 0; // G
                pixels[idx + 2] = 255; // R
                pixels[idx + 3] = 255; // A
            }
        }
        
        // Create SHM pool and buffer
        let mut file = tempfile::tempfile()?;
        file.write_all(&pixels)?;
        file.set_len(size as u64)?;
        
        let pool = shm.create_pool(file.as_fd(), size as i32, &qh, ());
        let buffer = pool.create_buffer(0, width as i32, height as i32, stride as i32, wl_shm::Format::Argb8888, &qh, ());
        
        // Attach buffer to surface
        surface.attach(Some(&buffer), 0, 0);
        surface.damage(0, 0, width as i32, height as i32);
        surface.commit();
        
        println!("Red square rendered!");
    } else {
        println!("Warning: Configure event not received after 20 roundtrips");
    }
    
    println!("Press Ctrl+C to exit.");
    
    // Run event loop
    loop {
        queue.blocking_dispatch(&mut app)?;
        std::thread::sleep(std::time::Duration::from_millis(16));
    }
}
