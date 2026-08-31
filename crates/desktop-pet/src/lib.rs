mod animation;
pub mod app;
pub mod asset;
pub mod config;
mod display;
pub mod error;
mod input;
mod interaction;
mod pet;
mod platform;
pub mod render;
pub mod time;

use anyhow::{Context, Result};
use app::Application;
use config::AppConfig;

/// Initializes diagnostics, runs the application, and reports boundary errors.
pub fn run(config: AppConfig) -> Result<()> {
    tracing_subscriber::fmt()
        .with_max_level(config.log_level.as_tracing_level())
        .try_init()
        .map_err(|error| anyhow::anyhow!("failed to initialize structured logging: {error}"))?;

    let mut app = Application::new(config).context("failed to construct DesktopPet")?;

    tracing::info!(
        version = env!("CARGO_PKG_VERSION"),
        platform = std::env::consts::OS,
        "DesktopPet starting"
    );
    app.run().context("DesktopPet event loop failed")?;
    tracing::info!("DesktopPet stopped cleanly");

    Ok(())
}
