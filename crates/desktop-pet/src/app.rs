use crate::{config::AppConfig, error::AppError};

/// Composition root for long-lived application state.
#[derive(Debug)]
pub struct Application {
    config: AppConfig,
}

impl Application {
    pub fn new(config: AppConfig) -> Result<Self, AppError> {
        config.validate()?;
        Ok(Self { config })
    }

    pub fn config(&self) -> &AppConfig {
        &self.config
    }

    /// Phase 0 intentionally exits after proving startup and shutdown plumbing.
    pub fn run(self) {
        tracing::debug!(scale = self.config.scale, "application baseline is ready");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn application_accepts_default_config() {
        let app = Application::new(AppConfig::default()).expect("default config must be valid");

        assert_eq!(app.config().fps.active, 60);
    }
}
