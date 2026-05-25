use std::path::PathBuf;

use domain::ScopeType;
use graph_builder::{
    GraphRebuildOrchestrator, InMemoryDurableGraphState, ScopeRoot, SkillWatcher, WatcherRecovery,
};
use infrastructure::EventEnvelope;

fn synthetic_outbox_drain_enabled() -> bool {
    matches!(
        std::env::var("GRAPH_BUILDER_ALLOW_SYNTHETIC_OUTBOX_DRAIN")
            .ok()
            .as_deref(),
        Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("YES")
    )
}

/// Runs one watcher + rebuild cycle for local validation.
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    if !synthetic_outbox_drain_enabled() {
        return Err("graph-builder runtime durable state has no relay-backed outbox drain wiring yet; refusing to run with synthetic drain disabled (set GRAPH_BUILDER_ALLOW_SYNTHETIC_OUTBOX_DRAIN=1 only for local test/demo runs)".into());
    }

    let repo_root = PathBuf::from(std::env::var("GRAPH_BUILDER_PROJECT_ROOT").unwrap_or_else(
        |_| {
            std::env::current_dir()
                .unwrap_or_default()
                .display()
                .to_string()
        },
    ));
    let scopes = vec![
        ScopeRoot::new("project", ScopeType::Project, repo_root),
        ScopeRoot::new(
            "global",
            ScopeType::Global,
            std::env::var("GRAPH_BUILDER_GLOBAL_ROOT")
                .map(PathBuf::from)
                .unwrap_or_else(|_| std::env::current_dir().unwrap_or_default().join("docs")),
        ),
    ];

    let mut watcher = SkillWatcher::new(scopes)?;
    let mut recovery = WatcherRecovery::default();
    let mut durable_state = InMemoryDurableGraphState::with_synthetic_outbox_drain();
    let mut published_events: Vec<EventEnvelope> = Vec::new();
    let mut orchestrator = GraphRebuildOrchestrator::new(&mut durable_state, &mut published_events);

    let first_scan = watcher.collect_file_changes()?;
    let recovered = recovery.reconcile(
        &watcher.previous_snapshot(),
        &watcher.current_snapshot(),
        &watcher.scopes(),
    );
    let mut all_changes = first_scan;
    all_changes.extend(recovered);

    if !all_changes.is_empty() {
        let outcome = orchestrator.rebuild_from_changes(&watcher.scopes(), &all_changes)?;
        println!(
            "graph rebuilt at version {} with {} skills across {} communities",
            outcome.graph_version, outcome.skills_count, outcome.communities_count
        );
    } else {
        println!("no skill file changes detected");
    }

    Ok(())
}
