use std::sync::Arc;
use std::time::{Duration, Instant};
use log::{info, error, warn};
use tokio::sync::{mpsc, Mutex};
use tokio::task::JoinHandle;

use crate::wallpaper::{project, WallpaperType};
use crate::wallpaper::Wallpaper;
use anyhow::Result;

pub struct VideoWallpaper {
    video_path: String,
    is_paused: Arc<Mutex<bool>>,
    is_stopped: Arc<Mutex<bool>>,
    decode_task: Option<JoinHandle<()>>,
    render_task: Option<JoinHandle<()>>,
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
            is_paused: Arc::new(Mutex::new(false)),
            is_stopped: Arc::new(Mutex::new(false)),
            decode_task: None,
            render_task: None,
            project: None,
            wallpaper_type,
        }
    }
    
    pub fn stop(&mut self) {
        // Note: This is a synchronous method, so we can't use async here
        // The actual stopping will be handled by the async tasks checking the flag
        // This is a limitation of the current API design
        info!("VideoWallpaper stop requested (async tasks will check flag)");
    }
}

impl Wallpaper for VideoWallpaper {
    fn play(&mut self) {
        // Note: This is a synchronous method, can't set async mutex here
        // Will need to be handled differently in async context
        info!("VideoWallpaper play requested");
    }
    
    fn pause(&mut self) {
        // Note: This is a synchronous method, can't set async mutex here
        // Will need to be handled differently in async context
        info!("VideoWallpaper pause requested");
    }

    fn run(&mut self) {
        let (tx, rx) = mpsc::channel::<FrameData>(60); // Increased buffer to 600 frames (~24 seconds at 24fps) for smoother looping
        let video_path = self.video_path.clone();
        let is_paused = self.is_paused.clone();
        let is_stopped = self.is_stopped.clone();
        
        // Clone for render task
        let is_paused_render = is_paused.clone();
        let is_stopped_render = is_stopped.clone();
        
        // Get tokio runtime handle
        let handle = tokio::runtime::Handle::current();
        
        // Spawn decode task
        let decode_task = handle.spawn(async move {
            if let Err(e) = decode_video_async(&video_path, tx, is_paused, is_stopped).await {
                error!("Video decode error: {}", e);
            }
        });
        self.decode_task = Some(decode_task);
        
        // Spawn render task
        let render_task = handle.spawn(async move {
            render_frames_async(rx, is_paused_render, is_stopped_render).await;
        });
        self.render_task = Some(render_task);
    }

    fn info(&self) {
        
    }
}

async fn decode_video_async(
    video_path: &str,
    tx: mpsc::Sender<FrameData>,
    is_paused: Arc<Mutex<bool>>,
    is_stopped: Arc<Mutex<bool>>,
) -> Result<()> {
    use video_rs::{Decoder, DecoderBuilder};
    use video_rs::hwaccel::HardwareAccelerationDeviceType;

    info!("Opening video: {}", video_path);

    // Check available hardware acceleration
    let available_hw = HardwareAccelerationDeviceType::list_available();
    info!("Available hardware acceleration devices: {:?}", available_hw);

    // Create decoder with hardware acceleration
    let decoder = if available_hw.contains(&HardwareAccelerationDeviceType::VaApi) {
        info!("Using VA-API hardware acceleration (Intel/AMD GPU)");
        DecoderBuilder::new(std::path::Path::new(video_path))
            .with_hardware_acceleration(HardwareAccelerationDeviceType::VaApi)
            .build()
    } else if available_hw.contains(&HardwareAccelerationDeviceType::Cuda) {
        info!("Using CUDA hardware acceleration (NVIDIA GPU)");
        DecoderBuilder::new(std::path::Path::new(video_path))
            .with_hardware_acceleration(HardwareAccelerationDeviceType::Cuda)
            .build()
    } else if available_hw.contains(&HardwareAccelerationDeviceType::VideoToolbox) {
        info!("Using VideoToolbox hardware acceleration (Apple)");
        DecoderBuilder::new(std::path::Path::new(video_path))
            .with_hardware_acceleration(HardwareAccelerationDeviceType::VideoToolbox)
            .build()
    } else {
        info!("No hardware acceleration available, using software decoding");
        Decoder::new(std::path::Path::new(video_path))
    };

    let decoder = decoder.map_err(|e| anyhow::anyhow!("Failed to create decoder: {}", e))?;
    let decoder_arc = Arc::new(Mutex::new(decoder));

    info!("Video opened successfully");

    let mut frame_count = 0u64;
    let mut last_pts: Option<f64> = None;
    let mut frame_time_ms: u32 = 33; // Default to ~30fps
    let mut total_decode_time = Duration::from_secs(0);
    let mut total_convert_time = Duration::from_secs(0);

    loop {
        // Check stop flag every frame (critical for shutdown)
        if *is_stopped.lock().await {
            info!("Decode thread stopped");
            break;
        }



        // Only check pause flag every 10 frames to reduce lock contention
        if frame_count % 10 == 0 && *is_paused.lock().await {
            tokio::time::sleep(Duration::from_millis(100)).await;
            continue;
        }

        // Decode with timeout to prevent permanent blocking
        let decode_start = Instant::now();
        let decode_result = tokio::time::timeout(
            Duration::from_secs(5), // 5 second timeout
            async {
                let mut decoder_guard = decoder_arc.lock().await;
                decoder_guard.decode()
            }
        ).await;

        match decode_result {
            Ok(Ok((timestamp, frame_data))) => {
                let decode_time = decode_start.elapsed();
                total_decode_time += decode_time;
                frame_count += 1;

                // Get frame dimensions from the frame data itself
                let shape = frame_data.shape();
                let height = shape[0] as u32;
                let width = shape[1] as u32;

                // Debug: log pixel format and first few pixels (RGB format)
                if frame_count % 60 == 0 {
                    let frame_slice = frame_data.as_slice().unwrap();
                    info!("Frame {} - {}x{} - First 3 pixels (RGB): R={}, G={}, B={}, R={}, G={}, B={}",
                          frame_count, width, height,
                          frame_slice[0], frame_slice[1], frame_slice[2],
                          frame_slice[3], frame_slice[4], frame_slice[5]);
                }

                // Convert frame to BGRA
                let convert_start = std::time::Instant::now();
                let rgba_data = convert_frame_to_rgba(&frame_data, width, height)?;
                let convert_time = convert_start.elapsed();
                total_convert_time += convert_time;

                // Calculate frame time based on timestamp
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

                match tx.send(frame_data).await {
                    Ok(_) => {}
                    Err(_) => {
                        warn!("Render thread disconnected");
                        break;
                    }
                }

                if frame_count % 60 == 0 {
                    let avg_decode = total_decode_time.as_secs_f64() * 1000.0 / 60.0;
                    let avg_convert = total_convert_time.as_secs_f64() * 1000.0 / 60.0;
                    info!("Decoded {} frames, frame time: {}ms, avg_decode={:.2}ms, avg_convert={:.2}ms",
                          frame_count, frame_time_ms, avg_decode, avg_convert);
                    total_decode_time = Duration::from_secs(0);
                    total_convert_time = Duration::from_secs(0);
                }
            }
            Ok(Err(e)) => {
                let error_msg = e.to_string().to_lowercase();
                if error_msg.contains("end") || error_msg.contains("eof") || error_msg.contains("exhausted") {
                    // End of video, loop back using seek(0)
                    info!("Video ended, seeking to beginning");
                    let mut decoder_guard = decoder_arc.lock().await;
                    let _ = decoder_guard.seek(0);
                    
                    // Reset frame tracking
                    frame_count = 0;
                    last_pts = None;
                    frame_time_ms = 33;
                } else {
                    error!("Decode error: {}", e);
                    break;
                }
            }
            Err(_) => {
                // Timeout
                error!("Decode timeout after 5 seconds");
                break;
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
    // Wayland uses BGRA format, so we need to convert RGB -> BGRA
    // Note: Wayland expects BGRA, not RGBA
    
    let pixel_count = (width * height) as usize;
    let mut bgra_data = vec![0u8; pixel_count * 4];
    
    let frame_data_slice = frame_data.as_slice().unwrap();
    
    // Use unsafe pointer arithmetic for maximum performance
    // RGB24 -> BGRA: R->B, G->G, B->R
    unsafe {
        let rgb_ptr = frame_data_slice.as_ptr();
        let bgra_ptr = bgra_data.as_mut_ptr();
        
        for i in 0..pixel_count {
            let rgb_idx = i * 3;
            let bgra_idx = i * 4;
            
            *bgra_ptr.add(bgra_idx) = *rgb_ptr.add(rgb_idx + 2);     // B <- R
            *bgra_ptr.add(bgra_idx + 1) = *rgb_ptr.add(rgb_idx + 1); // G <- G
            *bgra_ptr.add(bgra_idx + 2) = *rgb_ptr.add(rgb_idx);     // R <- B
            *bgra_ptr.add(bgra_idx + 3) = 255;                        // A (fully opaque)
        }
    }
    
    Ok(bgra_data)
}

async fn render_frames_async(
    mut rx: mpsc::Receiver<FrameData>,
    is_paused: Arc<Mutex<bool>>,
    is_stopped: Arc<Mutex<bool>>,
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
    let start_time = std::time::Instant::now();
    let mut first_frame_time: Option<std::time::Instant> = None;
    let mut next_frame_time = start_time; // Time when next frame should be displayed
    let mut last_frame_time: Option<std::time::Instant> = None;

    while !*is_stopped.lock().await {
        // Check pause flag
        if *is_paused.lock().await {
            tokio::time::sleep(Duration::from_millis(10)).await;
            continue;
        }

        // Try to receive a frame - use timeout for non-blocking
        let recv_start = std::time::Instant::now();
        match tokio::time::timeout(Duration::from_millis(1), rx.recv()).await {
            Ok(Some(frame_data)) => {
                frame_count += 1;

                // Detect loop restart (frame_time reset to default 33ms after > 100 frames)
                if frame_data.frame_time == 33 && frame_count > 100 {
                    // Loop detected, reset timing and frame count
                    frame_count = 0;
                    first_frame_time = Some(std::time::Instant::now());
                    next_frame_time = std::time::Instant::now();
                    info!("Loop detected, resetting frame count and timing");
                }

                // Log frame receive time gap
                if let Some(last) = last_frame_time {
                    let gap = last.elapsed();
                    if gap.as_millis() > 50 {
                        warn!("Frame receive gap: {:.2}ms (frame {})", gap.as_secs_f64() * 1000.0, frame_count);
                    }
                }
                last_frame_time = Some(std::time::Instant::now());

                // Check if this is a loop restart (frame_time reset to default 33ms)
                if frame_data.frame_time == 33 && frame_count > 100 {
                    // This might be a loop restart, reset timing
                    info!("Possible loop restart detected at frame {}, resetting timing", frame_count);
                    first_frame_time = Some(std::time::Instant::now());
                    next_frame_time = std::time::Instant::now();
                }

                // For the first frame, initialize timing
                let now = std::time::Instant::now();
                if first_frame_time.is_none() {
                    first_frame_time = Some(now);
                    next_frame_time = now;
                    info!("First frame received, starting playback");
                }

                let render_start = std::time::Instant::now();

                // Render frame to Wayland surface
                if let Err(e) = wayland_app.render_frame(&frame_data.frame, frame_data.width, frame_data.height) {
                    error!("Failed to render frame: {}", e);
                }

                // Process Wayland events
                if let Err(e) = wayland_app.dispatch_events() {
                    error!("Failed to dispatch Wayland events: {}", e);
                }

                let render_time = render_start.elapsed();

                // Calculate actual FPS based on time since first frame
                let fps = if let Some(first_time) = first_frame_time {
                    let elapsed = first_time.elapsed();
                    if elapsed.as_secs_f64() > 0.0 {
                        frame_count as f64 / elapsed.as_secs_f64()
                    } else {
                        0.0
                    }
                } else {
                    0.0
                };

                // Log every 30 frames
                if frame_count % 30 == 0 {
                    let total_elapsed = start_time.elapsed();
                    info!("Render {}: {}x{}, frame_time={}ms, render_time={:.2}ms, total_elapsed={:.2}s, FPS={:.2}",
                          frame_count, frame_data.width, frame_data.height,
                          frame_data.frame_time, render_time.as_secs_f64() * 1000.0,
                          total_elapsed.as_secs_f64(), fps);
                }

                // Calculate when next frame should be displayed based on video frame time
                next_frame_time += Duration::from_millis(frame_data.frame_time as u64);
                let now = std::time::Instant::now();

                // If we're ahead of schedule, wait
                if now < next_frame_time {
                    let sleep_time = next_frame_time.duration_since(now);
                    tokio::time::sleep(sleep_time).await;
                }
            }
            Ok(None) => {
                // Channel closed
                info!("Decode thread disconnected");
                break;
            }
            Err(_) => {
                // Timeout, no frame available, continue
                // This is normal when waiting for next frame
                continue;
            }
        }
    }

    info!("Render thread stopped, total frames rendered: {}", frame_count);
}
