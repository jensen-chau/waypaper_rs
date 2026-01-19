use std::sync::atomic::AtomicBool;
use std::sync::{Arc, mpsc};
use std::thread::{self};
use std::time::Duration;
use log::{info, error, warn};

use crate::wallpaper::{project, WallpaperType};
use crate::wallpaper::Wallpaper;
use anyhow::Result;

pub struct VideoWallpaper {
    video_path: String,
    is_paused: Arc<AtomicBool>,
    is_stopped: Arc<AtomicBool>,
    decode_thread: Option<thread::JoinHandle<()>>,
    render_thread: Option<thread::JoinHandle<()>>,
    project: Option<project::Project>,
    wallpaper_type: WallpaperType
}

pub struct FrameData {
    frame: Vec<u8>,
    width: u32,
    height: u32,
    frame_time: u32, // in milliseconds
}

impl VideoWallpaper {
    pub fn new(video_path: String, wallpaper_type: WallpaperType) -> Self {
        Self {
            video_path,
            is_paused: Arc::new(AtomicBool::new(false)),
            is_stopped: Arc::new(AtomicBool::new(false)),
            decode_thread: None,
            render_thread: None,
            project: None,
            wallpaper_type,
        }
    }
    
    pub fn stop(&mut self) {
        self.is_stopped.store(true, std::sync::atomic::Ordering::SeqCst);
        
        // Wait for threads to finish
        if let Some(thread) = self.decode_thread.take() {
            let _ = thread.join();
        }
        if let Some(thread) = self.render_thread.take() {
            let _ = thread.join();
        }
        
        info!("VideoWallpaper stopped");
    }
}

impl Wallpaper for VideoWallpaper {
    fn play(&mut self) {
        self.is_paused.store(false, std::sync::atomic::Ordering::SeqCst);
        info!("VideoWallpaper play");
    }
    
    fn pause(&mut self) {
        self.is_paused.store(true, std::sync::atomic::Ordering::SeqCst);
        info!("VideoWallpaper pause");
    }

    fn run(&mut self) {
        let (tx, rx) = mpsc::channel::<FrameData>();
        let video_path = self.video_path.clone();
        let is_paused = self.is_paused.clone();
        let is_stopped = self.is_stopped.clone();
        
        // Clone for render thread
        let is_paused_render = is_paused.clone();
        let is_stopped_render = is_stopped.clone();
        
        // Decode thread
        let decode_thread = thread::spawn(move || {
            if let Err(e) = decode_video(&video_path, tx, &is_paused, &is_stopped) {
                error!("Video decode error: {}", e);
            }
        });

        self.decode_thread = Some(decode_thread);
        
        // Render thread
        let render_thread = thread::spawn(move || {
            render_frames(rx, &is_paused_render, &is_stopped_render);
        });

        self.render_thread = Some(render_thread);
    }

    fn info(&self) {
        
    }
}

fn decode_video(
    video_path: &str,
    tx: mpsc::Sender<FrameData>,
    is_paused: &Arc<AtomicBool>,
    is_stopped: &Arc<AtomicBool>,
) -> Result<()> {
    use video_rs::Decoder;
    
    info!("Opening video: {}", video_path);
    
    // Open video decoder
    let mut decoder = Decoder::new(std::path::Path::new(video_path))?;
    
    // Get time base for timestamp calculations
    let time_base = decoder.time_base();
    let time_base_num = time_base.numerator() as f64;
    let time_base_den = time_base.denominator() as f64;
    
    info!("Video opened successfully");
    info!("Time base: {}/{}", time_base_num, time_base_den);
    
    let mut frame_count = 0u64;
    let mut last_pts: Option<f64> = None;
    let mut frame_time_ms: u32 = 33; // Default to ~30fps
    let mut total_decode_time = Duration::from_secs(0);
    let mut total_convert_time = Duration::from_secs(0);
    
    loop {
        if is_stopped.load(std::sync::atomic::Ordering::SeqCst) {
            info!("Decode thread stopped");
            break;
        }
        
        if is_paused.load(std::sync::atomic::Ordering::SeqCst) {
            thread::sleep(Duration::from_millis(10));
            continue;
        }
        
        // Decode next frame
        let decode_start = std::time::Instant::now();
        match decoder.decode() {
            Ok((timestamp, frame_data)) => {
                let decode_time = decode_start.elapsed();
                total_decode_time += decode_time;
                frame_count += 1;
                
                // Get frame dimensions from the frame data itself
                let shape = frame_data.shape();
                let height = shape[0] as u32;
                let width = shape[1] as u32;
                
                // Convert frame to RGBA
                let convert_start = std::time::Instant::now();
                let rgba_data = convert_frame_to_rgba(&frame_data, width, height)?;
                let convert_time = convert_start.elapsed();
                total_convert_time += convert_time;
                
                // Calculate frame time based on timestamp
                // timestamp is a Time type
                let pts = timestamp.as_secs_f64();
                
                // Calculate frame time from PTS difference
                if let Some(last) = last_pts {
                    let pts_diff = pts - last;
                    let time_ms = (pts_diff * 1000.0) as u32;
                    // Update frame time if it's reasonable (between 1ms and 1000ms)
                    if time_ms > 0 && time_ms < 1000 {
                        frame_time_ms = time_ms;
                    }
                }
                last_pts = Some(pts);
                
                // Send frame data
                let frame_data = FrameData {
                    frame: rgba_data,
                    width,
                    height,
                    frame_time: frame_time_ms,
                };
                
                if tx.send(frame_data).is_err() {
                    warn!("Render thread disconnected");
                    break;
                }
                
                if frame_count % 30 == 0 {
                    let avg_decode = total_decode_time.as_secs_f64() * 1000.0 / 30.0;
                    let avg_convert = total_convert_time.as_secs_f64() * 1000.0 / 30.0;
                    info!("Decoded {} frames, frame time: {}ms, avg_decode={:.2}ms, avg_convert={:.2}ms", 
                          frame_count, frame_time_ms, avg_decode, avg_convert);
                    total_decode_time = Duration::from_secs(0);
                    total_convert_time = Duration::from_secs(0);
                }
            }
            Err(e) => {
                let error_msg = e.to_string().to_lowercase();
                if error_msg.contains("end") || error_msg.contains("eof") {
                    // End of video, loop back
                    info!("Video ended, restarting");
                    decoder = Decoder::new(std::path::Path::new(video_path))?;
                    frame_count = 0;
                    last_pts = None;
                    frame_time_ms = 33;
                } else {
                    error!("Decode error: {}", e);
                    break;
                }
            }
        }
    }
    
    Ok(())
}

fn convert_frame_to_rgba(
    frame_data: &ndarray::ArrayBase<ndarray::OwnedRepr<u8>, ndarray::Dim<[usize; 3]>>,
    width: u32,
    height: u32,
) -> Result<Vec<u8>> {
    // frame_data is expected to be in RGB format (height, width, 3)
    // We need to convert it to RGBA (height, width, 4)
    
    let mut rgba_data = vec![0u8; (width * height * 4) as usize];
    
    let frame_data_view = frame_data.view();
    
    // Convert RGB to RGBA
    for y in 0..height {
        for x in 0..width {
            let dst_idx = (y * width + x) as usize * 4;
            
            // Ensure we're within bounds
            if dst_idx + 3 < rgba_data.len() {
                // Copy RGB values
                rgba_data[dst_idx] = frame_data_view[[y as usize, x as usize, 0]]; // R
                rgba_data[dst_idx + 1] = frame_data_view[[y as usize, x as usize, 1]]; // G
                rgba_data[dst_idx + 2] = frame_data_view[[y as usize, x as usize, 2]]; // B
                rgba_data[dst_idx + 3] = 255; // A (fully opaque)
            }
        }
    }
    
    Ok(rgba_data)
}

fn render_frames(
    rx: mpsc::Receiver<FrameData>,
    is_paused: &Arc<AtomicBool>,
    is_stopped: &Arc<AtomicBool>,
) {
    info!("Render thread started");
    
    // Initialize Wayland display
    let mut wayland_app = match crate::wayland::WaylandApp::new() {
        Ok(app) => app,
        Err(e) => {
            error!("Failed to initialize Wayland: {}", e);
            return;
        }
    };
    
    let mut frame_count = 0u64;
    let mut last_frame_time = std::time::Instant::now();
    let target_fps = 60.0; // Target 60 FPS
    let frame_duration = Duration::from_secs_f64(1.0 / target_fps);
    let mut total_render_time = Duration::from_secs(0);
    
    while !is_stopped.load(std::sync::atomic::Ordering::SeqCst) {
        if is_paused.load(std::sync::atomic::Ordering::SeqCst) {
            thread::sleep(Duration::from_millis(10));
            continue;
        }
        
        match rx.recv_timeout(Duration::from_millis(10)) {
            Ok(frame_data) => {
                frame_count += 1;
                let render_start = std::time::Instant::now();
                
                // Render frame to Wayland surface
                if let Err(e) = wayland_app.render_frame(&frame_data.frame, frame_data.width, frame_data.height) {
                    error!("Failed to render frame: {}", e);
                }
                
                let render_time = render_start.elapsed();
                total_render_time += render_time;
                let elapsed_since_last_frame = last_frame_time.elapsed();
                
                // Calculate actual FPS
                let fps = if elapsed_since_last_frame.as_secs_f64() > 0.0 {
                    1.0 / elapsed_since_last_frame.as_secs_f64()
                } else {
                    target_fps
                };
                
                // Log every 30 frames
                if frame_count % 30 == 0 {
                    let avg_render = total_render_time.as_secs_f64() * 1000.0 / 30.0;
                    info!("Render {}: {}x{}, frame_time={}ms, avg_render={:.2}ms, elapsed={:.2}ms, FPS={:.2}",
                          frame_count, frame_data.width, frame_data.height, 
                          frame_data.frame_time, avg_render,
                          elapsed_since_last_frame.as_secs_f64() * 1000.0, fps);
                    total_render_time = Duration::from_secs(0);
                }
                
                // Control frame rate - sleep if we rendered too fast
                if render_time < frame_duration {
                    let sleep_time = frame_duration.saturating_sub(render_time);
                    thread::sleep(sleep_time);
                }
                
                last_frame_time = std::time::Instant::now();
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                // No frame available, continue waiting
                continue;
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                info!("Decode thread disconnected");
                break;
            }
        }
    }
    
    info!("Render thread stopped, total frames rendered: {}", frame_count);
}