mod audio;
mod client;
mod files;
mod screen;
mod video;

use std::path::PathBuf;

use anyhow::Result;
use clap::Parser;
use shared::prelude::*;

use crate::client::StudentApp;

#[derive(Parser, Debug)]
#[command(author, version, about = "FJCPC Classroom Student Client")]
struct Cli {
    /// Path to student configuration JSON file
    #[arg(short, long, default_value = "configs/student_config.json")]
    config: PathBuf,
}

#[tokio::main]
async fn main() -> Result<()> {
    init_tracing("student")?;

    let cli = Cli::parse();
    let config = StudentConfig::load_from_path(&cli.config)?;
    StudentApp::new(config).run().await
}
