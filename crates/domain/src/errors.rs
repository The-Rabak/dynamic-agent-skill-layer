use thiserror::Error;

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum DomainError {
    #[error("invalid identifier: {0}")]
    InvalidIdentifier(String),
    #[error("invalid state: {0}")]
    InvalidState(String),
    #[error(transparent)]
    Config(#[from] ConfigError),
    #[error(transparent)]
    Embedding(#[from] EmbeddingError),
    #[error(transparent)]
    Extraction(#[from] ExtractionError),
    #[error(transparent)]
    Scope(#[from] ScopeError),
    #[error(transparent)]
    Compilation(#[from] CompilationError),
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum ConfigError {
    #[error("missing configuration: {field}")]
    MissingValue { field: &'static str },
    #[error("invalid configuration for {field}: {message}")]
    InvalidValue {
        field: &'static str,
        message: String,
    },
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum EmbeddingError {
    #[error("embedding provider unavailable: {0}")]
    ProviderUnavailable(String),
    #[error("embedding input is invalid: {0}")]
    InvalidInput(String),
    #[error("embedding request timed out after {timeout_ms}ms")]
    Timeout { timeout_ms: u64 },
    #[error("embedding failure: {0}")]
    Unexpected(String),
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum ExtractionError {
    #[error("extraction provider unavailable: {0}")]
    ProviderUnavailable(String),
    #[error("transcript payload is invalid: {0}")]
    InvalidTranscript(String),
    #[error("extraction request timed out after {timeout_ms}ms")]
    Timeout { timeout_ms: u64 },
    #[error("extraction failure: {0}")]
    Unexpected(String),
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum ScopeError {
    #[error("scope resolution unavailable: {0}")]
    ResolverUnavailable(String),
    #[error("invalid scope configuration: {0}")]
    InvalidConfiguration(String),
    #[error("scope resolution failure: {0}")]
    Unexpected(String),
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum CompilationError {
    #[error("context compilation input is invalid: {0}")]
    InvalidInput(String),
    #[error("context compilation failure: {0}")]
    Unexpected(String),
}
