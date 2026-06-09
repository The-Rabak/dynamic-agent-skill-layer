use std::collections::HashMap;

use domain::ScopeRoot;

use crate::watcher::{SkillFileChange, SkillSnapshot, diff_skill_snapshots};

/// Reconciles missed watcher transitions against durable snapshots.
pub struct WatcherRecovery {
    last_seen_generation_by_key: HashMap<String, u64>,
    current_generation: u64,
    max_generation_window: u64,
}

impl Default for WatcherRecovery {
    fn default() -> Self {
        Self::with_generation_window(64)
    }
}

impl WatcherRecovery {
    /// Creates recovery state with a bounded deduplication generation window.
    ///
    /// Keys seen within the configured window are suppressed; older keys are evicted
    /// so the cache cannot grow without bound over long-running processes.
    pub fn with_generation_window(max_generation_window: usize) -> Self {
        assert!(
            max_generation_window > 0,
            "max generation window must be greater than zero"
        );
        Self {
            last_seen_generation_by_key: HashMap::new(),
            current_generation: 0,
            max_generation_window: max_generation_window as u64,
        }
    }

    /// Reconciles snapshot differences and suppresses duplicate idempotency keys.
    pub fn reconcile(
        &mut self,
        previous_snapshot: &SkillSnapshot,
        current_snapshot: &SkillSnapshot,
        scopes: &[ScopeRoot],
    ) -> Vec<SkillFileChange> {
        self.current_generation = self.current_generation.saturating_add(1);
        let active_generation = self.current_generation;
        let discovered = diff_skill_snapshots(scopes, previous_snapshot, current_snapshot);
        let mut reconciled = Vec::new();
        for change in discovered {
            let was_seen = self
                .last_seen_generation_by_key
                .insert(change.idempotency_key.clone(), active_generation)
                .is_some();
            if !was_seen {
                reconciled.push(change);
            }
        }
        self.evict_stale_generations();
        reconciled
    }

    fn evict_stale_generations(&mut self) {
        let oldest_generation_to_keep = self
            .current_generation
            .saturating_sub(self.max_generation_window.saturating_sub(1));
        self.last_seen_generation_by_key
            .retain(|_, generation| *generation >= oldest_generation_to_keep);
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, path::PathBuf};

    use domain::{ScopeRoot, ScopeType};

    use super::WatcherRecovery;
    use crate::watcher::{SkillFileFingerprint, SkillSnapshot};

    fn scope_root() -> ScopeRoot {
        ScopeRoot::new("project", ScopeType::Project, PathBuf::from("/project"))
    }

    fn snapshot_with_file(path: &str, hash: &str) -> SkillSnapshot {
        let mut snapshot = BTreeMap::new();
        snapshot.insert(
            PathBuf::from(path),
            SkillFileFingerprint {
                content_hash: hash.to_owned(),
                modified_nanos: 1,
            },
        );
        snapshot
    }

    #[test]
    fn reconcile_is_idempotent_for_repeated_snapshots_within_window() {
        let scope = scope_root();
        let scopes = vec![scope];
        let empty_snapshot = SkillSnapshot::new();
        let current_snapshot = snapshot_with_file("/project/skill-a.md", "hash-a");
        let mut recovery = WatcherRecovery::with_generation_window(2);

        let first = recovery.reconcile(&empty_snapshot, &current_snapshot, &scopes);
        let second = recovery.reconcile(&empty_snapshot, &current_snapshot, &scopes);

        assert_eq!(first.len(), 1);
        assert!(
            second.is_empty(),
            "repeated reconciliation should suppress duplicate idempotency keys"
        );
    }

    #[test]
    fn reconcile_evicts_stale_idempotency_keys_when_window_advances() {
        let scope = scope_root();
        let scopes = vec![scope];
        let empty_snapshot = SkillSnapshot::new();
        let mut recovery = WatcherRecovery::with_generation_window(2);
        let first_snapshot = snapshot_with_file("/project/skill-a.md", "hash-a");
        let second_snapshot = snapshot_with_file("/project/skill-b.md", "hash-b");
        let third_snapshot = snapshot_with_file("/project/skill-c.md", "hash-c");

        let first = recovery.reconcile(&empty_snapshot, &first_snapshot, &scopes);
        let second = recovery.reconcile(&empty_snapshot, &second_snapshot, &scopes);
        let third = recovery.reconcile(&empty_snapshot, &third_snapshot, &scopes);
        let replay_first = recovery.reconcile(&empty_snapshot, &first_snapshot, &scopes);

        assert_eq!(first.len(), 1);
        assert_eq!(second.len(), 1);
        assert_eq!(third.len(), 1);
        assert_eq!(
            replay_first.len(),
            1,
            "old keys should be evicted after falling outside the generation window"
        );
    }
}
