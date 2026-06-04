/// Read-only infrastructure observers for the real-infra E2E harness.
///
/// These observers read from the REAL Postgres, Qdrant, and Redis instances
/// (host-mapped ports).  They NEVER mutate application state — observation
/// only.  White-box assertions and stage-log snapshots call these at each
/// pipeline stage to capture the ground truth.
///
/// # Ports (host-mapped, from docker-compose.test.yml)
/// - Postgres: `localhost:15432` (DSN: `skill_layer:skill_layer@localhost:15432/skill_layer_test`)
/// - Qdrant REST: `http://localhost:16333`, collection `skills`
/// - Redis: `redis://localhost:16379`, stream `skill-layer-events`
use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::stack::{POSTGRES_DSN, QDRANT_URL, REDIS_URL};

// ── Postgres ─────────────────────────────────────────────────────────────────

/// Read-only observer for the Postgres `skill_layer_test` database.
///
/// Uses `sqlx::PgPool` under the hood.  The pool is created on construction
/// and closed when the observer is dropped.
pub struct PgObserver {
    pool: sqlx::PgPool,
}

impl PgObserver {
    /// Connects to the host-mapped Postgres instance and returns an observer.
    ///
    /// Panics if the connection cannot be established.
    pub async fn connect() -> Self {
        let pool = sqlx::PgPool::connect(POSTGRES_DSN)
            .await
            .expect("PgObserver: should connect to test Postgres");
        Self { pool }
    }

    /// Reads the current `graph_version` from `graph_state` (singleton row).
    ///
    /// Returns `Err` when the query fails or the table is empty.
    pub async fn graph_version(&self) -> Result<i64, String> {
        let row = sqlx::query_as::<_, (i64,)>("SELECT graph_version FROM graph_state LIMIT 1")
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| format!("PgObserver::graph_version query failed: {e}"))?;

        row.map(|(v,)| v)
            .ok_or_else(|| "PgObserver::graph_version: graph_state table is empty".to_owned())
    }

    /// Returns the number of rows in `table`.
    ///
    /// `table` must be a valid table name (no SQL injection risk — used only
    /// internally with hard-coded names from the harness).
    pub async fn row_count(&self, table: &str) -> Result<i64, String> {
        // SAFETY: `table` is always a hard-coded literal from the harness.
        let sql = format!("SELECT COUNT(*) FROM {table}");
        let row = sqlx::query_as::<_, (i64,)>(&sql)
            .fetch_one(&self.pool)
            .await
            .map_err(|e| format!("PgObserver::row_count({table}) failed: {e}"))?;
        Ok(row.0)
    }

    /// Returns a snapshot of all table counts that matter for stage logging.
    ///
    /// Tables observed: `skills`, `subunits`, `communities`, `outbox_events`,
    /// `graph_state`.
    pub async fn table_counts(&self) -> HashMap<String, i64> {
        let tables = [
            "skills",
            "subunits",
            "communities",
            "outbox_events",
            "skill_subunits",
            "community_skills",
            "transcript_ingest_queue",
        ];
        let mut counts = HashMap::new();
        for table in tables {
            if let Ok(n) = self.row_count(table).await {
                counts.insert(table.to_owned(), n);
            }
        }
        counts
    }

    /// Fetches a skill row by its `stable_id` column.
    ///
    /// Returns `None` when no row matches.
    pub async fn skill_by_stable_id(&self, stable_id: &str) -> Option<HashMap<String, String>> {
        let row = sqlx::query_as::<_, (String, String, String)>(
            "SELECT stable_id, name, description FROM skills WHERE stable_id = $1 LIMIT 1",
        )
        .bind(stable_id)
        .fetch_optional(&self.pool)
        .await
        .ok()??;

        let mut map = HashMap::new();
        map.insert("stable_id".to_owned(), row.0);
        map.insert("name".to_owned(), row.1);
        map.insert("description".to_owned(), row.2);
        Some(map)
    }
}

// ── Qdrant ────────────────────────────────────────────────────────────────────

/// Read-only observer for the Qdrant `skills` collection.
///
/// Uses `reqwest` to query the REST API at `http://localhost:16333`.
pub struct QdrantObserver {
    http: reqwest::Client,
    base_url: String,
}

/// Summary of the Qdrant `skills` collection state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QdrantCollectionSnapshot {
    pub points_count: u64,
    pub status: String,
}

impl QdrantObserver {
    /// Creates an observer pointed at the host-mapped Qdrant REST port.
    pub fn new() -> Self {
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .build()
            .expect("QdrantObserver: reqwest client should build");
        Self {
            http,
            base_url: QDRANT_URL.to_owned(),
        }
    }

    /// Returns the current `points_count` and collection status for `skills`.
    pub async fn collection_snapshot(&self) -> Result<QdrantCollectionSnapshot, String> {
        let url = format!("{}/collections/skills", self.base_url);
        let resp = self
            .http
            .get(&url)
            .send()
            .await
            .map_err(|e| format!("QdrantObserver: GET {url} failed: {e}"))?;

        let body: Value = resp
            .json()
            .await
            .map_err(|e| format!("QdrantObserver: failed to parse collection info: {e}"))?;

        let points_count = body
            .pointer("/result/points_count")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);

        let status = body
            .pointer("/result/status")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_owned();

        Ok(QdrantCollectionSnapshot {
            points_count,
            status,
        })
    }

    /// Returns the total number of points in the `skills` collection.
    pub async fn points_count(&self) -> Result<u64, String> {
        Ok(self.collection_snapshot().await?.points_count)
    }

    /// Scrolls up to `limit` point IDs and their payloads from the `skills` collection.
    ///
    /// Returns the raw JSON `result` block from the scroll response.
    pub async fn scroll(&self, limit: u32) -> Result<Value, String> {
        let url = format!("{}/collections/skills/points/scroll", self.base_url);
        let body = serde_json::json!({
            "limit": limit,
            "with_payload": true,
            "with_vector": false,
        });

        let resp = self
            .http
            .post(&url)
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("QdrantObserver: POST {url} failed: {e}"))?;

        let json: Value = resp
            .json()
            .await
            .map_err(|e| format!("QdrantObserver: scroll parse failed: {e}"))?;

        Ok(json.get("result").cloned().unwrap_or(Value::Null))
    }
}

impl Default for QdrantObserver {
    fn default() -> Self {
        Self::new()
    }
}

// ── Redis ─────────────────────────────────────────────────────────────────────

/// Read-only observer for the Redis `skill-layer-events` stream.
///
/// Uses `redis` crate via `ConnectionManager`.  All operations are read-only
/// (`XLEN`, `XPENDING`, `XREAD`).
pub struct RedisObserver {
    client: redis::Client,
}

/// A single Redis stream entry (ID + key-value pairs).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RedisStreamEntry {
    pub id: String,
    pub fields: HashMap<String, String>,
}

impl RedisObserver {
    /// Creates an observer connected to the host-mapped Redis instance.
    pub fn new() -> Result<Self, String> {
        let client = redis::Client::open(REDIS_URL)
            .map_err(|e| format!("RedisObserver: failed to open client: {e}"))?;
        Ok(Self { client })
    }

    /// Returns the length of the `skill-layer-events` stream (`XLEN`).
    pub async fn xlen(&self) -> Result<u64, String> {
        let mut conn = self
            .client
            .get_multiplexed_async_connection()
            .await
            .map_err(|e| format!("RedisObserver: connection failed: {e}"))?;

        redis::cmd("XLEN")
            .arg("skill-layer-events")
            .query_async::<u64>(&mut conn)
            .await
            .map_err(|e| format!("RedisObserver::xlen failed: {e}"))
    }

    /// Returns the pending count for the `skill-layer` consumer group.
    ///
    /// Uses `XPENDING skill-layer-events skill-layer - + <count>` to count
    /// pending entries.
    pub async fn xpending_count(&self) -> Result<u64, String> {
        let mut conn = self
            .client
            .get_multiplexed_async_connection()
            .await
            .map_err(|e| format!("RedisObserver: connection failed: {e}"))?;

        // XPENDING <stream> <group> returns a summary: [count, min-id, max-id, consumers]
        let result: redis::Value = redis::cmd("XPENDING")
            .arg("skill-layer-events")
            .arg("skill-layer")
            .query_async(&mut conn)
            .await
            .map_err(|e| format!("RedisObserver::xpending failed: {e}"))?;

        // Extract count from the summary array
        match result {
            redis::Value::Array(ref arr) if !arr.is_empty() => match &arr[0] {
                redis::Value::Int(n) => Ok(*n as u64),
                _ => Ok(0),
            },
            redis::Value::Int(n) => Ok(n as u64),
            _ => Ok(0),
        }
    }

    /// Reads up to `count` recent entries from the `skill-layer-events` stream.
    ///
    /// Uses `XREVRANGE` to read from newest to oldest, then reverses the result
    /// so entries are returned in chronological order.
    pub async fn xread_recent(&self, count: usize) -> Result<Vec<RedisStreamEntry>, String> {
        let mut conn = self
            .client
            .get_multiplexed_async_connection()
            .await
            .map_err(|e| format!("RedisObserver: connection failed: {e}"))?;

        let result: redis::Value = redis::cmd("XREVRANGE")
            .arg("skill-layer-events")
            .arg("+")
            .arg("-")
            .arg("COUNT")
            .arg(count)
            .query_async(&mut conn)
            .await
            .map_err(|e| format!("RedisObserver::xread_recent failed: {e}"))?;

        parse_xrange_result(result)
    }

    /// Returns `true` when any recent stream entry's `envelope` JSON field
    /// contains `"event_type":"graph.rebuilt"` with a `graph_version > prev_version`.
    pub async fn graph_rebuilt_after(&self, prev_version: i64) -> Result<bool, String> {
        let entries = self.xread_recent(20).await?;
        for entry in &entries {
            if let Some(envelope_json) = entry.fields.get("envelope") {
                if let Ok(envelope) = serde_json::from_str::<Value>(envelope_json) {
                    let is_rebuilt = envelope.get("event_type").and_then(|v| v.as_str())
                        == Some("graph.rebuilt");
                    let version = envelope
                        .pointer("/payload/graph_version")
                        .and_then(|v| v.as_i64())
                        .unwrap_or(0);
                    if is_rebuilt && version > prev_version {
                        return Ok(true);
                    }
                }
            }
        }
        Ok(false)
    }
}

impl Default for RedisObserver {
    fn default() -> Self {
        Self::new().expect("RedisObserver: should connect to test Redis")
    }
}

/// Parses a Redis `XRANGE`/`XREVRANGE` response into `Vec<RedisStreamEntry>`.
fn parse_xrange_result(value: redis::Value) -> Result<Vec<RedisStreamEntry>, String> {
    let entries_raw = match value {
        redis::Value::Array(arr) => arr,
        _ => return Ok(vec![]),
    };

    let mut entries = Vec::new();
    for raw_entry in entries_raw {
        let parts = match raw_entry {
            redis::Value::Array(p) => p,
            _ => continue,
        };
        if parts.len() < 2 {
            continue;
        }
        let id = match &parts[0] {
            redis::Value::BulkString(b) => String::from_utf8_lossy(b).to_string(),
            redis::Value::SimpleString(s) => s.clone(),
            _ => continue,
        };
        let fields_raw = match &parts[1] {
            redis::Value::Array(f) => f,
            _ => continue,
        };

        let mut fields = HashMap::new();
        let mut i = 0;
        while i + 1 < fields_raw.len() {
            let key = match &fields_raw[i] {
                redis::Value::BulkString(b) => String::from_utf8_lossy(b).to_string(),
                redis::Value::SimpleString(s) => s.clone(),
                _ => {
                    i += 2;
                    continue;
                }
            };
            let val = match &fields_raw[i + 1] {
                redis::Value::BulkString(b) => String::from_utf8_lossy(b).to_string(),
                redis::Value::SimpleString(s) => s.clone(),
                _ => String::new(),
            };
            fields.insert(key, val);
            i += 2;
        }

        entries.push(RedisStreamEntry { id, fields });
    }

    Ok(entries)
}

// ── InfraSnapshot ─────────────────────────────────────────────────────────────

/// A point-in-time snapshot of all observable infrastructure state.
///
/// Captured at each significant stage boundary and embedded in the stage log.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InfraSnapshot {
    pub captured_at: String,
    pub pg_graph_version: Option<i64>,
    pub pg_table_counts: HashMap<String, i64>,
    pub qdrant_points_count: Option<u64>,
    pub qdrant_status: Option<String>,
    pub redis_stream_len: Option<u64>,
}

impl InfraSnapshot {
    /// Captures the current state of all three stores concurrently.
    pub async fn capture(pg: &PgObserver, qdrant: &QdrantObserver, redis: &RedisObserver) -> Self {
        let captured_at = chrono::Utc::now().to_rfc3339();

        let pg_graph_version = pg.graph_version().await.ok();
        let pg_table_counts = pg.table_counts().await;

        let qdrant_snapshot = qdrant.collection_snapshot().await;
        let qdrant_points_count = qdrant_snapshot.as_ref().ok().map(|s| s.points_count);
        let qdrant_status = qdrant_snapshot.ok().map(|s| s.status);

        let redis_stream_len = redis.xlen().await.ok();

        InfraSnapshot {
            captured_at,
            pg_graph_version,
            pg_table_counts,
            qdrant_points_count,
            qdrant_status,
            redis_stream_len,
        }
    }
}
