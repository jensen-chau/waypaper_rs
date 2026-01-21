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

    println!("Hardware acceleration: VAAPI enabled");

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