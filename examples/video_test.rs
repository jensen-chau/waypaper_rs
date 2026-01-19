use waypaper_rs::wallpaper::video::VideoWallpaper;
use waypaper_rs::wallpaper::Wallpaper;
use std::time::Duration;
use std::thread;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();
    
    let video_path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "test.mp4".to_string());
    
    println!("Testing video playback with: {}", video_path);
    
    let mut wallpaper = VideoWallpaper::new(
        video_path,
        waypaper_rs::wallpaper::WallpaperType::Video,
    );
    
    // Run the wallpaper
    wallpaper.run();
    
    // Let it run for longer to see if frames are decoded
    println!("Running for 30 seconds...");
    thread::sleep(Duration::from_secs(30));
    
    // Stop the wallpaper
    wallpaper.stop();
    
    println!("Done!");
    
    Ok(())
}