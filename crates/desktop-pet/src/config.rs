use serde::{Deserialize, Serialize};
use tracing::Level;

use crate::error::ConfigError;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    Error,
    Warn,
    Info,
    Debug,
    Trace,
}

impl LogLevel {
    pub fn as_tracing_level(self) -> Level {
        match self {
            Self::Error => Level::ERROR,
            Self::Warn => Level::WARN,
            Self::Info => Level::INFO,
            Self::Debug => Level::DEBUG,
            Self::Trace => Level::TRACE,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FpsConfig {
    pub active: u16,
    pub idle: u16,
    pub sleep: u16,
}

impl Default for FpsConfig {
    fn default() -> Self {
        Self {
            active: 60,
            idle: 30,
            sleep: 15,
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct AppConfig {
    pub scale: f64,
    pub always_on_top: bool,
    pub mouse_interaction: bool,
    pub fps: FpsConfig,
    pub log_level: LogLevel,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            scale: 1.0,
            always_on_top: true,
            mouse_interaction: true,
            fps: FpsConfig::default(),
            log_level: LogLevel::Info,
        }
    }
}

impl AppConfig {
    pub fn from_json(input: &str) -> Result<Self, ConfigError> {
        let config: Self = serde_json::from_str(input)?;
        config.validate()?;
        Ok(config)
    }

    pub fn validate(&self) -> Result<(), ConfigError> {
        if !self.scale.is_finite() || !(0.25..=4.0).contains(&self.scale) {
            return Err(ConfigError::InvalidScale(self.scale));
        }

        let FpsConfig {
            active,
            idle,
            sleep,
        } = self.fps;
        if sleep == 0 || sleep > idle || idle > active || active > 240 {
            return Err(ConfigError::InvalidFrameRates {
                active,
                idle,
                sleep,
            });
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_partial_json_with_defaults() {
        let config = AppConfig::from_json(r#"{"scale":1.5,"log_level":"debug"}"#)
            .expect("valid config should parse");

        assert_eq!(config.scale, 1.5);
        assert_eq!(config.log_level, LogLevel::Debug);
        assert_eq!(config.fps, FpsConfig::default());
    }

    #[test]
    fn rejects_unknown_fields() {
        let error = AppConfig::from_json(r#"{"unknown":true}"#)
            .expect_err("unknown fields must be rejected");

        assert!(matches!(error, ConfigError::Json(_)));
    }

    #[test]
    fn rejects_invalid_frame_rate_order() {
        let config = AppConfig {
            fps: FpsConfig {
                active: 30,
                idle: 60,
                sleep: 15,
            },
            ..AppConfig::default()
        };

        assert!(matches!(
            config.validate(),
            Err(ConfigError::InvalidFrameRates { .. })
        ));
    }
}
