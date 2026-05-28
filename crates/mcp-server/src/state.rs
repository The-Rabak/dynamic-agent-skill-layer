use std::collections::HashSet;
use std::sync::Arc;

use dashmap::DashMap;
use redis::AsyncCommands;
use serde::{Deserialize, Serialize};

use crate::tools::compile_context::CompileContextStatus;

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

    fn session_prefix(session_id: &str) -> String {
        format!("suppression:{}", session_id.trim())
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

    async fn try_redis_del_pattern(&self, session_id: &str) {
        let Some(client) = &self.redis_client else {
            return;
        };
        let mut conn = match client.get_multiplexed_async_connection().await {
            Ok(conn) => conn,
            Err(error) => {
                tracing::warn!(
                    ?error,
                    session_id = %session_id,
                    "failed to acquire redis connection for suppression clear"
                );
                return;
            }
        };
        let prefix = Self::session_prefix(session_id);
        let pattern = format!("{prefix}*");
        let keys: Vec<String> = match redis::cmd("KEYS").arg(&pattern).query_async(&mut conn).await {
            Ok(k) => k,
            Err(error) => {
                tracing::warn!(
                    ?error,
                    %pattern,
                    "redis KEYS failed during suppression clear"
                );
                return;
            }
        };
        if !keys.is_empty() {
            let _: () = conn.del(&keys[..]).await.unwrap_or_default();
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
        let session_id = session_id.to_owned();
        tokio::spawn(async move {
            let state = SessionSuppressionState {
                inner: Arc::default(),
                redis_client,
                ttl_secs: 0,
            };
            state.try_redis_del_pattern(&session_id).await;
        });
    }
}

impl Default for SessionSuppressionState {
    fn default() -> Self {
        Self::new(None, Self::DEFAULT_TTL_SECS)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CachedContext {
    pub status: CompileContextStatus,
    pub reason_code: Option<String>,
    pub additional_context: Option<String>,
    pub scopes_considered: Vec<String>,
    pub graph_version: i64,
}

#[derive(Debug, Clone)]
pub struct CompiledContextCache {
    inner: Arc<DashMap<String, CachedContext>>,
    session_index: Arc<DashMap<String, HashSet<String>>>,
    redis_client: Option<redis::Client>,
    ttl_secs: u64,
}

impl CompiledContextCache {
    pub const DEFAULT_TTL_SECS: u64 = 600;

    pub fn new(redis_client: Option<redis::Client>, ttl_secs: u64) -> Self {
        Self {
            inner: Arc::default(),
            session_index: Arc::default(),
            redis_client,
            ttl_secs,
        }
    }

    fn prompt_hash(prompt: &str) -> String {
        let mut hasher = blake3::Hasher::new();
        hasher.update(prompt.as_bytes());
        hasher.finalize().to_hex()[..16].to_owned()
    }

    fn scope_fingerprint(configured_scopes: &[String]) -> String {
        let mut sorted = configured_scopes.to_vec();
        sorted.sort();
        sorted.join(",")
    }

    fn cache_key(prompt: &str, configured_scopes: &[String]) -> String {
        format!(
            "cache:{}:{}",
            Self::prompt_hash(prompt),
            Self::scope_fingerprint(configured_scopes)
        )
    }

    fn session_prefix(session_id: &str) -> String {
        format!("cache_session:{}", session_id.trim())
    }

    fn redis_entry_key(session_id: &str, cache_key: &str) -> String {
        format!("{}:{}", Self::session_prefix(session_id), cache_key)
    }

    async fn try_redis_get(&self, redis_key: &str) -> Option<CachedContext> {
        let client = self.redis_client.as_ref()?;
        let mut conn = match client.get_multiplexed_async_connection().await {
            Ok(conn) => conn,
            Err(error) => {
                tracing::warn!(
                    ?error,
                    %redis_key,
                    "failed to acquire redis connection for cache lookup"
                );
                return None;
            }
        };
        let raw: Option<String> = match conn.get(redis_key).await {
            Ok(raw) => raw,
            Err(error) => {
                tracing::warn!(
                    ?error,
                    %redis_key,
                    "redis GET failed for cache key"
                );
                return None;
            }
        };
        match raw {
            Some(json) => match serde_json::from_str::<CachedContext>(&json) {
                Ok(entry) => Some(entry),
                Err(error) => {
                    tracing::warn!(
                        ?error,
                        %redis_key,
                        "failed to deserialize cache entry from redis"
                    );
                    None
                }
            },
            None => None,
        }
    }

    async fn try_redis_setex(
        &self,
        redis_key: &str,
        entry: &CachedContext,
    ) {
        let Some(client) = &self.redis_client else {
            return;
        };
        let mut conn = match client.get_multiplexed_async_connection().await {
            Ok(conn) => conn,
            Err(error) => {
                tracing::warn!(
                    ?error,
                    %redis_key,
                    "failed to acquire redis connection for cache write"
                );
                return;
            }
        };
        let value = match serde_json::to_string(entry) {
            Ok(json) => json,
            Err(error) => {
                tracing::warn!(
                    ?error,
                    %redis_key,
                    "failed to serialize cache entry for redis"
                );
                return;
            }
        };
        if let Err(error) = conn.set_ex::<_, _, ()>(redis_key, &value, self.ttl_secs).await {
            tracing::warn!(
                ?error,
                %redis_key,
                ttl_secs = self.ttl_secs,
                "redis SETEX failed for cache key"
            );
        }
    }

    async fn try_redis_del_pattern_cc(&self, session_id: &str) {
        let Some(client) = &self.redis_client else {
            return;
        };
        let mut conn = match client.get_multiplexed_async_connection().await {
            Ok(conn) => conn,
            Err(error) => {
                tracing::warn!(
                    ?error,
                    session_id = %session_id,
                    "failed to acquire redis connection for cache clear"
                );
                return;
            }
        };
        let prefix = Self::session_prefix(session_id);
        let pattern = format!("{prefix}*");
        let keys: Vec<String> = match redis::cmd("KEYS").arg(&pattern).query_async(&mut conn).await {
            Ok(k) => k,
            Err(error) => {
                tracing::warn!(
                    ?error,
                    %pattern,
                    "redis KEYS failed during cache clear"
                );
                return;
            }
        };
        if !keys.is_empty() {
            let _: () = conn.del(&keys[..]).await.unwrap_or_default();
        }
    }

    pub async fn get(
        &self,
        prompt: &str,
        configured_scopes: &[String],
        graph_version: i64,
    ) -> Option<CachedContext> {
        let key = Self::cache_key(prompt, configured_scopes);

        if let Some(redis_entry) = self
            .try_redis_get(&key)
            .await
        {
            if redis_entry.graph_version == graph_version {
                self.inner.insert(key.clone(), redis_entry.clone());
                return Some(redis_entry);
            }
        }

        self.inner
            .get(&key)
            .filter(|entry| entry.graph_version == graph_version)
            .map(|entry| entry.clone())
    }

    pub async fn set(
        &self,
        session_id: &str,
        prompt: &str,
        configured_scopes: &[String],
        entry: CachedContext,
    ) {
        let key = Self::cache_key(prompt, configured_scopes);
        let redis_key = Self::redis_entry_key(session_id, &key);

        {
            let mut index = self.session_index.entry(session_id.to_owned()).or_default();
            index.insert(key.clone());
        }

        self.inner.insert(key.clone(), entry.clone());
        self.try_redis_setex(&redis_key, &entry).await;
    }

    pub fn clear_session(&self, session_id: &str) {
        if let Some((_key, mut keys)) = self.session_index.remove(session_id) {
            for key in keys.drain() {
                self.inner.remove(&key);
            }
        }

        let redis_client = self.redis_client.clone();
        let session_id = session_id.to_owned();
        tokio::spawn(async move {
            let cache = CompiledContextCache {
                inner: Arc::default(),
                session_index: Arc::default(),
                redis_client,
                ttl_secs: 0,
            };
            cache.try_redis_del_pattern_cc(&session_id).await;
        });
    }
}

impl Default for CompiledContextCache {
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

    #[tokio::test]
    async fn cache_round_trips_same_prompt_same_version() {
        let cache = CompiledContextCache::default();
        let scopes = vec!["global".to_owned()];
        let entry = CachedContext {
            status: CompileContextStatus::Ok,
            reason_code: None,
            additional_context: Some("context".to_owned()),
            scopes_considered: scopes.clone(),
            graph_version: 7,
        };

        cache
            .set("session-a", "how to read a file", &scopes, entry.clone())
            .await;

        let cached = cache
            .get("how to read a file", &scopes, 7)
            .await;
        assert_eq!(cached, Some(entry));
    }

    #[tokio::test]
    async fn cache_miss_on_version_mismatch() {
        let cache = CompiledContextCache::default();
        let scopes = vec!["global".to_owned()];
        let entry = CachedContext {
            status: CompileContextStatus::Ok,
            reason_code: None,
            additional_context: Some("context".to_owned()),
            scopes_considered: scopes.clone(),
            graph_version: 7,
        };

        cache
            .set("session-a", "prompt", &scopes, entry)
            .await;

        assert!(cache.get("prompt", &scopes, 8).await.is_none());
    }

    #[tokio::test]
    async fn clear_session_removes_cached_entries() {
        let cache = CompiledContextCache::default();
        let scopes = vec!["global".to_owned()];
        let entry = CachedContext {
            status: CompileContextStatus::Ok,
            reason_code: None,
            additional_context: Some("ctx".to_owned()),
            scopes_considered: scopes.clone(),
            graph_version: 7,
        };

        cache
            .set("session-b", "prompt-1", &scopes, entry.clone())
            .await;
        cache
            .set("session-b", "prompt-2", &scopes, entry.clone())
            .await;
        cache
            .set("session-c", "prompt-1", &scopes, entry.clone())
            .await;

        cache.clear_session("session-b");

        assert!(cache.get("prompt-1", &scopes, 7).await.is_none());
        assert!(cache.get("prompt-2", &scopes, 7).await.is_none());
        assert!(cache.get("prompt-1", &scopes, 7).await.is_none());
    }

    #[tokio::test]
    async fn default_cache_ttl_is_ten_minutes() {
        let cache = CompiledContextCache::default();
        assert_eq!(cache.ttl_secs, 600);
    }
}