use std::sync::Arc;

use dashmap::DashMap;
use infrastructure::{AsyncCommands, RedisClient, redis_cmd};
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
    redis_client: Option<RedisClient>,
    ttl_secs: u64,
}

pub(crate) async fn scan_and_del_pattern(client: &RedisClient, pattern: &str, context: &str) {
    let mut conn = match client.get_multiplexed_async_connection().await {
        Ok(conn) => conn,
        Err(error) => {
            tracing::warn!(
                ?error,
                %pattern,
                %context,
                "failed to acquire redis connection for pattern delete"
            );
            return;
        }
    };

    let mut cursor: u64 = 0;
    let mut keys: Vec<String> = Vec::new();
    loop {
        let (next_cursor, mut batch): (u64, Vec<String>) = match redis_cmd("SCAN")
            .cursor_arg(cursor)
            .arg("MATCH")
            .arg(pattern)
            .arg("COUNT")
            .arg(100)
            .query_async(&mut conn)
            .await
        {
            Ok(result) => result,
            Err(error) => {
                tracing::warn!(
                    ?error,
                    %pattern,
                    %context,
                    "redis SCAN failed"
                );
                return;
            }
        };
        keys.append(&mut batch);
        cursor = next_cursor;
        if cursor == 0 {
            break;
        }
    }
    if !keys.is_empty()
        && let Err(error) = conn.del::<_, ()>(&keys[..]).await
    {
        tracing::warn!(
            ?error,
            key_count = keys.len(),
            %context,
            "failed to delete keys from redis"
        );
    }
}

impl SessionSuppressionState {
    pub const DEFAULT_TTL_SECS: u64 = 3600;

    pub fn new(redis_client: Option<RedisClient>, ttl_secs: u64) -> Self {
        Self {
            inner: Arc::default(),
            redis_client,
            ttl_secs,
        }
    }

    fn redis_key(session_id: &str, repo_path: &str) -> String {
        format!("suppression:{}::{}", session_id.trim(), repo_path.trim())
    }

    fn local_key(session_id: &str, repo_path: &str) -> String {
        format!("{}::{}", session_id.trim(), repo_path.trim())
    }

    fn session_prefix(session_id: &str) -> String {
        format!("suppression:{}", session_id.trim())
    }

    /// Escapes Redis glob metacharacters in `raw` so it is safe to embed in a
    /// `MATCH` pattern without matching unintended keys.
    ///
    /// Redis SCAN / KEYS treats `*`, `?`, `[`, `]`, and `\` as special. Each
    /// is prefixed with `\` so it matches literally. A session_id like `"*"`
    /// becomes `"\*"` instead of the wildcard that would wipe all sessions.
    fn escape_redis_glob(raw: &str) -> String {
        let mut escaped = String::with_capacity(raw.len());
        for ch in raw.chars() {
            if matches!(ch, '*' | '?' | '[' | ']' | '\\') {
                escaped.push('\\');
            }
            escaped.push(ch);
        }
        escaped
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

    async fn try_redis_setex(&self, session_id: &str, repo_path: &str, entry: &SuppressionEntry) {
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

    pub fn clear_session(&self, session_id: &str) {
        let prefix = Self::session_prefix(session_id);
        self.inner.retain(|key, _| !key.starts_with(&prefix));

        let redis_client = self.redis_client.clone();
        let escaped_sid = Self::escape_redis_glob(session_id.trim());
        let pattern = format!("suppression:{escaped_sid}:*");
        tokio::spawn(async move {
            if let Some(client) = redis_client {
                scan_and_del_pattern(&client, &pattern, "suppression_clear").await;
            }
        });
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

    #[test]
    fn escape_redis_glob_escapes_all_metacharacters() {
        // Wildcard `*` must not match all sessions when embedded in a pattern.
        assert_eq!(SessionSuppressionState::escape_redis_glob("*"), r"\*");
        // Other metacharacters are also escaped.
        assert_eq!(
            SessionSuppressionState::escape_redis_glob("abc?[def]\\xyz"),
            r"abc\?\[def\]\\xyz"
        );
        // Plain session IDs pass through unchanged.
        assert_eq!(
            SessionSuppressionState::escape_redis_glob("session-abc-123"),
            "session-abc-123"
        );
    }

    #[test]
    fn clear_session_pattern_escapes_wildcard_session_id() {
        // Verify that a session_id of "*" does NOT produce "suppression:*:*"
        // which would match every suppression key.
        let _state = SessionSuppressionState::default();
        // We cannot call clear_session and inspect the pattern directly, but
        // we can verify the escaping helper produces the safe escaped form
        // and that session_prefix uses the raw id (not the glob-safe form,
        // which is intentional — the in-memory DashMap uses exact-prefix match,
        // not glob matching).
        let escaped = SessionSuppressionState::escape_redis_glob("*");
        assert_eq!(
            escaped, r"\*",
            "wildcard session must be escaped for Redis SCAN"
        );
    }
}
