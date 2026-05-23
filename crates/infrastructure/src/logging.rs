use thiserror::Error;
use tracing::info;
use tracing_subscriber::{EnvFilter, fmt, layer::SubscriberExt, util::SubscriberInitExt};

#[derive(Debug, Error)]
pub enum LoggingError {
    #[error("logging bootstrap failed: {0}")]
    Bootstrap(String),
}

#[derive(Debug, Clone)]
pub struct ServiceLoggingConfig {
    pub service_name: String,
    pub service_version: String,
    pub environment: String,
    pub default_level: String,
}

impl ServiceLoggingConfig {
    pub fn new(
        service_name: impl Into<String>,
        service_version: impl Into<String>,
        environment: impl Into<String>,
        default_level: impl Into<String>,
    ) -> Self {
        Self {
            service_name: service_name.into(),
            service_version: service_version.into(),
            environment: environment.into(),
            default_level: default_level.into(),
        }
    }
}

pub fn init_service_logging(config: ServiceLoggingConfig) -> Result<(), LoggingError> {
    if config.service_name.trim().is_empty() {
        return Err(LoggingError::Bootstrap(
            "service_name must not be blank".to_owned(),
        ));
    }

    if config.default_level.trim().is_empty() {
        return Err(LoggingError::Bootstrap(
            "default_level must not be blank".to_owned(),
        ));
    }

    let env_filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(&config.default_level));

    tracing_subscriber::registry()
        .with(env_filter)
        .with(
            fmt::layer()
                .json()
                .with_current_span(true)
                .with_span_list(true)
                .with_target(true)
                .with_ansi(false),
        )
        .try_init()
        .map_err(|error| LoggingError::Bootstrap(error.to_string()))?;

    info!(
        service = %config.service_name,
        version = %config.service_version,
        environment = %config.environment,
        log_format = "json",
        tracing_ready = true,
        "structured logging initialized"
    );
    Ok(())
}

pub fn init_logging(service_name: &str, default_level: &str) -> Result<(), LoggingError> {
    let environment = std::env::var("APP_ENV")
        .or_else(|_| std::env::var("ENVIRONMENT"))
        .unwrap_or_else(|_| "local".to_owned());

    init_service_logging(ServiceLoggingConfig::new(
        service_name,
        "unknown",
        environment,
        default_level,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn init_logging_rejects_blank_service_name() {
        let error = init_logging("   ", "info").expect_err("blank service name should fail");

        assert!(matches!(error, LoggingError::Bootstrap(_)));
    }

    #[test]
    fn init_service_logging_rejects_blank_default_level() {
        let error = init_service_logging(ServiceLoggingConfig::new(
            "mcp-server",
            "0.1.0",
            "test",
            "   ",
        ))
        .expect_err("blank default level should fail");

        assert!(matches!(error, LoggingError::Bootstrap(_)));
    }
}
