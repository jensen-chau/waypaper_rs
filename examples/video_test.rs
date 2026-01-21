use waypaper_rs::wallpaper::video_hw::{VideoWallpaper, HardwareAcceleration};
use waypaper_rs::wallpaper::Wallpaper;
use std::time::Duration;
use tokio::time::sleep;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();

    let video_path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "test.mp4".to_string());

    println!("Testing video playback with: {}", video_path);

    let mut wallpaper = VideoWallpaper::new(
        video_path,
        waypaper_rs::wallpaper::WallpaperType::Video,
    );

    // 设置硬件加速类型（VAAPI 适用于 Linux Intel/AMD GPU）
    // 可选的硬件加速类型：
    // - VAAPI (Linux Intel/AMD GPU)
    // - CUDA (NVIDIA GPU)
    // - VDPAU (Linux NVIDIA GPU)
    // - QSV (Intel Quick Sync Video)
    // - VideoToolbox (macOS)
    // - D3D11VA (Windows)
    // - None (软件解码)
    wallpaper.set_hardware_acceleration(HardwareAcceleration::VAAPI);

    // === 性能优化配置 ===
    
    // 1. 设置目标帧率（默认 30fps，可设置 15-60fps）
    // 降低帧率可以显著减少 CPU 占用
    wallpaper.set_target_fps(30);
    
    // 2. 设置最大分辨率（默认 1920x1080）
    // 降低分辨率可以大幅减少 CPU 和内存占用
    // 4K (3840x2160) -> 1080p (1920x1080) 可以减少约 75% 的数据量
    wallpaper.set_max_resolution(1920, 1080);
    
    // 如果需要使用原始分辨率，可以调用：
    // wallpaper.disable_resolution_limit();
    
    println!("Hardware acceleration: VAAPI enabled");
    println!("Target FPS: 30");
    println!("Max resolution: 1920x1080");

    // Run the wallpaper
    wallpaper.run();

    // Let it run for longer to see if frames are decoded
    println!("Running for 30 seconds...");
    sleep(Duration::from_secs(30)).await;

    // Stop the wallpaper
    wallpaper.stop();

    println!("Done!");

    Ok(())
}