mod audio;
mod screen;
mod server;

use std::path::PathBuf;

use anyhow::Result;
use clap::Parser;
use shared::prelude::*;

use crate::server::TeacherServer;

#[derive(Parser, Debug)]
#[command(author, version, about = "FJCPC Classroom Teacher Console")]
struct Cli {
    /// Path to teacher configuration (TOML)
    #[arg(short, long, default_value = "configs/teacher_config.toml")]
    config: PathBuf,

    /// Automatically start screen broadcast on launch
    #[arg(long)]
    auto_start_broadcast: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    init_tracing("teacher")?;

    let cli = Cli::parse();
    let config = TeacherConfig::load_from_path(&cli.config)?;
    let server = TeacherServer::new(config)?;
    server.run(cli.auto_start_broadcast).await?;

    Ok(())
}
