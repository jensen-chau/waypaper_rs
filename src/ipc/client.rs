use anyhow::{Context, Result};
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;

use crate::ipc::protocol::{IpcRequest, IpcResponse};

pub struct IpcClient {
    stream: UnixStream,
}

impl IpcClient {
    /// 连接到 Unix socket
    pub fn connect<P: AsRef<std::path::Path>>(socket_path: P) -> Result<Self> {
        let stream = UnixStream::connect(socket_path)
            .context("Failed to connect to IPC server")?;

        Ok(IpcClient { stream })
    }

    /// 发送请求并等待响应
    pub fn send_request(&mut self, request: IpcRequest) -> Result<IpcResponse> {
        // 序列化请求
        let request_json =
            serde_json::to_string(&request).context("Failed to serialize request")?;

        // 发送请求长度和数据
        let request_bytes = request_json.as_bytes();
        let len = request_bytes.len() as u32;

        self.stream
            .write_all(&len.to_be_bytes())
            .context("Failed to write request length")?;
        self.stream
            .write_all(request_bytes)
            .context("Failed to write request data")?;

        // 读取响应长度
        let mut len_bytes = [0u8; 4];
        self.stream
            .read_exact(&mut len_bytes)
            .context("Failed to read response length")?;
        let response_len = u32::from_be_bytes(len_bytes) as usize;

        // 读取响应数据
        let mut response_bytes = vec![0u8; response_len];
        self.stream
            .read_exact(&mut response_bytes)
            .context("Failed to read response data")?;

        // 反序列化响应
        let response: IpcResponse = serde_json::from_slice(&response_bytes)
            .context("Failed to deserialize response")?;

        Ok(response)
    }

    /// 设置壁纸
    pub fn set_wallpaper(&mut self, path: String) -> Result<IpcResponse> {
        let request = IpcRequest::SetWallpaper { path };
        self.send_request(request)
    }

    /// 获取当前壁纸
    pub fn get_wallpaper(&mut self) -> Result<IpcResponse> {
        let request = IpcRequest::GetWallpaper;
        self.send_request(request)
    }

    /// 获取状态
    pub fn get_status(&mut self) -> Result<IpcResponse> {
        let request = IpcRequest::GetStatus;
        self.send_request(request)
    }

    /// 关闭服务器
    pub fn shutdown(&mut self) -> Result<IpcResponse> {
        let request = IpcRequest::Shutdown;
        self.send_request(request)
    }
}