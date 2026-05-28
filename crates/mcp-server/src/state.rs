use std::sync::Arc;

use dashmap::DashMap;
use redis::AsyncCommands;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct SuppressionEntry {
    suppressed: bool,
    graph_version: i64,
    scopes_considered: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct SessionSuppressionState {
    inner: Arc<DashMap<String, SuppressionEntry>>,
    redis_client: Option<redis::Client>,
    ttl_secs: u64,
}

impl SessionSuppressionState {
    pub const DEFAULT_TTL_SECS: u64 = 3600;

    pub fn new(redis_client: Option<redis::Client>, ttl_secs: u64) -> Self {
        Self {
            inner: Arc::default(),
            redis_client,
            ttl_secs,
        }
    }

    fn redis_key(session_id: &str, repo_path: &str) -> String {
        format!(
            "suppression:{}::{}",
            session_id.trim(),
            repo_path.trim()
        )
    }

    fn local_key(session_id: &str, repo_path: &str) -> String {
        format!("{}::{}", session_id.trim(), repo_path.trim())
    }

    async fn try_redis_get(&self, session_id: &str, repo_path: &str) -> Option<SuppressionEntry> {
        let client = self.redis_client.as_ref()?;
        let mut conn = match client.get_multiplexed_async_connection().await {
            Ok(conn) => conn,
            Err(error) => {
                tracing::warn!(
                    ?error,
                    session_id = %session_id,
                    repo_path = %repo_path,
                    "failed to acquire redis connection for suppression lookup"
                );
                return None;
            }
        };
        let key = Self::redis_key(session_id, repo_path);
        let raw: Option<String> = match conn.get(&key).await {
            Ok(raw) => raw,
            Err(error) => {
                tracing::warn!(
                    ?error,
                    %key,
                    "redis GET failed for suppression key"
                );
                return None;
            }
        };
        match raw {
            Some(json) => match serde_json::from_str::<SuppressionEntry>(&json) {
                Ok(entry) => Some(entry),
                Err(error) => {
                    tracing::warn!(
                        ?error,
                        %key,
                        "failed to deserialize suppression entry from redis"
                    );
                    None
                }
            },
            None => None,
        }
    }

    async fn try_redis_setex(
        &self,
        session_id: &str,
        repo_path: &str,
        entry: &SuppressionEntry,
    ) {
        let Some(client) = &self.redis_client else {
            return;
        };
        let mut conn = match client.get_multiplexed_async_connection().await {
            Ok(conn) => conn,
            Err(error) => {
                tracing::warn!(
                    ?error,
                    session_id = %session_id,
                    repo_path = %repo_path,
                    "failed to acquire redis connection for suppression write"
                );
                return;
            }
        };
        let key = Self::redis_key(session_id, repo_path);
        let value = match serde_json::to_string(entry) {
            Ok(json) => json,
            Err(error) => {
                tracing::warn!(
                    ?error,
                    %key,
                    "failed to serialize suppression entry for redis"
                );
                return;
            }
        };
        if let Err(error) = conn.set_ex::<_, _, ()>(&key, &value, self.ttl_secs).await {
            tracing::warn!(
                ?error,
                %key,
                ttl_secs = self.ttl_secs,
                "redis SETEX failed for suppression key"
            );
        }
    }

    pub async fn is_suppressed(
        &self,
        session_id: &str,
        repo_path: &str,
        graph_version: i64,
    ) -> bool {
        if let Some(redis_entry) = self.try_redis_get(session_id, repo_path).await {
            return redis_entry.suppressed && redis_entry.graph_version == graph_version;
        }

        let key = Self::local_key(session_id, repo_path);
        self.inner
            .get(&key)
            .map(|entry| entry.suppressed && entry.graph_version == graph_version)
            .unwrap_or(false)
    }

    pub async fn graph_version(&self, session_id: &str, repo_path: &str) -> Option<i64> {
        if let Some(redis_entry) = self.try_redis_get(session_id, repo_path).await {
            return Some(redis_entry.graph_version);
        }

        let key = Self::local_key(session_id, repo_path);
        self.inner.get(&key).map(|entry| entry.graph_version)
    }

    pub async fn scopes_considered(
        &self,
        session_id: &str,
        repo_path: &str,
    ) -> Option<Vec<String>> {
        if let Some(redis_entry) = self.try_redis_get(session_id, repo_path).await {
            return Some(redis_entry.scopes_considered);
        }

        let key = Self::local_key(session_id, repo_path);
        self.inner
            .get(&key)
            .map(|entry| entry.scopes_considered.clone())
    }

    pub async fn mark_healthy(
        &self,
        session_id: &str,
        repo_path: &str,
        graph_version: i64,
        scopes_considered: &[String],
    ) {
        let entry = SuppressionEntry {
            suppressed: true,
            graph_version,
            scopes_considered: scopes_considered.to_vec(),
        };

        let local_key = Self::local_key(session_id, repo_path);
        self.inner.insert(local_key, entry.clone());

        self.try_redis_setex(session_id, repo_path, &entry).await;
    }
}

impl Default for SessionSuppressionState {
    fn default() -> Self {
        Self::new(None, Self::DEFAULT_TTL_SECS)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn suppression_state_round_trips_by_session_scope_key() {
        let state = SessionSuppressionState::default();
        assert!(!state.is_suppressed("session", "/repo", 7).await);
        assert_eq!(state.graph_version("session", "/repo").await, None);
        assert_eq!(state.scopes_considered("session", "/repo").await, None);

        state
            .mark_healthy("session", "/repo", 7, &["global".to_owned()])
            .await;
        assert!(state.is_suppressed("session", "/repo", 7).await);
        assert!(!state.is_suppressed("session", "/repo", 8).await);
        assert_eq!(state.graph_version("session", "/repo").await, Some(7));
        assert_eq!(
            state.scopes_considered("session", "/repo").await,
            Some(vec!["global".to_owned()])
        );
    }

    #[tokio::test]
    async fn default_ttl_is_one_hour() {
        let state = SessionSuppressionState::default();
        assert_eq!(state.ttl_secs, 3600);
    }
}