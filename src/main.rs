mod cli;
mod ipc;
mod wayland;
mod wallpaper;

use anyhow::Result;
use clap::Parser;
use cli::Args;
use env_logger::Env;

fn main() -> Result<()> {
    env_logger::Builder::from_env(Env::default().default_filter_or("info")).init();

    let args = Args::parse();

    cli::execute_command(args)?;

    Ok(())
}
