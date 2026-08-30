use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("application configuration is invalid")]
    Config(#[from] ConfigError),
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("configuration JSON is invalid: {0}")]
    Json(#[from] serde_json::Error),

    #[error("scale must be finite and between 0.25 and 4.0, got {0}")]
    InvalidScale(f64),

    #[error(
        "frame rates must satisfy 1 <= sleep <= idle <= active <= 240; got active={active}, idle={idle}, sleep={sleep}"
    )]
    InvalidFrameRates { active: u16, idle: u16, sleep: u16 },
}
