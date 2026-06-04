use std::sync::Arc;

use admin::tools::{
    FilesystemGraphRebuildTrigger, GraphRebuildTrigger, GraphSnapshotReader,
    PostgresGraphSnapshotReader,
};
use domain::{EmbeddingService, ScopeRoot, ScopeType};
use infrastructure::{OllamaEmbeddingConfig, OllamaEmbeddingService};

/// Runtime dependencies that wire admin tools into the MCP transport surface.
///
/// This module keeps transport entry points in `lib.rs` declarative and confines
/// admin/graph assembly details to a dedicated boundary module.
pub(crate) struct AdminRuntimeDependencies {
    pub(crate) rebuild_trigger: Arc<dyn GraphRebuildTrigger>,
    pub(crate) graph_reader: Arc<dyn GraphSnapshotReader>,
}

pub(crate) fn live_admin_runtime_dependencies() -> AdminRuntimeDependencies {
    // Production boot: build the real Ollama embedder up front and FAIL LOUD if
    // `OLLAMA_URL` is unset (no fallback embedder in production).
    admin_runtime_dependencies_with_embedder(build_embedding_service())
}

/// Builds admin runtime deps with a caller-provided embedder.
///
/// Used by the explicit-graph / test constructors so that building the router does
/// NOT require a live `OLLAMA_URL`: the embedder is only exercised when an admin
/// rebuild is actually triggered, so it is correct to defer the live requirement to
/// production boot (`live_admin_runtime_dependencies`) and let tests pass their own.
pub(crate) fn admin_runtime_dependencies_with_embedder(
    embedding_service: Arc<dyn EmbeddingService>,
) -> AdminRuntimeDependencies {
    let graph_reader = Arc::new(PostgresGraphSnapshotReader::with_default_database_env())
        as Arc<dyn GraphSnapshotReader>;

    let rebuild_trigger = Arc::new(FilesystemGraphRebuildTrigger::new(
        default_scope_roots(),
        embedding_service,
    )) as Arc<dyn GraphRebuildTrigger>;

    AdminRuntimeDependencies {
        rebuild_trigger,
        graph_reader,
    }
}

/// Builds a real Ollama embedding service from the `OLLAMA_URL` environment variable.
///
/// Panics at boot when `OLLAMA_URL` is unset — production must not continue without
/// a real embedder. There is no silent fallback.
fn build_embedding_service() -> Arc<dyn EmbeddingService> {
    let base_url = std::env::var("OLLAMA_URL")
        .expect("OLLAMA_URL must be set to connect to the embedding service");
    let config = OllamaEmbeddingConfig {
        base_url,
        model: "nomic-embed-text".to_owned(),
        timeout_ms: 5_000,
        batch_timeout_ms: 10_000,
        max_concurrency: 4,
    };
    let service = OllamaEmbeddingService::from_config(config)
        .expect("OllamaEmbeddingService construction must succeed with a valid config");
    Arc::new(service) as Arc<dyn EmbeddingService>
}

/// Build the default project and global scope roots used by the filesystem
/// graph-rebuild trigger.
///
/// Both roots are canonicalized before `ScopeRoot` construction so that a
/// relative fallback such as `"."` (produced when `GRAPH_BUILDER_PROJECT_ROOT`
/// is unset *and* `current_dir()` fails) cannot silently break the
/// `starts_with` scope gate.  Without canonicalization, boot-time
/// `std::fs::canonicalize` on a skill path yields an absolute path that can
/// never match a relative `"."` prefix, silently dropping post-migration
/// skills that have correct `source_paths` provenance.
///
/// Canonicalization is best-effort: if the resolved path does not exist on
/// disk (e.g. a fresh repo with no `docs/` yet), the non-canonical value is
/// kept so boot does not fail hard.
fn default_scope_roots() -> Vec<ScopeRoot> {
    let project_root_raw = std::env::var("GRAPH_BUILDER_PROJECT_ROOT")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::env::current_dir().unwrap_or_else(|_| ".".into()));
    let global_root_raw = std::env::var("GRAPH_BUILDER_GLOBAL_ROOT")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| project_root_raw.join("docs"));

    // Canonicalize once so all downstream `starts_with` checks use absolute
    // paths regardless of CWD at the time the scope gate runs.
    let project_root = std::fs::canonicalize(&project_root_raw).unwrap_or(project_root_raw);
    let global_root = std::fs::canonicalize(&global_root_raw).unwrap_or(global_root_raw);

    vec![
        ScopeRoot::new("project", ScopeType::Project, project_root),
        ScopeRoot::new("global", ScopeType::Global, global_root),
    ]
}
