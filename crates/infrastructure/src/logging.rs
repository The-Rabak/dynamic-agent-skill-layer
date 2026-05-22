use thiserror::Error;
use tracing::info;
use tracing_subscriber::{EnvFilter, fmt};

#[derive(Debug, Error)]
pub enum LoggingError {
    #[error("logging bootstrap failed: {0}")]
    Bootstrap(String),
}

pub fn init_logging(service_name: &str, default_level: &str) -> Result<(), LoggingError> {
    if service_name.trim().is_empty() {
        return Err(LoggingError::Bootstrap(
            "service_name must not be blank".to_owned(),
        ));
    }

    let env_filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(default_level));

    fmt()
        .json()
        .with_env_filter(env_filter)
        .with_current_span(true)
        .with_span_list(true)
        .with_target(true)
        .with_ansi(false)
        .try_init()
        .map_err(|error| LoggingError::Bootstrap(error.to_string()))?;

    info!(service = service_name, "structured logging initialized");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn init_logging_rejects_blank_service_name() {
        let error = init_logging("   ", "info").expect_err("blank service name should fail");

        assert!(matches!(error, LoggingError::Bootstrap(_)));
    }
}
