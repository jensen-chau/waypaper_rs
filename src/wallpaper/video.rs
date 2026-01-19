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
        let (tx, rx) = mpsc::channel::<FrameData>(30); // Buffer 30 frames
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
    
    // Try to create decoder with hardware acceleration
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
    
    let mut decoder = decoder.map_err(|e| anyhow::anyhow!("Failed to create decoder: {}", e))?;
    
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
        if *is_stopped.lock().await {
            info!("Decode thread stopped");
            break;
        }
        
        if *is_paused.lock().await {
            tokio::time::sleep(Duration::from_millis(10)).await;
            continue;
        }
        let decode_start = Instant::now();
        match decoder.decode() {
            Ok((timestamp, frame_data)) => {
                let decode_time = decode_start.elapsed();
                total_decode_time += decode_time;
                frame_count += 1;
                
                // Get frame dimensions from the frame data itself
                let shape = frame_data.shape();
                let height = shape[0] as u32;
                let width = shape[1] as u32;
                
                // Debug: log pixel format and first few pixels (RGB format)
                if frame_count % 30 == 0 {
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
                
                match tx.send(frame_data).await {
                    Ok(_) => {}
                    Err(_) => {
                        warn!("Render thread disconnected");
                        break;
                    }
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
                if error_msg.contains("end") || error_msg.contains("eof") || error_msg.contains("exhausted") {
                    // End of video, loop back
                    info!("Video ended, restarting");
                    
                    // Recreate decoder with hardware acceleration
                    let new_decoder = if available_hw.contains(&HardwareAccelerationDeviceType::VaApi) {
                        DecoderBuilder::new(std::path::Path::new(video_path))
                            .with_hardware_acceleration(HardwareAccelerationDeviceType::VaApi)
                            .build()
                    } else if available_hw.contains(&HardwareAccelerationDeviceType::Cuda) {
                        DecoderBuilder::new(std::path::Path::new(video_path))
                            .with_hardware_acceleration(HardwareAccelerationDeviceType::Cuda)
                            .build()
                    } else if available_hw.contains(&HardwareAccelerationDeviceType::VideoToolbox) {
                        DecoderBuilder::new(std::path::Path::new(video_path))
                            .with_hardware_acceleration(HardwareAccelerationDeviceType::VideoToolbox)
                            .build()
                    } else {
                        Decoder::new(std::path::Path::new(video_path))
                    };
                    
                    decoder = new_decoder.map_err(|e| anyhow::anyhow!("Failed to recreate decoder: {}", e))?;
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

// Hardware-accelerated decode using ffmpeg-next
#[allow(dead_code)]
#[allow(dead_code)]
async fn decode_video_hwaccel(
    video_path: &str,
    tx: mpsc::Sender<FrameData>,
    is_paused: Arc<Mutex<bool>>,
    is_stopped: Arc<Mutex<bool>>,
) -> Result<()> {
    use ffmpeg_next as ffmpeg;
    
    info!("Opening video with hardware acceleration: {}", video_path);
    
    // Initialize FFmpeg
    ffmpeg::init()?;
    
    // Open input
    let mut input = ffmpeg::format::input(&video_path)?;
    info!("Input format: {}", input.format().name());
    
    // Find video stream
    let video_stream_index = input
        .streams()
        .best(ffmpeg::media::Type::Video)
        .ok_or_else(|| anyhow::anyhow!("No video stream found"))?
        .index();
    
    let video_stream = input.stream(video_stream_index).unwrap();
    let video_stream_params = video_stream.parameters();
    
    info!("Video codec: {}", video_stream_params.id().name());
    
    // Find decoder
    let codec_id = video_stream_params.id();
    let decoder = ffmpeg::codec::decoder::find(codec_id)
        .ok_or_else(|| anyhow::anyhow!("Decoder not found for codec: {}", codec_id.name()))?;
    
    info!("Using decoder: {}", decoder.name());
    
    // Create decoder context
    let mut decoder_context = ffmpeg::codec::context::Context::new_with_codec(decoder);
    decoder_context.set_parameters(video_stream_params)?;
    
    // Try to enable hardware acceleration
    // Note: Hardware acceleration support in ffmpeg-next is limited
    // We'll try to use a hardware-accelerated codec if available
    info!("Hardware acceleration support: Limited in current implementation");
    
    let mut decoder = decoder_context.decoder().video()?;
    
    info!("Decoder initialized");
    info!("Video size: {}x{}", decoder.width(), decoder.height());
    info!("Pixel format: {:?}", decoder.format());
    
    let mut frame_count = 0u64;
    let mut last_pts: Option<i64> = None;
    let mut frame_time_ms: u32 = 33;
    let mut total_decode_time = Duration::from_secs(0);
    let mut total_convert_time = Duration::from_secs(0);
    
    let packet = ffmpeg::packet::Packet::empty();
    let mut frame = ffmpeg::frame::Video::empty();
    
    // Get time_base before loop to avoid borrow issues
    let time_base_num = video_stream.time_base().numerator() as i64;
    let time_base_den = video_stream.time_base().denominator() as i64;
    
    loop {
        if *is_stopped.lock().await {
            info!("Decode thread stopped");
            break;
        }
        
        if *is_paused.lock().await {
            tokio::time::sleep(Duration::from_millis(10)).await;
            continue;
        }
        
        // Read packet
        let decode_start = std::time::Instant::now();
        let mut packets = input.packets();
        let mut has_packet = false;
        
        for (stream, packet) in packets.by_ref() {
            if stream.index() != video_stream_index {
                continue;
            }
            
            has_packet = true;
            
            // Send packet to decoder
            decoder.send_packet(&packet)?;
            
            // Receive frame from decoder
            while decoder.receive_frame(&mut frame).is_ok() {
                let decode_time = decode_start.elapsed();
                total_decode_time += decode_time;
                frame_count += 1;
                
                let width = frame.width();
                let height = frame.height();
                
                // Convert frame data to RGBA
                let convert_start = std::time::Instant::now();
                let rgba_data = convert_ffmpeg_frame_to_rgba(&frame, width, height)?;
                let convert_time = convert_start.elapsed();
                total_convert_time += convert_time;
                
                // Calculate frame time from PTS
                if let Some(pts) = frame.pts() {
                    if let Some(last) = last_pts {
                        let pts_diff = pts - last;
                        let time_ms = (pts_diff * 1000 * time_base_num / time_base_den) as u32;
                        if time_ms > 0 && time_ms < 1000 {
                            frame_time_ms = time_ms;
                        }
                    }
                    last_pts = Some(pts);
                }
                
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
                
                if frame_count % 30 == 0 {
                    let avg_decode = total_decode_time.as_secs_f64() * 1000.0 / 30.0;
                    let avg_convert = total_convert_time.as_secs_f64() * 1000.0 / 30.0;
                    info!("Decoded {} frames, frame time: {}ms, avg_decode={:.2}ms, avg_convert={:.2}ms", 
                          frame_count, frame_time_ms, avg_decode, avg_convert);
                    total_decode_time = Duration::from_secs(0);
                    total_convert_time = Duration::from_secs(0);
                }
            }
            break; // Process one packet at a time
        }
        
        // Check if we reached EOF
        if !has_packet {
            info!("Video ended, restarting");
            input = ffmpeg::format::input(&video_path)?;
            frame_count = 0;
            last_pts = None;
            frame_time_ms = 33;
        }
    }
    
    Ok(())
}

fn convert_ffmpeg_frame_to_rgba(frame: &ffmpeg_next::frame::Video, width: u32, height: u32) -> Result<Vec<u8>> {
    use ffmpeg_next as ffmpeg;
    
    let pixel_count = (width * height) as usize;
    let mut rgba_data = vec![0u8; pixel_count * 4];
    
    // Get frame data
    let data = frame.data(0);
    let linesize = frame.stride(0);
    
    // Convert based on pixel format
    unsafe {
        match frame.format() {
            ffmpeg::util::format::pixel::Pixel::YUV420P => {
                // YUV420P to RGB conversion
                let y_plane = data.as_ptr();
                let u_plane = data.as_ptr().add(linesize as usize * height as usize);
                let v_plane = u_plane.add(linesize as usize * height as usize / 4);
                
                let mut rgba_idx = 0;
                for y in 0..height as usize {
                    for x in 0..width as usize {
                        let y_idx = y * linesize as usize + x;
                        let u_idx = (y / 2) * (linesize as usize / 2) + (x / 2);
                        let v_idx = u_idx;
                        
                        let y_val = *y_plane.add(y_idx) as f64;
                        let u_val = *u_plane.add(u_idx) as f64 - 128.0;
                        let v_val = *v_plane.add(v_idx) as f64 - 128.0;
                        
                        let r = (y_val + 1.402 * v_val) as u8;
                        let g = (y_val - 0.344136 * u_val - 0.714136 * v_val) as u8;
                        let b = (y_val + 1.772 * u_val) as u8;
                        
                        rgba_data[rgba_idx] = r;
                        rgba_data[rgba_idx + 1] = g;
                        rgba_data[rgba_idx + 2] = b;
                        rgba_data[rgba_idx + 3] = 255;
                        rgba_idx += 4;
                    }
                }
            }
            ffmpeg::util::format::pixel::Pixel::RGB24 => {
                // RGB24 to RGBA
                let rgb_ptr = data.as_ptr();
                let rgba_ptr = rgba_data.as_mut_ptr();
                
                for i in 0..pixel_count {
                    let rgb_idx = i * 3;
                    let rgba_idx = i * 4;
                    
                    *rgba_ptr.add(rgba_idx) = *rgb_ptr.add(rgb_idx);
                    *rgba_ptr.add(rgba_idx + 1) = *rgb_ptr.add(rgb_idx + 1);
                    *rgba_ptr.add(rgba_idx + 2) = *rgb_ptr.add(rgb_idx + 2);
                    *rgba_ptr.add(rgba_idx + 3) = 255;
                }
            }
            _ => {
                return Err(anyhow::anyhow!("Unsupported pixel format: {:?}", frame.format()));
            }
        }
    }
    
    Ok(rgba_data)
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
    let target_fps = 60.0; // Target 60 FPS
    let frame_duration = Duration::from_secs_f64(1.0 / target_fps);
    let mut total_render_time = Duration::from_secs(0);
    
    while !*is_stopped.lock().await {
        if *is_paused.lock().await {
            tokio::time::sleep(Duration::from_millis(10)).await;
            continue;
        }
        
        // Try to receive a frame - use timeout for non-blocking
        match tokio::time::timeout(Duration::from_millis(1), rx.recv()).await {
            Ok(Some(frame_data)) => {
                frame_count += 1;
                let render_start = std::time::Instant::now();
                
                // Render frame to Wayland surface
                if let Err(e) = wayland_app.render_frame(&frame_data.frame, frame_data.width, frame_data.height) {
                    error!("Failed to render frame: {}", e);
                }
                
                let render_time = render_start.elapsed();
                total_render_time += render_time;
                let total_elapsed = start_time.elapsed();
                
                // Calculate actual FPS based on total time
                let fps = if total_elapsed.as_secs_f64() > 0.0 {
                    frame_count as f64 / total_elapsed.as_secs_f64()
                } else {
                    0.0
                };
                
                // Log every 30 frames
                if frame_count % 30 == 0 {
                    let avg_render = total_render_time.as_secs_f64() * 1000.0 / 30.0;
                    info!("Render {}: {}x{}, frame_time={}ms, avg_render={:.2}ms, total_elapsed={:.2}s, FPS={:.2}",
                          frame_count, frame_data.width, frame_data.height, 
                          frame_data.frame_time, avg_render,
                          total_elapsed.as_secs_f64(), fps);
                    total_render_time = Duration::from_secs(0);
                }
                
                // Control frame rate - sleep if we rendered too fast
                if render_time < frame_duration {
                    let sleep_time = frame_duration.saturating_sub(render_time);
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
                continue;
            }
        }
    }
    
    info!("Render thread stopped, total frames rendered: {}", frame_count);
}