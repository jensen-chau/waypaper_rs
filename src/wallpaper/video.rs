use crate::wallpaper::Wallpaper;

pub struct VideoWallpaper {
}

impl Wallpaper for VideoWallpaper {
    fn play() {
        println!("VideoWallpaper play");
    }
    
    fn pause() {
        println!("VideoWallpaper pause");
    }

}

