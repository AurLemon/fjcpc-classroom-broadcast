mod audio;
mod screen;
mod server;
#[cfg(feature = "ui")]
mod ui;

use std::path::PathBuf;
use std::sync::Arc;

#[cfg(feature = "ui")]
use anyhow::Context;
use anyhow::Result;
use clap::Parser;
use shared::prelude::*;
#[cfg(feature = "ui")]
use tokio::sync::mpsc;
#[cfg(feature = "ui")]
use tracing::error;
#[cfg(not(feature = "ui"))]
use tracing::warn;

#[cfg(feature = "ui")]
use crate::server::ServerCommand;
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

    /// Run without launching the Windows control panel UI
    #[arg(long)]
    headless: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    init_tracing("teacher")?;

    let cli = Cli::parse();
    let config = TeacherConfig::load_from_path(&cli.config)?;
    let server = Arc::new(TeacherServer::new(config)?);

    #[cfg(feature = "ui")]
    {
        if !cli.headless {
            let (command_tx, command_rx) = mpsc::unbounded_channel();
            let server_task = {
                let server_clone = Arc::clone(&server);
                tokio::spawn(async move {
                    server_clone
                        .run(cli.auto_start_broadcast, Some(command_rx))
                        .await
                })
            };

            let ui_context = ui::UiContext::new(command_tx.clone(), cli.config.clone());

            let ui_result = tokio::task::spawn_blocking(move || ui::run(ui_context))
                .await
                .context("UI thread panicked")?;

            if let Err(err) = ui_result {
                error!(?err, "控制面板出现错误");
            }

            let _ = command_tx.send(ServerCommand::Quit);

            match server_task.await {
                Ok(result) => {
                    if let Err(err) = result {
                        return Err(err);
                    }
                }
                Err(join_err) => return Err(join_err.into()),
            }

            return Ok(());
        }
    }

    #[cfg(not(feature = "ui"))]
    {
        if !cli.headless {
            warn!("UI feature disabled (enable `ui` feature to open the control panel); falling back to CLI mode.");
        }
    }

    server.run(cli.auto_start_broadcast, None).await
}
