use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::errors::{ConfigError, DomainError};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EmbeddingConfig {
    pub provider: String,
    pub model: String,
    pub dimension: usize,
    pub timeout_ms: u64,
    pub batch_timeout_ms: u64,
    pub max_concurrency: u16,
}

impl Default for EmbeddingConfig {
    fn default() -> Self {
        Self {
            provider: "ollama".to_owned(),
            model: "qwen3-embedding:4b".to_owned(),
            dimension: 2560,
            timeout_ms: 500,
            batch_timeout_ms: 5_000,
            max_concurrency: 4,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExtractionConfig {
    pub provider: String,
    pub model: String,
    pub timeout_ms: u64,
}

impl Default for ExtractionConfig {
    fn default() -> Self {
        Self {
            provider: "claude".to_owned(),
            model: "sonnet".to_owned(),
            timeout_ms: 1_500,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScopeConfig {
    pub project_scope_enabled: bool,
    pub global_scope_enabled: bool,
    pub team_scope_enabled: bool,
    pub global_paths: Vec<PathBuf>,
}

impl Default for ScopeConfig {
    fn default() -> Self {
        Self {
            project_scope_enabled: true,
            global_scope_enabled: true,
            team_scope_enabled: false,
            global_paths: vec![],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompilationConfig {
    pub max_skills: usize,
    pub max_context_chars: usize,
    pub include_rescue_context: bool,
}

impl Default for CompilationConfig {
    fn default() -> Self {
        Self {
            max_skills: 12,
            max_context_chars: 12_000,
            include_rescue_context: true,
        }
    }
}

/// Hyper-parameters for HDBSCAN community detection run during graph rebuild.
///
/// HDBSCAN is executed per scope against the in-memory embeddings produced by
/// the configured embedding arm (default qwen3-embedding:4b, 2560-dim).
/// These parameters control cluster granularity and the noise floor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HdbscanConfig {
    /// Minimum number of skills required to form a semantic cluster.
    /// Skills that never join a cluster large enough are labelled noise (-1)
    /// and are represented as individual "unclustered" communities so they
    /// remain retrievable.
    pub min_cluster_size: usize,
}

impl Default for HdbscanConfig {
    fn default() -> Self {
        Self {
            min_cluster_size: 3,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct DomainConfig {
    pub embedding: EmbeddingConfig,
    pub extraction: ExtractionConfig,
    pub scope: ScopeConfig,
    pub compilation: CompilationConfig,
    pub hdbscan: HdbscanConfig,
}

impl DomainConfig {
    pub fn validate(&self) -> Result<(), DomainError> {
        if self.embedding.dimension == 0 {
            return Err(invalid_value(
                "embedding.dimension",
                "must be greater than zero",
            ));
        }

        if self.embedding.timeout_ms == 0 {
            return Err(invalid_value(
                "embedding.timeout_ms",
                "must be greater than zero",
            ));
        }

        if self.embedding.batch_timeout_ms == 0 {
            return Err(invalid_value(
                "embedding.batch_timeout_ms",
                "must be greater than zero",
            ));
        }

        if self.embedding.max_concurrency == 0 {
            return Err(invalid_value(
                "embedding.max_concurrency",
                "must be greater than zero",
            ));
        }

        if self.extraction.provider.trim().is_empty() {
            return Err(invalid_value("extraction.provider", "must not be blank"));
        }

        if self.extraction.model.trim().is_empty() {
            return Err(invalid_value("extraction.model", "must not be blank"));
        }

        if self.extraction.timeout_ms == 0 {
            return Err(invalid_value(
                "extraction.timeout_ms",
                "must be greater than zero",
            ));
        }

        if !self.scope.project_scope_enabled
            && !self.scope.global_scope_enabled
            && !self.scope.team_scope_enabled
        {
            return Err(invalid_value("scope", "at least one scope must be enabled"));
        }

        if self.compilation.max_skills == 0 {
            return Err(invalid_value(
                "compilation.max_skills",
                "must be greater than zero",
            ));
        }

        if self.compilation.max_context_chars == 0 {
            return Err(invalid_value(
                "compilation.max_context_chars",
                "must be greater than zero",
            ));
        }

        Ok(())
    }
}

fn invalid_value(field: &'static str, message: impl Into<String>) -> DomainError {
    ConfigError::InvalidValue {
        field,
        message: message.into(),
    }
    .into()
}
