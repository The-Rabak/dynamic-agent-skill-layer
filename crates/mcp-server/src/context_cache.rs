use std::collections::BTreeMap;
use std::sync::Arc;

use dashmap::DashMap;
use infrastructure::{AsyncCommands, RedisClient};
use serde::{Deserialize, Serialize};

use crate::tools::compile_context::CompileContextStatus;

use crate::suppression_state::scan_and_del_pattern;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CachedContext {
    pub status: CompileContextStatus,
    pub reason_code: Option<String>,
    pub additional_context: Option<String>,
    pub scopes_considered: Vec<String>,
    pub graph_version: i64,
    pub health: BTreeMap<String, String>,
}

#[derive(Debug, Clone)]
pub struct CompiledContextCache {
    inner: Arc<DashMap<String, CachedContext>>,
    redis_client: Option<RedisClient>,
    ttl_secs: u64,
}

impl CompiledContextCache {
    pub const DEFAULT_TTL_SECS: u64 = 600;

    pub fn new(redis_client: Option<RedisClient>, ttl_secs: u64) -> Self {
        Self {
            inner: Arc::default(),
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

    fn cache_key(session_id: &str, prompt: &str, configured_scopes: &[String]) -> String {
        format!(
            "cache:{}:{}:{}",
            session_id.trim(),
            Self::prompt_hash(prompt),
            Self::scope_fingerprint(configured_scopes)
        )
    }

    fn cache_prefix_for_clear(session_id: &str) -> String {
        format!("cache:{}:", session_id.trim())
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

    async fn try_redis_setex(&self, redis_key: &str, entry: &CachedContext) {
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
        if let Err(error) = conn
            .set_ex::<_, _, ()>(redis_key, &value, self.ttl_secs)
            .await
        {
            tracing::warn!(
                ?error,
                %redis_key,
                ttl_secs = self.ttl_secs,
                "redis SETEX failed for cache key"
            );
        }
    }

    /// Returns a cached context entry if present. Checks local DashMap first
    /// (zero I/O), then falls back to Redis. On Redis hit, warms the local
    /// DashMap as a side effect for future calls.
    pub async fn get(
        &self,
        session_id: &str,
        prompt: &str,
        configured_scopes: &[String],
        graph_version: i64,
    ) -> Option<CachedContext> {
        let key = Self::cache_key(session_id, prompt, configured_scopes);

        if let Some(entry) = self.inner.get(&key) {
            if entry.graph_version == graph_version {
                return Some(entry.clone());
            }
        }

        if let Some(redis_entry) = self.try_redis_get(&key).await {
            if redis_entry.graph_version == graph_version {
                self.inner.insert(key.clone(), redis_entry.clone());
                return Some(redis_entry);
            }
        }

        None
    }

    pub async fn set(
        &self,
        session_id: &str,
        prompt: &str,
        configured_scopes: &[String],
        entry: CachedContext,
    ) {
        let key = Self::cache_key(session_id, prompt, configured_scopes);
        let redis_key = key.clone();

        self.inner.insert(key.clone(), entry.clone());
        self.try_redis_setex(&redis_key, &entry).await;
    }

    pub fn clear_session(&self, session_id: &str) {
        let prefix = Self::cache_prefix_for_clear(session_id);
        self.inner.retain(|key, _| !key.starts_with(&prefix));

        let redis_client = self.redis_client.clone();
        let sid = session_id.to_owned();
        let pattern = format!("cache:{}:*", sid.trim());
        tokio::spawn(async move {
            if let Some(client) = redis_client {
                scan_and_del_pattern(&client, &pattern, "cache_clear").await;
            }
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
    async fn cache_round_trips_same_prompt_same_version() {
        let cache = CompiledContextCache::default();
        let scopes = vec!["global".to_owned()];
        let entry = CachedContext {
            status: CompileContextStatus::Ok,
            reason_code: None,
            additional_context: Some("context".to_owned()),
            scopes_considered: scopes.clone(),
            graph_version: 7,
            health: BTreeMap::new(),
        };

        cache
            .set("session-a", "how to read a file", &scopes, entry.clone())
            .await;

        let cached = cache
            .get("session-a", "how to read a file", &scopes, 7)
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
            health: BTreeMap::new(),
        };

        cache.set("session-a", "prompt", &scopes, entry).await;

        assert!(cache.get("session-a", "prompt", &scopes, 8).await.is_none());
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
            health: BTreeMap::new(),
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

        assert!(
            cache
                .get("session-b", "prompt-1", &scopes, 7)
                .await
                .is_none()
        );
        assert!(
            cache
                .get("session-b", "prompt-2", &scopes, 7)
                .await
                .is_none()
        );
        assert_eq!(
            cache.get("session-c", "prompt-1", &scopes, 7).await,
            Some(entry)
        );
    }

    #[tokio::test]
    async fn default_cache_ttl_is_ten_minutes() {
        let cache = CompiledContextCache::default();
        assert_eq!(cache.ttl_secs, 600);
    }
}
