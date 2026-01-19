use anyhow::{Context, Result};
use log::{info, error};
use std::path::Path;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::Mutex;

use crate::ipc::protocol::{IpcRequest, IpcResponse};

/// 服务器状态
#[derive(Debug, Default)]
struct ServerState {
    current_wallpaper: Option<String>,
    running: bool,
}

pub struct WayServer {
    listener: UnixListener,
    state: Arc<Mutex<ServerState>>,
}

impl WayServer {
    pub fn new<P: AsRef<Path>>(path: P) -> Result<Self> {
        let listener = UnixListener::bind(path)
            .context("Failed to bind Unix socket")?;

        let state = Arc::new(Mutex::new(ServerState {
            current_wallpaper: None,
            running: true,
        }));

        Ok(WayServer { listener, state })
    }

    pub async fn run(&self) -> Result<()> {
        info!("Waypaper daemon started, listening on socket");

        loop {
            match self.listener.accept().await {
                Ok((stream, _addr)) => {
                    let state = self.state.clone();
                    tokio::spawn(async move {
                        if let Err(e) = handle_client(stream, state).await {
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
    state: Arc<Mutex<ServerState>>,
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

    let response = handle_request(request, &state).await;

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
    state: &Arc<Mutex<ServerState>>,
) -> IpcResponse {
    match request {
        IpcRequest::SetWallpaper { path } => {
            // TODO: 实际设置壁纸的逻辑
            let mut state = state.lock().await;
            state.current_wallpaper = Some(path.clone());
            IpcResponse::success(format!("Wallpaper set: {}", path))
        }
        IpcRequest::GetWallpaper => {
            let state = state.lock().await;
            IpcResponse::wallpaper_path(state.current_wallpaper.clone())
        }
        IpcRequest::GetStatus => {
            let state = state.lock().await;
            IpcResponse::status(state.running)
        }
        IpcRequest::Shutdown => {
            let mut state = state.lock().await;
            state.running = false;
            IpcResponse::success("Server is closing".to_string())
        }
    }
}
