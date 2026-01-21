use anyhow::{Context, Result};
use log::{info, error};
use std::path::Path;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::Mutex;

use crate::ipc::protocol::{IpcRequest, IpcResponse};
use crate::wallpaper::player::Player;
use crate::wallpaper::{Wallpaper, WallpaperType};
use crate::wallpaper::video_hw::VideoWallpaper;
use crate::wallpaper::project::build_project;

pub struct WayServer {
    listener: UnixListener,
    player: Arc<Mutex<Player>>,
}

impl WayServer {
    pub fn new<P: AsRef<Path>>(path: P) -> Result<Self> {
        let listener = UnixListener::bind(path)
            .context("Failed to bind Unix socket")?;

        let player = Arc::new(Mutex::new(Player::new()));

        Ok(WayServer { listener, player })
    }

    pub async fn run(&self) -> Result<()> {
        info!("Waypaper daemon started, listening on socket");

        loop {
            match self.listener.accept().await {
                Ok((stream, _addr)) => {
                    let player = self.player.clone();
                    tokio::spawn(async move {
                        if let Err(e) = handle_client(stream, player).await {
                            error!("Error handling client: {}", e);
                        }
                    });
                }
                Err(e) => {
                    error!("Failed to accept connection: {}", e);
                }
            }
        }
    }
}

async fn handle_client(
    mut stream: UnixStream,
    player: Arc<Mutex<Player>>,
) -> Result<()> {
    let request_len = stream
        .read_u32()
        .await
        .context("Failed to read request length")? as usize;

    let mut request_bytes = vec![0u8; request_len];
    stream
        .read_exact(&mut request_bytes)
        .await
        .context("Failed to read request data")?;

    let request: IpcRequest = serde_json::from_slice(&request_bytes)
        .context("Failed to deserialize request")?;

    info!("Receive command {:#?}", request);

    let response = handle_request(request, &player).await;

    let response_json = serde_json::to_string(&response)
        .context("Failed to serialize response")?;

    let response_bytes = response_json.as_bytes();
    let len = response_bytes.len() as u32;

    stream
        .write_u32(len)
        .await
        .context("Failed to write response length")?;
    stream
        .write_all(response_bytes)
        .await
        .context("Failed to write response data")?;

    Ok(())
}

async fn handle_request(
    request: IpcRequest,
    player: &Arc<Mutex<Player>>,
) -> IpcResponse {
    match request {
        IpcRequest::SetWallpaper { path } => {
            // 检查文件是否存在
            if !std::path::Path::new(&path).exists() {
                return IpcResponse::error(format!("File not found: {}", path));
            }

            // 读取 project.json
            let project_dir = std::path::Path::new(&path).parent()
                .unwrap_or_else(|| std::path::Path::new(""));
            let project_json_path = project_dir.join("project.json");
            
            let project_json_path_str = project_json_path.to_str()
                .unwrap_or_else(|| {
                    error!("Failed to convert project.json path to string");
                    return "";
                });

            let project = match build_project(project_json_path_str) {
                Ok(p) => p,
                Err(e) => {
                    error!("Failed to load project.json: {}", e);
                    return IpcResponse::error(format!("Failed to load project.json: {}", e));
                }
            };

            // 根据 project.json 创建相应的壁纸实例
            let wallpaper: Box<dyn Wallpaper + Send> = match project.wallpaper_type.to_lowercase().as_str() {
                "video" => {
                    let mut video_wallpaper = VideoWallpaper::new(path.clone(), WallpaperType::Video);
                    // 设置性能优化参数
                    video_wallpaper.set_target_fps(30);
                    video_wallpaper.set_max_resolution(1280, 720);
                    Box::new(video_wallpaper)
                }
                _ => {
                    return IpcResponse::error(format!("Unsupported wallpaper type: {}", project.wallpaper_type));
                }
            };

            // 设置到 player
            {
                let mut player = player.lock().await;
                player.set_wallpaper(wallpaper);
                player.run();
            }

            info!("Wallpaper set: {} (type: {})", path, project.wallpaper_type);
            IpcResponse::success(format!("Wallpaper set: {} ({})", path, project.wallpaper_type))
        }
        IpcRequest::GetWallpaper => {
            let player = player.lock().await;
            let is_running = player.is_running();
            IpcResponse::success(format!("Player status: {}", if is_running { "Running" } else { "Stopped" }))
        }
        IpcRequest::GetStatus => {
            let player = player.lock().await;
            let is_running = player.is_running();
            IpcResponse::status(is_running)
        }
        IpcRequest::Shutdown => {
            // 停止壁纸
            {
                let mut player = player.lock().await;
                player.stop();
                player.clear();
            }
            IpcResponse::success("Server is closing".to_string())
        }
    }
}