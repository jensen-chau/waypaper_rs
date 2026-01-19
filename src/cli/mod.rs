use anyhow::Result;
use clap::{Parser, Subcommand};

use crate::ipc::client::IpcClient;
use crate::ipc::protocol::IpcResponse;

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
pub struct Args {
    #[arg(short, long, default_value = "/tmp/waypaper.sock")]
    pub socket: String,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    Set { path: String },
    Get,
    Status,
    Shutdown,
}

pub fn execute_command(args: Args) -> Result<()> {
    let mut client = IpcClient::connect(&args.socket)?;

    match args.command {
        Command::Set { path } => {
            let response = client.set_wallpaper(path)?;
            handle_response(response)?;
        }
        Command::Get => {
            let response = client.get_wallpaper()?;
            handle_response(response)?;
        }
        Command::Status => {
            let response = client.get_status()?;
            handle_response(response)?;
        }
        Command::Shutdown => {
            let response = client.shutdown()?;
            handle_response(response)?;
        }
    }

    Ok(())
}

fn handle_response(response: IpcResponse) -> Result<()> {
    match response {
        crate::ipc::protocol::IpcResponse::Success { message } => {
            println!("{}", message);
        }
        crate::ipc::protocol::IpcResponse::WallpaperPath { path } => {
            match path {
                Some(p) => println!("Current wallpaper: {}", p),
                None => println!("No wallpaper set"),
            }
        }
        crate::ipc::protocol::IpcResponse::Status { running } => {
            println!("Daemon status: {}", if running { "Running" } else { "Stopped" });
        }
        crate::ipc::protocol::IpcResponse::Error { message } => {
            eprintln!("Error: {}", message);
            return Err(anyhow::anyhow!("{}", message));
        }
    }
    Ok(())
}
