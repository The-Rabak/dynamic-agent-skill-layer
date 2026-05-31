use std::sync::Arc;

use admin::tools::{
    FilesystemGraphRebuildTrigger, GraphRebuildTrigger, GraphSnapshotReader,
    PostgresGraphSnapshotReader,
};
use domain::{ScopeRoot, ScopeType};

/// Runtime dependencies that wire admin tools into the MCP transport surface.
///
/// This module keeps transport entry points in `lib.rs` declarative and confines
/// admin/graph assembly details to a dedicated boundary module.
pub(crate) struct AdminRuntimeDependencies {
    pub(crate) rebuild_trigger: Arc<dyn GraphRebuildTrigger>,
    pub(crate) graph_reader: Arc<dyn GraphSnapshotReader>,
}

pub(crate) fn live_admin_runtime_dependencies() -> AdminRuntimeDependencies {
    let graph_reader = Arc::new(PostgresGraphSnapshotReader::with_default_database_env())
        as Arc<dyn GraphSnapshotReader>;
    let rebuild_trigger = Arc::new(FilesystemGraphRebuildTrigger::new(default_scope_roots()))
        as Arc<dyn GraphRebuildTrigger>;

    AdminRuntimeDependencies {
        rebuild_trigger,
        graph_reader,
    }
}

fn default_scope_roots() -> Vec<ScopeRoot> {
    let project_root = std::env::var("GRAPH_BUILDER_PROJECT_ROOT")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::env::current_dir().unwrap_or_else(|_| ".".into()));
    let global_root = std::env::var("GRAPH_BUILDER_GLOBAL_ROOT")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| project_root.join("docs"));
    vec![
        ScopeRoot::new("project", ScopeType::Project, project_root),
        ScopeRoot::new("global", ScopeType::Global, global_root),
    ]
}
