//! Postgres-backed persisted embedding cache for the skill-graph snapshot builder.
//!
//! # Purpose
//!
//! Eliminates the ~7-minute full-corpus re-embed on every boot/reload (T17 AC2 + AC3).
//! On an unchanged 262-skill corpus, all embedding batches collapse to ~zero embed calls
//! by loading precomputed vectors from this store.  Only skills whose content or model
//! changed are re-embedded and then upserted back.
//!
//! # Cache key
//!
//! Each row is keyed by `(skill_id TEXT, view_kind TEXT, model_name TEXT)`.  The
//! `skill_id` is the UUID stored in `skills.id` (as text), which is the durable stable
//! identity after rebuild.  The `view_kind` is a short string identifying which text
//! was embedded (see [`ViewKind`] constants).  The `model_name` is the Ollama model
//! identifier (e.g. `"qwen3-embedding:4b"`).
//!
//! # Dimension mismatch
//!
//! A cached row whose stored `dimension` does not equal the requested dimension is a
//! hard fail-loud error ([`EmbeddingCacheError::DimensionMismatch`]).  This is
//! consistent with #235 semantics: serving a wrong-dimension vector would silently
//! corrupt cosine similarity scores.
//!
//! # Blank-view invariant
//!
//! Blank view texts are never written to this table (callers must not pass rows with
//! empty vectors).  The load path returns an absent entry (cache miss) for any
//! `(skill_id, view_kind)` not present in the table, so the caller sees an empty
//! `Vec<f32>` — identical to the `embed_dense_view_skipping_blank` blank-skip
//! semantics from T09.
//!
//! # Vector encoding
//!
//! `f32` embeddings are stored as little-endian IEEE-754 bytes in a `BYTEA` column.
//! Use [`encode_f32_vector`] / [`decode_f32_vector`] for exact-roundtrip conversion.

use std::collections::HashMap;

use sqlx::PgPool;
use thiserror::Error;

// ── View-kind constants ──────────────────────────────────────────────────────

/// View-kind label for the primary skill embedding (name + description + tags).
pub const VIEW_KIND_E_SUMMARY: &str = "e_summary";
/// View-kind label for the task-trigger dense view (T09 e_task).
pub const VIEW_KIND_E_TASK: &str = "e_task";
/// View-kind label for the needs/requirements dense view (T09 e_needs).
pub const VIEW_KIND_E_NEEDS: &str = "e_needs";
/// View-kind label for the avoid-when dense view (T09 e_negative).
pub const VIEW_KIND_E_NEGATIVE: &str = "e_negative";

/// Returns the view-kind label for a subunit at `position` (0-indexed).
///
/// The label is `subunit:{position}`, e.g. `"subunit:0"`, `"subunit:1"`.
/// Position matches `skill_subunits.position` — the stable ordering column
/// written by the graph rebuild path.
pub fn subunit_view_kind(position: usize) -> String {
    format!("subunit:{position}")
}

// ── Errors ───────────────────────────────────────────────────────────────────

/// Errors produced by the embedding cache store.
#[derive(Debug, Error)]
pub enum EmbeddingCacheError {
    /// A cached vector's stored dimension does not match the requested model dimension.
    ///
    /// This is a hard fail-loud error: serving a wrong-dimension vector would silently
    /// corrupt cosine similarity scores and break retrieval quality.  The operator must
    /// clear the stale row (or the full table) before the mcp-server can proceed.
    #[error(
        "cached embedding dimension mismatch: skill_id={skill_id:?}, view_kind={view_kind:?}, \
         model_name={model_name:?} — cached dimension={cached_dimension}, \
         requested dimension={requested_dimension}; \
         clear the skill_embeddings table or re-embed with the current model"
    )]
    DimensionMismatch {
        skill_id: String,
        view_kind: String,
        model_name: String,
        cached_dimension: usize,
        requested_dimension: usize,
    },
    /// An underlying Postgres query failed.
    #[error("embedding cache persistence error: {0}")]
    Persistence(#[from] sqlx::Error),
}

// ── Row types ─────────────────────────────────────────────────────────────────

/// A single row to upsert into the `skill_embeddings` cache.
///
/// The caller is responsible for:
/// - Computing `content_hash` via [`content_hash_for_view_text`].
/// - Ensuring `vector` is non-empty (blank-view rows must NOT be upserted).
/// - Setting `dimension` to the real vector length (`vector.len()`).
#[derive(Debug, Clone)]
pub struct EmbeddingCacheRow {
    /// UUID of the skill (from `skills.id::TEXT`), the durable stable identity.
    pub skill_id: String,
    /// View-kind label (use the `VIEW_KIND_*` constants or [`subunit_view_kind`]).
    pub view_kind: String,
    /// Ollama model identifier, e.g. `"qwen3-embedding:4b"`.
    pub model_name: String,
    /// Real vector dimension — must equal `vector.len()`.
    pub dimension: usize,
    /// BLAKE3 hex hash of the exact view text that was embedded.
    pub content_hash: String,
    /// The f32 embedding vector.  Must be non-empty.
    pub vector: Vec<f32>,
}

/// A single cache entry loaded from `skill_embeddings`.
///
/// Returned by [`EmbeddingCacheStore::load_for_model`]; keyed in the output
/// map by `(skill_id, view_kind)`.
#[derive(Debug, Clone)]
pub struct LoadedEmbedding {
    /// BLAKE3 hex hash of the view text at the time this embedding was computed.
    pub content_hash: String,
    /// The f32 embedding vector, decoded from little-endian BYTEA storage.
    pub vector: Vec<f32>,
}

// ── Vector encoding helpers ───────────────────────────────────────────────────

/// Encodes an `f32` vector as little-endian IEEE-754 bytes for BYTEA storage.
///
/// The encoding is exact-roundtrip: `decode_f32_vector(encode_f32_vector(&v)) == v`
/// for any finite or non-finite `f32` slice.  The output length is `4 * v.len()`.
pub fn encode_f32_vector(v: &[f32]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(v.len() * 4);
    for &x in v {
        bytes.extend_from_slice(&x.to_le_bytes());
    }
    bytes
}

/// Decodes a little-endian BYTEA blob back into an `f32` vector.
///
/// The input length must be a multiple of 4.  Returns an error string if the
/// byte count is not aligned — this indicates corruption or a wrong-type column.
///
/// # Errors
///
/// Returns `Err` when `bytes.len() % 4 != 0`.
pub fn decode_f32_vector(bytes: &[u8]) -> Result<Vec<f32>, String> {
    if !bytes.len().is_multiple_of(4) {
        return Err(format!(
            "BYTEA length {} is not a multiple of 4 — cannot decode as f32 vector",
            bytes.len()
        ));
    }
    Ok(bytes
        .chunks_exact(4)
        .map(|chunk| {
            let arr: [u8; 4] = chunk
                .try_into()
                .expect("chunks_exact always yields 4 bytes");
            f32::from_le_bytes(arr)
        })
        .collect())
}

// ── Content hashing ────────────────────────────────────────────────────────────

/// Computes the BLAKE3 hex hash of a view text string for cache-key purposes.
///
/// The hash covers the exact byte representation of `text`.  If the text
/// changes by even one character, the hash changes and the cache entry is
/// considered stale (cache miss → re-embed).
pub fn content_hash_for_view_text(text: &str) -> String {
    blake3::hash(text.as_bytes()).to_hex().to_string()
}

// ── Store ─────────────────────────────────────────────────────────────────────

/// Postgres adapter for the persisted embedding cache.
///
/// Reads and writes the `skill_embeddings` table created by migration 011.
/// Constructed from a `PgPool` — same pattern as [`PostgresGraphSnapshotStore`].
///
/// All public methods are async and map `sqlx::Error` to [`EmbeddingCacheError`].
#[derive(Debug, Clone)]
pub struct EmbeddingCacheStore {
    pool: PgPool,
}

impl EmbeddingCacheStore {
    /// Creates a new store backed by the given Postgres connection pool.
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Loads all cached embeddings for the given `model_name` and verifies
    /// that every row's stored dimension equals `requested_dimension`.
    ///
    /// Returns a map keyed by `(skill_id, view_kind)` so callers can check
    /// cache presence by performing a `HashMap::get` per (skill, view) pair.
    ///
    /// # Errors
    ///
    /// - [`EmbeddingCacheError::DimensionMismatch`] — if any cached row's
    ///   stored `dimension` does not equal `requested_dimension`.  This is
    ///   fail-loud by design: the model name matches but the dimension changed,
    ///   indicating a model server change that would corrupt retrieval scores.
    ///
    /// - [`EmbeddingCacheError::Persistence`] — on any Postgres I/O error.
    pub async fn load_for_model(
        &self,
        model_name: &str,
        requested_dimension: usize,
    ) -> Result<HashMap<(String, String), LoadedEmbedding>, EmbeddingCacheError> {
        let rows = sqlx::query_as::<_, (String, String, i32, String, Vec<u8>)>(
            r#"
            SELECT skill_id, view_kind, dimension, content_hash, vector
            FROM skill_embeddings
            WHERE model_name = $1
            "#,
        )
        .bind(model_name)
        .fetch_all(&self.pool)
        .await?;

        let mut cache: HashMap<(String, String), LoadedEmbedding> =
            HashMap::with_capacity(rows.len());

        for (skill_id, view_kind, dimension, content_hash, vector_bytes) in rows {
            let cached_dimension = dimension as usize;
            if cached_dimension != requested_dimension {
                return Err(EmbeddingCacheError::DimensionMismatch {
                    skill_id,
                    view_kind,
                    model_name: model_name.to_owned(),
                    cached_dimension,
                    requested_dimension,
                });
            }
            let vector = decode_f32_vector(&vector_bytes).map_err(|msg| {
                sqlx::Error::Decode(
                    format!(
                        "failed to decode vector for skill_id={skill_id:?}, \
                         view_kind={view_kind:?}: {msg}"
                    )
                    .into(),
                )
            })?;
            cache.insert(
                (skill_id, view_kind),
                LoadedEmbedding {
                    content_hash,
                    vector,
                },
            );
        }

        Ok(cache)
    }

    /// Upserts a batch of embedding rows into `skill_embeddings`.
    ///
    /// Uses `ON CONFLICT (skill_id, view_kind, model_name) DO UPDATE` so
    /// re-embedding a changed skill replaces its cached row atomically.
    ///
    /// Rows with an empty vector must NOT be passed — callers are responsible
    /// for filtering blank views before calling this method.  An empty-vector
    /// row would violate the blank-view invariant: loading it back would return
    /// an empty vector that the caller might mistake for a real embedding.
    ///
    /// # Errors
    ///
    /// Returns [`EmbeddingCacheError::Persistence`] on any Postgres I/O error.
    pub async fn upsert_many(&self, rows: &[EmbeddingCacheRow]) -> Result<(), EmbeddingCacheError> {
        if rows.is_empty() {
            return Ok(());
        }
        let mut tx = self.pool.begin().await?;

        for row in rows {
            debug_assert!(
                !row.vector.is_empty(),
                "EmbeddingCacheStore::upsert_many must not be called with an empty vector \
                 (view_kind={:?}, skill_id={:?}); callers must filter blank views before upserting",
                row.view_kind,
                row.skill_id
            );
            let encoded = encode_f32_vector(&row.vector);
            sqlx::query(
                r#"
                INSERT INTO skill_embeddings
                    (skill_id, view_kind, model_name, dimension, content_hash, vector, updated_at)
                VALUES ($1, $2, $3, $4, $5, $6, NOW())
                ON CONFLICT (skill_id, view_kind, model_name) DO UPDATE
                    SET dimension    = EXCLUDED.dimension,
                        content_hash = EXCLUDED.content_hash,
                        vector       = EXCLUDED.vector,
                        updated_at   = NOW()
                "#,
            )
            .bind(&row.skill_id)
            .bind(&row.view_kind)
            .bind(&row.model_name)
            .bind(row.dimension as i32)
            .bind(&row.content_hash)
            .bind(&encoded)
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;
        Ok(())
    }
}

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── Vector encode/decode roundtrip ────────────────────────────────────────

    /// Proves that encode → decode is an exact roundtrip for typical embedding values.
    #[test]
    fn f32_vector_bytea_roundtrip_is_exact() {
        let original: Vec<f32> = vec![0.0, 1.0, -1.0, 0.5, -0.5, f32::MAX, f32::MIN_POSITIVE];
        let encoded = encode_f32_vector(&original);
        let decoded =
            decode_f32_vector(&encoded).expect("decode must succeed for well-formed bytes");
        assert_eq!(
            original, decoded,
            "decoded vector must exactly equal the original (bit-for-bit via LE bytes)"
        );
    }

    /// Proves that an empty vector encodes to zero bytes and decodes back to empty.
    #[test]
    fn f32_vector_bytea_roundtrip_empty() {
        let original: Vec<f32> = vec![];
        let encoded = encode_f32_vector(&original);
        assert!(
            encoded.is_empty(),
            "encoding an empty vector must produce zero bytes"
        );
        let decoded = decode_f32_vector(&encoded).expect("decode of empty bytes must succeed");
        assert!(
            decoded.is_empty(),
            "decoded result of zero bytes must be an empty vector"
        );
    }

    /// Proves that a non-aligned byte slice returns an error, not a panic or silent truncation.
    #[test]
    fn f32_vector_decode_rejects_misaligned_bytes() {
        let bad_bytes: Vec<u8> = vec![0x00, 0x01, 0x02]; // 3 bytes — not a multiple of 4
        let result = decode_f32_vector(&bad_bytes);
        assert!(
            result.is_err(),
            "decode_f32_vector must return Err for byte slices not aligned to 4 bytes"
        );
    }

    /// Proves NaN round-trips exactly (bit pattern is preserved).
    #[test]
    fn f32_vector_bytea_roundtrip_preserves_nan_bit_pattern() {
        let nan = f32::NAN;
        let encoded = encode_f32_vector(&[nan]);
        let decoded = decode_f32_vector(&encoded).expect("decode must succeed");
        // NaN != NaN by IEEE definition; compare bit patterns instead.
        assert_eq!(
            nan.to_bits(),
            decoded[0].to_bits(),
            "bit pattern of NaN must survive roundtrip"
        );
    }

    // ── Content hash ─────────────────────────────────────────────────────────

    /// Proves that identical texts produce the same hash (cache hit condition).
    #[test]
    fn content_hash_same_text_produces_same_hash() {
        let text = "rust error handling patterns";
        let h1 = content_hash_for_view_text(text);
        let h2 = content_hash_for_view_text(text);
        assert_eq!(h1, h2, "same text must hash to the same value");
    }

    /// Proves that different texts produce different hashes (cache miss condition).
    #[test]
    fn content_hash_different_text_produces_different_hash() {
        let h1 = content_hash_for_view_text("handle errors explicitly");
        let h2 = content_hash_for_view_text("handle errors implicitly");
        assert_ne!(h1, h2, "different texts must produce different hashes");
    }

    /// Proves that empty text has a stable, non-empty hash string.
    #[test]
    fn content_hash_of_empty_text_is_non_empty_string() {
        let h = content_hash_for_view_text("");
        assert!(
            !h.is_empty(),
            "hash of empty text must be a non-empty hex string"
        );
    }

    // ── Cache hit / miss logic (in-memory simulation) ─────────────────────────

    /// Proves that when content_hash matches a cache entry, the cached vector
    /// is returned and no re-embed is needed (cache-hit scenario).
    ///
    /// This test simulates the load-or-embed-and-cache decision loop in
    /// `build_graph_from_pg` without requiring a database connection.
    #[test]
    fn cache_hit_returns_cached_vector_without_re_embed() {
        let skill_id = "skill-uuid-abc".to_owned();
        let view_kind = VIEW_KIND_E_SUMMARY.to_owned();
        let text = "debugging Rust async code";
        let hash = content_hash_for_view_text(text);
        let cached_vector: Vec<f32> = vec![0.1, 0.2, 0.3];

        // Simulate a loaded cache containing one entry.
        let mut cache: HashMap<(String, String), LoadedEmbedding> = HashMap::new();
        cache.insert(
            (skill_id.clone(), view_kind.clone()),
            LoadedEmbedding {
                content_hash: hash.clone(),
                vector: cached_vector.clone(),
            },
        );

        // The cache-hit check: same hash → return cached vector, counter stays 0.
        let mut embed_calls: usize = 0;
        let result = if let Some(entry) = cache.get(&(skill_id, view_kind)) {
            if entry.content_hash == hash {
                entry.vector.clone()
            } else {
                embed_calls += 1;
                vec![0.9, 0.9, 0.9] // would come from embedder
            }
        } else {
            embed_calls += 1;
            vec![0.9, 0.9, 0.9]
        };

        assert_eq!(
            result, cached_vector,
            "cache hit must return the cached vector"
        );
        assert_eq!(embed_calls, 0, "cache hit must not trigger an embed call");
    }

    /// Proves that when the content_hash changes (text was modified), the cached
    /// entry is NOT used and a re-embed is triggered.
    #[test]
    fn cache_miss_on_content_change_triggers_re_embed() {
        let skill_id = "skill-uuid-xyz".to_owned();
        let view_kind = VIEW_KIND_E_TASK.to_owned();
        let old_text = "use cargo test";
        let new_text = "use cargo nextest"; // changed
        let cached_hash = content_hash_for_view_text(old_text);
        let new_hash = content_hash_for_view_text(new_text);

        let mut cache: HashMap<(String, String), LoadedEmbedding> = HashMap::new();
        cache.insert(
            (skill_id.clone(), view_kind.clone()),
            LoadedEmbedding {
                content_hash: cached_hash.clone(),
                vector: vec![0.1, 0.2, 0.3],
            },
        );

        // Simulate the check: new_hash != cached_hash → re-embed required.
        let mut embed_calls: usize = 0;
        let result = if let Some(entry) = cache.get(&(skill_id, view_kind)) {
            if entry.content_hash == new_hash {
                entry.vector.clone()
            } else {
                embed_calls += 1;
                vec![0.7, 0.8, 0.9] // fresh embedding
            }
        } else {
            embed_calls += 1;
            vec![0.7, 0.8, 0.9]
        };

        assert_ne!(
            result,
            vec![0.1_f32, 0.2, 0.3],
            "stale cache must not be returned"
        );
        assert_eq!(
            embed_calls, 1,
            "content change must trigger exactly one re-embed call"
        );
    }

    // ── Dimension mismatch construction ───────────────────────────────────────

    /// Proves that the DimensionMismatch error carries the correct diagnostic fields.
    ///
    /// This mirrors the check performed inside `load_for_model` — if a cached
    /// row's dimension differs from the requested dimension, the error must name
    /// the offending skill, view, model, and both dimensions so the operator can
    /// diagnose the mismatch without a debugger.
    #[test]
    fn dimension_mismatch_error_contains_all_diagnostic_fields() {
        let error = EmbeddingCacheError::DimensionMismatch {
            skill_id: "skill-uuid-aaa".to_owned(),
            view_kind: "e_summary".to_owned(),
            model_name: "qwen3-embedding:4b".to_owned(),
            cached_dimension: 768,
            requested_dimension: 2560,
        };
        let msg = error.to_string();
        assert!(
            msg.contains("skill-uuid-aaa"),
            "error must include skill_id"
        );
        assert!(msg.contains("e_summary"), "error must include view_kind");
        assert!(
            msg.contains("qwen3-embedding:4b"),
            "error must include model_name"
        );
        assert!(msg.contains("768"), "error must include cached_dimension");
        assert!(
            msg.contains("2560"),
            "error must include requested_dimension"
        );
    }

    // ── Subunit view-kind naming ───────────────────────────────────────────────

    /// Proves that subunit_view_kind returns the expected positional label.
    #[test]
    fn subunit_view_kind_formats_position_correctly() {
        assert_eq!(subunit_view_kind(0), "subunit:0");
        assert_eq!(subunit_view_kind(1), "subunit:1");
        assert_eq!(subunit_view_kind(99), "subunit:99");
    }

    // ── Live PG roundtrip (ignored; requires DATABASE_URL + live Postgres) ──────

    /// Full roundtrip: upsert a row → load it back → verify exact equality.
    ///
    /// Run with:
    ///   `cargo test -p infrastructure embedding_cache -- --ignored`
    ///
    /// Requires `DATABASE_URL` pointing to a live Postgres instance.  Applies all
    /// migrations via [`PostgresAdapter::run_migrations`] before the test body so
    /// the `skill_embeddings` table (migration 011) is guaranteed to exist.
    #[ignore]
    #[tokio::test]
    async fn live_pg_roundtrip_upsert_and_load() {
        use super::super::postgres::{PostgresAdapter, PostgresConfig};

        let db_url = std::env::var("DATABASE_URL")
            .expect("DATABASE_URL must be set for live postgres tests");

        let config = PostgresConfig {
            database_url: db_url,
            max_connections: 2,
            min_connections: 1,
            connect_timeout_secs: 5,
            acquire_timeout_secs: 5,
        };
        let adapter = PostgresAdapter::connect(&config)
            .await
            .expect("PostgresAdapter connect must succeed for live roundtrip test");
        adapter
            .run_migrations()
            .await
            .expect("run_migrations must succeed before live roundtrip test");

        let pool = adapter.pool().clone();

        // Clean up any leftover rows from previous runs.
        sqlx::query("DELETE FROM skill_embeddings WHERE skill_id = 'test-roundtrip-skill'")
            .execute(&pool)
            .await
            .expect("cleanup DELETE must succeed");

        let store = EmbeddingCacheStore::new(pool);

        let original_vector: Vec<f32> = (0..16).map(|i| i as f32 * 0.1).collect();
        let row = EmbeddingCacheRow {
            skill_id: "test-roundtrip-skill".to_owned(),
            view_kind: VIEW_KIND_E_SUMMARY.to_owned(),
            model_name: "test-model".to_owned(),
            dimension: original_vector.len(),
            content_hash: content_hash_for_view_text("test roundtrip text"),
            vector: original_vector.clone(),
        };

        store
            .upsert_many(std::slice::from_ref(&row))
            .await
            .expect("upsert_many must succeed");

        let loaded = store
            .load_for_model("test-model", original_vector.len())
            .await
            .expect("load_for_model must succeed");

        let entry = loaded
            .get(&(
                "test-roundtrip-skill".to_owned(),
                VIEW_KIND_E_SUMMARY.to_owned(),
            ))
            .expect("upserted entry must be present in load result");

        assert_eq!(
            entry.vector, original_vector,
            "loaded vector must be exactly equal to the upserted vector (bit-for-bit f32)"
        );
        assert_eq!(
            entry.content_hash,
            content_hash_for_view_text("test roundtrip text"),
            "loaded content_hash must match the upserted hash"
        );
    }

    /// Proves that load_for_model returns DimensionMismatch when a stored row's
    /// dimension does not match the requested dimension.
    ///
    /// Run with:
    ///   `cargo test -p infrastructure embedding_cache -- --ignored`
    ///
    /// Requires `DATABASE_URL` pointing to a live Postgres instance.  Applies all
    /// migrations via [`PostgresAdapter::run_migrations`] before the test body so
    /// the `skill_embeddings` table (migration 011) is guaranteed to exist.
    #[ignore]
    #[tokio::test]
    async fn live_pg_dimension_mismatch_fails_loud() {
        use super::super::postgres::{PostgresAdapter, PostgresConfig};

        let db_url = std::env::var("DATABASE_URL")
            .expect("DATABASE_URL must be set for live postgres tests");

        let config = PostgresConfig {
            database_url: db_url,
            max_connections: 2,
            min_connections: 1,
            connect_timeout_secs: 5,
            acquire_timeout_secs: 5,
        };
        let adapter = PostgresAdapter::connect(&config)
            .await
            .expect("PostgresAdapter connect must succeed for live mismatch test");
        adapter
            .run_migrations()
            .await
            .expect("run_migrations must succeed before live mismatch test");

        let pool = adapter.pool().clone();

        // Clean up leftovers.
        sqlx::query("DELETE FROM skill_embeddings WHERE skill_id = 'test-mismatch-skill'")
            .execute(&pool)
            .await
            .expect("cleanup DELETE must succeed");

        let store = EmbeddingCacheStore::new(pool);

        // Upsert a 4-dim vector.
        let row = EmbeddingCacheRow {
            skill_id: "test-mismatch-skill".to_owned(),
            view_kind: VIEW_KIND_E_SUMMARY.to_owned(),
            model_name: "mismatch-model".to_owned(),
            dimension: 4,
            content_hash: content_hash_for_view_text("mismatch test"),
            vector: vec![0.1, 0.2, 0.3, 0.4],
        };
        store
            .upsert_many(&[row])
            .await
            .expect("upsert_many must succeed");

        // Load requesting dimension 8 — must fail loud.
        let result = store.load_for_model("mismatch-model", 8).await;
        assert!(
            matches!(result, Err(EmbeddingCacheError::DimensionMismatch { .. })),
            "load_for_model must return DimensionMismatch when stored dim != requested dim; \
             got: {result:?}"
        );
    }
}
