use serde::{Deserialize, Serialize};

/// IPC 请求类型
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum IpcRequest {
    /// 设置壁纸
    SetWallpaper { path: String },
    /// 获取当前壁纸
    GetWallpaper,
    /// 获取状态
    GetStatus,
    /// 退出服务
    Shutdown,
}

/// IPC 响应类型
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum IpcResponse {
    /// 成功响应
    Success { message: String },
    /// 壁纸路径响应
    WallpaperPath { path: Option<String> },
    /// 状态响应
    Status { running: bool },
    /// 错误响应
    Error { message: String },
}

impl IpcResponse {
    pub fn success(message: impl Into<String>) -> Self {
        IpcResponse::Success {
            message: message.into(),
        }
    }

    pub fn wallpaper_path(path: Option<String>) -> Self {
        IpcResponse::WallpaperPath { path }
    }

    pub fn status(running: bool) -> Self {
        IpcResponse::Status { running }
    }

    pub fn error(message: impl Into<String>) -> Self {
        IpcResponse::Error {
            message: message.into(),
        }
    }
}