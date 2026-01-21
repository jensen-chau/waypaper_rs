use env_logger::Env;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // 初始化日志，输出到文件
    env_logger::Builder::from_env(Env::default().default_filter_or("info"))
        .format_timestamp_millis()
        .init();

    // 启动 server
    let socket_path = "/tmp/waypaper.sock";
    let socket_path = std::path::Path::new(socket_path);

    // 如果 socket 文件已存在，先删除
    if socket_path.exists() {
        std::fs::remove_file(socket_path)?;
    }

    let server = waypaper_rs::ipc::server::WayServer::new(socket_path)?;
    server.run().await

}
