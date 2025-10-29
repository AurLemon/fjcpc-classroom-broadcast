use anyhow::Result;
use tracing_subscriber::{fmt, EnvFilter};

/// Initialize a compact tracing subscriber that honors `RUST_LOG`.
pub fn init_tracing(app_name: &str) -> Result<()> {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    // Multiple initialisation attempts are benign; ignore the second one.
    let subscriber = fmt()
        .with_env_filter(filter)
        .with_thread_ids(true)
        .with_thread_names(true)
        .with_target(false)
        .compact()
        .finish();

    if tracing::subscriber::set_global_default(subscriber).is_err() {
        // Another subscriber is already set (likely in tests); treat as success.
        return Ok(());
    }

    tracing::info!(application = app_name, "logging initialized");
    Ok(())
}
