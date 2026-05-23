use std::sync::Arc;

use dashmap::DashMap;

#[derive(Debug, Clone, PartialEq, Eq)]
struct SuppressionEntry {
    suppressed: bool,
    graph_version: i64,
    scopes_considered: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct SessionSuppressionState {
    inner: Arc<DashMap<String, SuppressionEntry>>,
}

impl SessionSuppressionState {
    fn key(session_id: &str, repo_path: &str) -> String {
        format!("{}::{}", session_id.trim(), repo_path.trim())
    }

    pub fn is_suppressed(&self, session_id: &str, repo_path: &str, graph_version: i64) -> bool {
        let key = Self::key(session_id, repo_path);
        self.inner
            .get(&key)
            .map(|entry| entry.suppressed && entry.graph_version == graph_version)
            .unwrap_or(false)
    }

    pub fn graph_version(&self, session_id: &str, repo_path: &str) -> Option<i64> {
        let key = Self::key(session_id, repo_path);
        self.inner.get(&key).map(|entry| entry.graph_version)
    }

    pub fn scopes_considered(&self, session_id: &str, repo_path: &str) -> Option<Vec<String>> {
        let key = Self::key(session_id, repo_path);
        self.inner
            .get(&key)
            .map(|entry| entry.scopes_considered.clone())
    }

    pub fn mark_healthy(
        &self,
        session_id: &str,
        repo_path: &str,
        graph_version: i64,
        scopes_considered: &[String],
    ) {
        let key = Self::key(session_id, repo_path);
        self.inner.insert(
            key,
            SuppressionEntry {
                suppressed: true,
                graph_version,
                scopes_considered: scopes_considered.to_vec(),
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn suppression_state_round_trips_by_session_scope_key() {
        let state = SessionSuppressionState::default();
        assert!(!state.is_suppressed("session", "/repo", 7));
        assert_eq!(state.graph_version("session", "/repo"), None);
        assert_eq!(state.scopes_considered("session", "/repo"), None);

        state.mark_healthy("session", "/repo", 7, &["global".to_owned()]);
        assert!(state.is_suppressed("session", "/repo", 7));
        assert!(!state.is_suppressed("session", "/repo", 8));
        assert_eq!(state.graph_version("session", "/repo"), Some(7));
        assert_eq!(
            state.scopes_considered("session", "/repo"),
            Some(vec!["global".to_owned()])
        );
    }
}
