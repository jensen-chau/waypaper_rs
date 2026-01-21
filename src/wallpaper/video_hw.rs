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

/// 硬件加速类型
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HardwareAcceleration {
    /// VAAPI (Video Acceleration API) - Linux Intel/AMD GPU
    VAAPI,
    /// CUDA - NVIDIA GPU
    CUDA,
    /// VDPAU (Video Decode and Presentation API for Unix) - Linux NVIDIA GPU
    VDPAU,
    /// QSV (Quick Sync Video) - Intel GPU
    QSV,
    /// VideoToolbox - macOS
    VideoToolbox,
    /// D3D11VA - Windows
    D3D11VA,
    /// 无硬件加速（软件解码）
    None,
}

impl HardwareAcceleration {
    /// 获取硬件设备的名称
    pub fn device_name(&self) -> &'static str {
        match self {
            HardwareAcceleration::VAAPI => "/dev/dri/renderD128",  // VAAPI 需要设备路径
            HardwareAcceleration::CUDA => "cuda",
            HardwareAcceleration::VDPAU => "vdpau",
            HardwareAcceleration::QSV => "qsv",
            HardwareAcceleration::VideoToolbox => "videotoolbox",
            HardwareAcceleration::D3D11VA => "d3d11va",
            HardwareAcceleration::None => "",
        }
    }

    /// 获取 FFmpeg AVHWDeviceType
    pub fn av_hwdevice_type(&self) -> ffmpeg::ffi::AVHWDeviceType {
        match self {
            HardwareAcceleration::VAAPI => ffmpeg::ffi::AVHWDeviceType::AV_HWDEVICE_TYPE_VAAPI,
            HardwareAcceleration::CUDA => ffmpeg::ffi::AVHWDeviceType::AV_HWDEVICE_TYPE_CUDA,
            HardwareAcceleration::VDPAU => ffmpeg::ffi::AVHWDeviceType::AV_HWDEVICE_TYPE_VDPAU,
            HardwareAcceleration::QSV => ffmpeg::ffi::AVHWDeviceType::AV_HWDEVICE_TYPE_QSV,
            HardwareAcceleration::VideoToolbox => ffmpeg::ffi::AVHWDeviceType::AV_HWDEVICE_TYPE_VIDEOTOOLBOX,
            HardwareAcceleration::D3D11VA => ffmpeg::ffi::AVHWDeviceType::AV_HWDEVICE_TYPE_D3D11VA,
            HardwareAcceleration::None => ffmpeg::ffi::AVHWDeviceType::AV_HWDEVICE_TYPE_NONE,
        }
    }

    /// 获取硬件像素格式
    pub fn hw_pixel_format(&self) -> ffmpeg::format::Pixel {
        match self {
            HardwareAcceleration::VAAPI => ffmpeg::format::Pixel::VAAPI,
            HardwareAcceleration::CUDA => ffmpeg::format::Pixel::CUDA,
            HardwareAcceleration::VDPAU => ffmpeg::format::Pixel::VDPAU,
            HardwareAcceleration::QSV => ffmpeg::format::Pixel::QSV,
            HardwareAcceleration::VideoToolbox => ffmpeg::format::Pixel::VIDEOTOOLBOX,
            HardwareAcceleration::D3D11VA => ffmpeg::format::Pixel::D3D11,
            HardwareAcceleration::None => panic!("None has no hw pixel format"),
        }
    }
}

/// 硬件解码器包装器
pub struct HardwareDecoder {
    hw_device_ctx: Option<*mut ffmpeg::ffi::AVBufferRef>,
    hw_frames_ctx: Option<*mut ffmpeg::ffi::AVBufferRef>,
    hw_accel_type: HardwareAcceleration,
}

unsafe impl Send for HardwareDecoder {}
unsafe impl Sync for HardwareDecoder {}

impl HardwareDecoder {
    /// 创建新的硬件解码器
    pub fn new(hw_accel_type: HardwareAcceleration) -> Result<Self> {
        match hw_accel_type {
            HardwareAcceleration::None => Ok(Self {
                hw_device_ctx: None,
                hw_frames_ctx: None,
                hw_accel_type,
            }),
            _ => {
                let device_name = hw_accel_type.device_name();
                let hw_device_type = hw_accel_type.av_hwdevice_type();
                info!("Initializing hardware device: {} (type: {:?})", device_name, hw_device_type);

                let mut hw_device_ctx_ptr: *mut ffmpeg::ffi::AVBufferRef = std::ptr::null_mut();

                // 将 device_name 转换为 CString 以确保正确的 null 终止
                let device_name_cstr = std::ffi::CString::new(device_name)
                    .map_err(|e| anyhow::anyhow!("Failed to create CString: {}", e))?;

                // 调用 FFmpeg 的 av_hwdevice_ctx_create
                let ret = unsafe {
                    ffmpeg::ffi::av_hwdevice_ctx_create(
                        &mut hw_device_ctx_ptr,
                        hw_device_type,
                        device_name_cstr.as_ptr(),
                        std::ptr::null_mut(),
                        0,
                    )
                };

                if ret < 0 {
                    return Err(anyhow::anyhow!(
                        "Failed to create hardware device context: error code {}",
                        ret
                    ));
                }

                if hw_device_ctx_ptr.is_null() {
                    return Err(anyhow::anyhow!(
                        "Hardware device context is null after creation"
                    ));
                }

                info!("Hardware device context created successfully");

                Ok(Self {
                    hw_device_ctx: Some(hw_device_ctx_ptr),
                    hw_frames_ctx: None,
                    hw_accel_type,
                })
            }
        }
    }

    /// 配置解码器使用硬件加速
    pub fn configure_decoder(
        &mut self,
        decoder: &mut ffmpeg::codec::decoder::Video,
    ) -> Result<()> {
        if self.hw_accel_type == HardwareAcceleration::None {
            return Ok(());
        }

        info!("Configuring decoder for hardware acceleration");

        unsafe {
            let codec_ctx = decoder.as_mut_ptr();
            let hw_device_ctx = self.hw_device_ctx.unwrap();

            // 设置硬件设备上下文
            (*codec_ctx).hw_device_ctx = ffmpeg::ffi::av_buffer_ref(hw_device_ctx);

            if (*codec_ctx).hw_device_ctx.is_null() {
                return Err(anyhow::anyhow!(
                    "Failed to set hw_device_ctx in decoder context"
                ));
            }

            info!("Hardware device context set in decoder");
        }

        Ok(())
    }

    /// 从硬件帧传输到软件帧
    pub fn transfer_frame(&self, hw_frame: &Video, sw_frame: &mut Video) -> Result<()> {
        if self.hw_accel_type == HardwareAcceleration::None {
            return Err(anyhow::anyhow!("No hardware acceleration enabled"));
        }

        unsafe {
            let hw_frame_ptr = hw_frame.as_ptr();
            let sw_frame_ptr = sw_frame.as_mut_ptr();

            let ret = ffmpeg::ffi::av_hwframe_transfer_data(
                sw_frame_ptr,
                hw_frame_ptr,
                0,
            );

            if ret < 0 {
                return Err(anyhow::anyhow!(
                    "Failed to transfer frame from hardware to software: error code {}",
                    ret
                ));
            }

            // 复制帧属性（时间戳等）
            let ret = ffmpeg::ffi::av_frame_copy_props(sw_frame_ptr, hw_frame_ptr);
            if ret < 0 {
                return Err(anyhow::anyhow!(
                    "Failed to copy frame properties: error code {}",
                    ret
                ));
            }
        }

        Ok(())
    }
}

impl Drop for HardwareDecoder {
    fn drop(&mut self) {
        if let Some(mut hw_device_ctx) = self.hw_device_ctx {
            unsafe {
                ffmpeg::ffi::av_buffer_unref(&mut hw_device_ctx);
            }
        }
        if let Some(mut hw_frames_ctx) = self.hw_frames_ctx {
            unsafe {
                ffmpeg::ffi::av_buffer_unref(&mut hw_frames_ctx);
            }
        }
    }
}

pub struct VideoWallpaper {
    video_path: String,
    is_paused: Arc<Mutex<bool>>,
    is_stopped: Arc<Mutex<bool>>,
    decode_task: Option<JoinHandle<()>>,
    render_task: Option<JoinHandle<()>>,
    project: Option<project::Project>,
    wallpaper_type: WallpaperType,
    hw_accel_type: HardwareAcceleration,
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
            hw_accel_type: HardwareAcceleration::VAAPI, // 默认使用 VAAPI
        }
    }

    /// 设置硬件加速类型
    pub fn set_hardware_acceleration(&mut self, hw_accel_type: HardwareAcceleration) {
        self.hw_accel_type = hw_accel_type;
    }

    pub fn stop(&mut self) {
        info!("VideoWallpaper stop requested (async tasks will check flag)");
    }
}

impl Wallpaper for VideoWallpaper {
    fn play(&mut self) {
        info!("VideoWallpaper play requested");
    }

    fn pause(&mut self) {
        info!("VideoWallpaper pause requested");
    }

    fn run(&mut self) {
        let (tx, rx) = mpsc::channel::<FrameData>(60);
        let video_path = self.video_path.clone();
        let is_paused = self.is_paused.clone();
        let is_stopped = self.is_stopped.clone();
        let hw_accel_type = self.hw_accel_type;

        let is_paused_render = is_paused.clone();
        let is_stopped_render = is_stopped.clone();

        let handle = tokio::runtime::Handle::current();

        let decode_task = handle.spawn(async move {
            if let Err(e) = decode_video_async(&video_path, tx, is_paused, is_stopped, hw_accel_type).await {
                error!("Video decode error: {}", e);
            }
        });
        self.decode_task = Some(decode_task);

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
    hw_accel_type: HardwareAcceleration,
) -> Result<()> {
    info!("decode_video_async started with hardware acceleration: {:?}", hw_accel_type);
    let video_path = video_path.to_string();
    let output_width = 1920u32;
    let output_height = 1080u32;

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

        // Initialize hardware decoder if enabled
        let mut hw_decoder = HardwareDecoder::new(hw_accel_type)?;
        hw_decoder.configure_decoder(&mut decoder)?;
        info!("Hardware decoder configured: {:?}", hw_accel_type);

        info!("Video opened: {}x{} -> {}x{} (BGRA)",
              decoder.width(), decoder.height(), output_width, output_height);

        let mut frame_count = 0u64;
        let mut last_pts: Option<i64> = None;
        let mut frame_time_ms: u32 = 33;

        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| anyhow::anyhow!("Failed to create runtime: {}", e))?;

        let result = rt.block_on(async move {
            let mut decoder = decoder;

            info!("Starting decode loop...");
            let mut packet_count = 0u64;

            // 先解码第一帧以获取实际的硬件帧格式
            let mut scaler: Option<Context> = None;
            let mut first_frame_decoded = false;

            loop {
                // 每 100 帧才检查一次 stop 标志，减少锁竞争
                if frame_count % 100 == 0 && *is_stopped.lock().await {
                    info!("Decode thread stopped");
                    break Ok(());
                }

                // 每 10 帧检查一次暂停标志
                if frame_count % 10 == 0 && *is_paused.lock().await {
                    tokio::time::sleep(Duration::from_millis(100)).await;
                    continue;
                }

                let (stream, packet) = match ictx.packets().next() {
                    Some((s, p)) => (s, p),
                    None => {
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
                    if let Err(e) = decoder.send_packet(&packet) {
                        error!("Failed to send packet to decoder: {}", e);
                        break Err(anyhow::anyhow!("Decoder error"));
                    }

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

                            // Check if frame is in hardware format
                            let frame_format = decoded.format();
                            let is_hw_frame = matches!(frame_format,
                                ffmpeg::format::Pixel::VAAPI |
                                ffmpeg::format::Pixel::CUDA |
                                ffmpeg::format::Pixel::VDPAU |
                                ffmpeg::format::Pixel::QSV |
                                ffmpeg::format::Pixel::VIDEOTOOLBOX |
                                ffmpeg::format::Pixel::D3D11
                            );

                            let bgra_frame = if is_hw_frame {
                                // Transfer from hardware to software
                                let mut sw_frame = Video::empty();
                                hw_decoder.transfer_frame(&decoded, &mut sw_frame)?;

                                // 在第一帧传输后创建缩放器
                                if !first_frame_decoded {
                                    let sw_format = sw_frame.format();
                                    let sw_width = sw_frame.width();
                                    let sw_height = sw_frame.height();
                                    info!("Creating scaler for software frame: {}x{} format: {:?}", sw_width, sw_height, sw_format);

                                    // 如果尺寸相同，不创建缩放器
                                    if sw_width == output_width && sw_height == output_height && sw_format == ffmpeg::format::Pixel::BGRA {
                                        info!("No scaling needed, dimensions and format match");
                                        first_frame_decoded = true;
                                    } else {
                                        scaler = Some(Context::get(
                                            sw_format,
                                            sw_width,
                                            sw_height,
                                            ffmpeg::format::Pixel::BGRA,
                                            output_width,
                                            output_height,
                                            Flags::FAST_BILINEAR, // 使用更快的算法
                                        ).map_err(|e| anyhow::anyhow!("Failed to create scaler: {}", e))?);
                                        first_frame_decoded = true;
                                    }
                                }

                                sw_frame
                            } else {
                                // 如果已经是软件帧，检查是否需要缩放
                                if !first_frame_decoded {
                                    let sw_format = decoded.format();
                                    let sw_width = decoded.width();
                                    let sw_height = decoded.height();
                                    info!("Creating scaler for software frame: {}x{} format: {:?}", sw_width, sw_height, sw_format);

                                    // 如果尺寸相同，不创建缩放器
                                    if sw_width == output_width && sw_height == output_height && sw_format == ffmpeg::format::Pixel::BGRA {
                                        info!("No scaling needed, dimensions and format match");
                                        first_frame_decoded = true;
                                    } else {
                                        scaler = Some(Context::get(
                                            sw_format,
                                            sw_width,
                                            sw_height,
                                            ffmpeg::format::Pixel::BGRA,
                                            output_width,
                                            output_height,
                                            Flags::FAST_BILINEAR, // 使用更快的算法
                                        ).map_err(|e| anyhow::anyhow!("Failed to create scaler: {}", e))?);
                                        first_frame_decoded = true;
                                    }
                                }
                                decoded
                            };

                            // Scale and convert frame to BGRA
                            let mut final_bgra_frame = Video::empty();
                            if let Some(ref mut scaler) = scaler {
                                scaler.run(&bgra_frame, &mut final_bgra_frame)
                                    .map_err(|e| anyhow::anyhow!("Failed to scale frame: {}", e))?;
                            } else {
                                // No scaler needed, use as-is
                                final_bgra_frame = bgra_frame;
                            }

                            let frame_data = extract_frame_data(&final_bgra_frame, output_width, output_height)?;

                            if frame_count % 60 == 0 {
                                info!("Frame {} - {}x{} - Hardware: {}",
                                      frame_count, output_width, output_height, is_hw_frame);
                            }

                            if let Some(last) = last_pts {
                                let pts_diff = (pts - last) as f64;
                                let time_ms = (pts_diff * time_base.numerator() as f64 / time_base.denominator() as f64 * 1000.0) as u32;
                                if time_ms > 0 && time_ms < 1000 {
                                    frame_time_ms = time_ms;
                                }
                            }
                            last_pts = Some(pts);

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
    let row_size = width * 4;
    let mut frame_data = vec![0u8; row_size * height];

    unsafe {
        let src_ptr = data.as_ptr();
        let dst_ptr = frame_data.as_mut_ptr();

        // 使用 memcpy 逐行拷贝，比逐像素拷贝快得多
        for y in 0..height {
            let src_row = src_ptr.add(y * stride);
            let dst_row = dst_ptr.add(y * row_size);
            std::ptr::copy_nonoverlapping(src_row, dst_row, row_size);
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
    let mut next_frame_time = start_time;
    let mut last_frame_time: Option<std::time::Instant> = None;

    while !*is_stopped.lock().await {
        if *is_paused.lock().await {
            // 暂停时使用更长的 sleep 时间，减少 CPU 占用
            tokio::time::sleep(Duration::from_millis(100)).await;
            continue;
        }

        // 使用阻塞 recv() 而不是 timeout，减少轮询
        match rx.recv().await {
            Some(frame_data) => {
                frame_count += 1;

                if frame_data.frame_time == 33 && frame_count > 100 {
                    frame_count = 0;
                    first_frame_time = Some(std::time::Instant::now());
                    next_frame_time = std::time::Instant::now();
                    info!("Loop detected, resetting frame count and timing");
                }

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

                let now = std::time::Instant::now();
                if first_frame_time.is_none() {
                    first_frame_time = Some(now);
                    next_frame_time = now;
                    info!("First frame received, starting playback");
                }

                let render_start = std::time::Instant::now();

                if let Err(e) =
                    wayland_app.render_frame(&frame_data.frame, frame_data.width, frame_data.height)
                {
                    error!("Failed to render frame: {}", e);
                }

                // 只在每 5 帧才 dispatch 一次，减少 CPU 占用
                if frame_count % 5 == 0 {
                    if let Err(e) = wayland_app.dispatch_events() {
                        error!("Failed to dispatch Wayland events: {}", e);
                    }
                }

                let render_time = render_start.elapsed();

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

                next_frame_time += Duration::from_millis(frame_data.frame_time as u64);
                let now = std::time::Instant::now();

                if now < next_frame_time {
                    let sleep_time = next_frame_time.duration_since(now);
                    tokio::time::sleep(sleep_time).await;
                }
            }
            None => {
                info!("Decode thread disconnected");
                break;
            }
        }
    }

    info!(
        "Render thread stopped, total frames rendered: {}",
        frame_count
    );
}