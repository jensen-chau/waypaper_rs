use log::{error, info, warn};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{Mutex, mpsc};
use tokio::task::JoinHandle;

use crate::wallpaper::Wallpaper;
use crate::wallpaper::{WallpaperType, project};
use anyhow::Result;
use ffmpeg_next as ffmpeg;

use ffmpeg::format::input;
use ffmpeg::media::Type;
use ffmpeg::software::scaling::{context::Context, flag::Flags};
use ffmpeg::util::frame::video::Video;
pub struct VideoWallpaper {
    video_path: String,
    is_paused: Arc<Mutex<bool>>,
    is_stopped: Arc<Mutex<bool>>,
    decode_task: Option<JoinHandle<()>>,
    render_task: Option<JoinHandle<()>>,
    project: Option<project::Project>,
    wallpaper_type: WallpaperType,
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

    fn info(&self) {}
}

async fn decode_video_async(
    video_path: &str,
    tx: mpsc::Sender<FrameData>,
    is_paused: Arc<Mutex<bool>>,
    is_stopped: Arc<Mutex<bool>>,
) -> Result<()> {
    info!("decode_video_async started");
    let video_path = video_path.to_string();
    let output_width = 1920u32;
    let output_height = 1080u32;

    // Run all ffmpeg operations in a blocking thread
    tokio::task::spawn_blocking::<_, Result<()>>(move || {


        info!("spawn_blocking thread started");

        // Initialize ffmpeg
        info!("Initializing ffmpeg...");
        ffmpeg::init().map_err(|e| anyhow::anyhow!("Failed to initialize ffmpeg: {}", e))?;
        info!("ffmpeg initialized successfully");

        info!("Opening video: {}", video_path);

        // Open input file
        let mut ictx = input(&video_path)
            .map_err(|e| anyhow::anyhow!("Failed to open video file: {}", e))?;
        info!("Video file opened successfully");

        // Find best video stream
        let input_stream = ictx
            .streams()
            .best(Type::Video)
            .ok_or_else(|| anyhow::anyhow!("No video stream found"))?;
        let video_stream_index = input_stream.index();
        info!("Found video stream at index {}", video_stream_index);

        // Get stream time base for timestamp conversion
        let time_base = input_stream.time_base();
        info!("Stream time base: {}/{}", time_base.numerator(), time_base.denominator());

// Create decoder
        let context_decoder = ffmpeg::codec::context::Context::from_parameters(input_stream.parameters())
            .map_err(|e| anyhow::anyhow!("Failed to create decoder context: {}", e))?;
        let mut decoder = context_decoder.decoder().video()
            .map_err(|e| anyhow::anyhow!("Failed to create video decoder: {}", e))?;

        info!("Decoder created successfully");

        info!("Video opened: {}x{} -> {}x{} (BGRA)",
              decoder.width(), decoder.height(), output_width, output_height);

        let mut frame_count = 0u64;
        let mut last_pts: Option<i64> = None;
        let mut frame_time_ms: u32 = 33; // Default to ~30fps

        // Create runtime for async operations
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| anyhow::anyhow!("Failed to create runtime: {}", e))?;

        let result = rt.block_on(async move {
            let mut decoder = decoder;

            info!("Starting decode loop...");
            let mut packet_count = 0u64;

            // Create reusable scaler
            let mut scaler = Context::get(
                decoder.format(),
                decoder.width(),
                decoder.height(),
                ffmpeg::format::Pixel::BGRA,
                output_width,
                output_height,
                Flags::BILINEAR,
            ).map_err(|e| anyhow::anyhow!("Failed to create scaler: {}", e))?;
            info!("Scaler created successfully");

            loop {
                // Check stop flag every frame (critical for shutdown)
                if *is_stopped.lock().await {
                    info!("Decode thread stopped");
                    break Ok(());
                }

                // Only check pause flag every 10 frames to reduce lock contention
                if frame_count % 10 == 0 && *is_paused.lock().await {
                    tokio::time::sleep(Duration::from_millis(100)).await;
                    continue;
                }

                // Read next packet
                let (stream, packet) = match ictx.packets().next() {
                    Some((s, p)) => (s, p),
                    None => {
                        // End of stream, loop back
                        info!("Video ended, seeking to beginning");
                        let _ = ictx.seek(0, ..);
                        frame_count = 0;
                        last_pts = None;
                        frame_time_ms = 33;
                        continue;
                    }
                };

                packet_count += 1;
                if packet_count % 100 == 0 {
                    info!("Processed {} packets", packet_count);
                }

                if stream.index() == video_stream_index {
                    // Send packet to decoder
                    if let Err(e) = decoder.send_packet(&packet) {
                        error!("Failed to send packet to decoder: {}", e);
                        break Err(anyhow::anyhow!("Decoder error"));
                    }

                    // Receive decoded frames
                    let mut decoded = Video::empty();
                    match decoder.receive_frame(&mut decoded) {
                        Ok(_) => {
                            let pts = match decoded.pts() {
                                Some(p) => p,
                                None => continue,
                            };

                            frame_count += 1;

                            if frame_count == 1 {
                                info!("Successfully decoded first frame");
                            }

                            // Scale and convert frame to BGRA (reuse scaler)
                            let mut bgra_frame = Video::empty();
                            scaler.run(&decoded, &mut bgra_frame)
                                .map_err(|e| anyhow::anyhow!("Failed to scale frame: {}", e))?;

                            let frame_data = extract_frame_data(&bgra_frame, output_width, output_height)?;

                            // Debug: log first few pixels (BGRA format)
                            if frame_count % 60 == 0 {
                                info!("Frame {} - {}x{} - First 2 pixels (BGRA): B={}, G={}, R={}, A={}, B={}, G={}, R={}, A={}",
                                      frame_count, output_width, output_height,
                                      frame_data[0], frame_data[1], frame_data[2], frame_data[3],
                                      frame_data[4], frame_data[5], frame_data[6], frame_data[7]);
                            }

                            // Calculate frame time from PTS difference
                            if let Some(last) = last_pts {
                                let pts_diff = (pts - last) as f64;
                                let time_ms = (pts_diff * time_base.numerator() as f64 / time_base.denominator() as f64 * 1000.0) as u32;
                                if time_ms > 0 && time_ms < 1000 {
                                    frame_time_ms = time_ms;
                                }
                            }
                            last_pts = Some(pts);

                            // Send frame data
                            let frame_data = FrameData {
                                frame: frame_data,
                                width: output_width,
                                height: output_height,
                                frame_time: frame_time_ms,
                            };

                            if tx.send(frame_data).await.is_err() {
                                warn!("Render thread disconnected");
                                break Err(anyhow::anyhow!("Render thread disconnected"));
                            }

                            if frame_count % 60 == 0 {
                                info!("Decoded {} frames, frame time: {}ms", frame_count, frame_time_ms);
                            }
                        }
                        Err(ffmpeg::Error::Eof) | Err(ffmpeg::Error::Other { errno: 11, .. }) => {
                            // No frame available, continue
                        }
                        Err(e) => {
                            error!("Failed to receive frame: {}", e);
                            break Err(anyhow::anyhow!("Failed to receive frame: {}", e));
                        }
                    }
                }
            }
        });

        result
    }).await.map_err(|e| anyhow::anyhow!("Spawn blocking task failed: {}", e))?
}

/// Extract frame data from Video frame
fn extract_frame_data(
    frame: &ffmpeg::util::frame::video::Video,
    width: u32,
    height: u32,
) -> Result<Vec<u8>> {
    let stride = frame.stride(0);
    let data = frame.data(0);

    let width = width as usize;
    let height = height as usize;
    let mut frame_data = vec![0u8; width * height * 4];

    unsafe {
        let src_ptr = data.as_ptr();
        let dst_ptr = frame_data.as_mut_ptr();

        for y in 0..height {
            let src_row_start = y * stride;
            let dst_row_start = y * width * 4;

            for x in 0..width {
                let src_idx = src_row_start + x * 4;
                let dst_idx = dst_row_start + x * 4;

                *dst_ptr.add(dst_idx) = *src_ptr.add(src_idx); // B
                *dst_ptr.add(dst_idx + 1) = *src_ptr.add(src_idx + 1); // G
                *dst_ptr.add(dst_idx + 2) = *src_ptr.add(src_idx + 2); // R
                *dst_ptr.add(dst_idx + 3) = *src_ptr.add(src_idx + 3); // A
            }
        }
    }

    Ok(frame_data)
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
                        warn!(
                            "Frame receive gap: {:.2}ms (frame {})",
                            gap.as_secs_f64() * 1000.0,
                            frame_count
                        );
                    }
                }
                last_frame_time = Some(std::time::Instant::now());

                // Check if this is a loop restart (frame_time reset to default 33ms)
                if frame_data.frame_time == 33 && frame_count > 100 {
                    // This might be a loop restart, reset timing
                    info!(
                        "Possible loop restart detected at frame {}, resetting timing",
                        frame_count
                    );
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
                if let Err(e) =
                    wayland_app.render_frame(&frame_data.frame, frame_data.width, frame_data.height)
                {
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
                    info!(
                        "Render {}: {}x{}, frame_time={}ms, render_time={:.2}ms, total_elapsed={:.2}s, FPS={:.2}",
                        frame_count,
                        frame_data.width,
                        frame_data.height,
                        frame_data.frame_time,
                        render_time.as_secs_f64() * 1000.0,
                        total_elapsed.as_secs_f64(),
                        fps
                    );
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

    info!(
        "Render thread stopped, total frames rendered: {}",
        frame_count
    );
}
