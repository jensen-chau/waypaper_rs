use crate::wallpaper::project::Project;
use anyhow::Result;

pub mod web;
pub mod video;
pub mod video_hw;
pub mod project;
pub mod player;

#[derive(Debug, thiserror::Error)]
pub enum WallpaperError {
    #[error("unknown wallpaper type: {0}")]
    UnknownWallpaperType(String),
}

pub trait Wallpaper: Send + Sync {
    fn play(&mut self);
    fn pause(&mut self);
    fn run(&mut self);
    fn info(&self);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WallpaperType {
    Video,
    Web,
    Scene,
}

pub fn get_wallpaper_type(project: &Project) -> Result<WallpaperType, WallpaperError> {
    match project.wallpaper_type.to_lowercase().as_str() {
        "web" => Ok(WallpaperType::Web),
        "video" => Ok(WallpaperType::Video),
        "scene" => Ok(WallpaperType::Scene),
        _ => Err(WallpaperError::UnknownWallpaperType(project.wallpaper_type.clone())),
    }
}