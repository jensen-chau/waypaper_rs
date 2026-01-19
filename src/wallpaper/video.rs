use std::sync::atomic::AtomicBool;
use std::sync::{Arc, mpsc};
use std::thread::{self};
use std::time::Duration;
use log::info;

use crate::wallpaper::{project, WallpaperType};
use crate::wallpaper::Wallpaper;

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
    frame_time: u32,
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
        let (_rx, _tx) = mpsc::channel::<FrameData>();
        //need two task: 1.decode video 2. render
        //
        let is_paused_arc = self.is_paused.clone();
        let is_stopped_arc = self.is_stopped.clone();
        
        let is_paused_arc2 = is_paused_arc.clone();
        let is_stopped_arc2 = is_stopped_arc.clone();
        
        let decode_thread = thread::spawn(move || {
            while !is_stopped_arc.load(std::sync::atomic::Ordering::SeqCst) {
                if is_paused_arc.load(std::sync::atomic::Ordering::SeqCst) {
                    thread::sleep(Duration::from_millis(100));
                }
                //TODO 
            }
        });

        self.decode_thread = Some(decode_thread);

        let render_thread = thread::spawn(move || {
            while !is_stopped_arc2.load(std::sync::atomic::Ordering::SeqCst) {
                if is_paused_arc2.load(std::sync::atomic::Ordering::SeqCst) {
                    thread::sleep(Duration::from_millis(100));
                }
            }
        });

        self.render_thread = Some(render_thread);

    }

    fn info(&self) {
        
    }

}

