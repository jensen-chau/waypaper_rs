use anyhow::Result;
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::wallpaper::Wallpaper;

/// 壁纸播放器
/// 保存和管理实现了 Wallpaper trait 的对象
pub struct Player {
    wallpaper: Option<Box<dyn Wallpaper + Send>>,
    is_running: Arc<Mutex<bool>>,
}

impl Player {
    /// 创建新的播放器
    pub fn new() -> Self {
        Self {
            wallpaper: None,
            is_running: Arc::new(Mutex::new(false)),
        }
    }

    /// 设置壁纸
    pub fn set_wallpaper(&mut self, wallpaper: Box<dyn Wallpaper + Send>) {
        // 停止当前壁纸（如果存在）
        if let Some(mut w) = self.wallpaper.take() {
            w.pause();
        }

        self.wallpaper = Some(wallpaper);
        *self.is_running.blocking_lock() = true;
    }

    /// 播放壁纸
    pub fn play(&mut self) {
        if let Some(wallpaper) = &mut self.wallpaper {
            wallpaper.play();
            *self.is_running.blocking_lock() = true;
        }
    }

    /// 暂停壁纸
    pub fn pause(&mut self) {
        if let Some(wallpaper) = &mut self.wallpaper {
            wallpaper.pause();
            *self.is_running.blocking_lock() = false;
        }
    }

    /// 运行壁纸（启动播放循环）
    pub fn run(&mut self) {
        if let Some(wallpaper) = &mut self.wallpaper {
            wallpaper.run();
            *self.is_running.blocking_lock() = true;
        }
    }

    /// 停止壁纸
    pub fn stop(&mut self) {
        if let Some(wallpaper) = &mut self.wallpaper {
            wallpaper.pause();
            *self.is_running.blocking_lock() = false;
        }
    }

    /// 检查是否正在运行
    pub fn is_running(&self) -> bool {
        *self.is_running.blocking_lock()
    }

    /// 获取壁纸信息
    pub fn info(&self) {
        if let Some(wallpaper) = &self.wallpaper {
            wallpaper.info();
        }
    }

    /// 清除当前壁纸
    pub fn clear(&mut self) {
        self.stop();
        self.wallpaper = None;
    }
}

impl Default for Player {
    fn default() -> Self {
        Self::new()
    }
}