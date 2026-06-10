use std::time::Duration;

use reqwest::StatusCode;
use serde::Deserialize;
use serde_json::{Value, json};
use thiserror::Error;
use tokio::time::timeout;

use crate::persistence::outbox::{OutboxVectorStore, VectorPointListing};

/// Derives a Qdrant collection name scoped to the embedding model.
///
/// Collection names are model-keyed so that embeddings from different models
/// (with different dimensions) coexist side-by-side without collision. A
/// nomic-embed-text (768-dim) run and a qwen3-embedding:4b (2560-dim) run each
/// get their own collection; switching the env var selects the right one and
/// never clobbers the other.
///
/// The model name is lowercased and any character that is not ASCII alphanumeric
/// or `-` is replaced with `-`, then consecutive hyphens are collapsed. This
/// keeps the name safe as a Qdrant collection identifier (which must be a valid
/// C-style identifier or URL segment in many environments).
///
/// # Errors
///
/// Returns `QdrantError::InvalidConfiguration` when `model` is empty or consists
/// entirely of non-alphanumeric characters, which would produce a bare `skills__`
/// collection name and silently overlap with any other degenerate model name.
///
/// Note: a broader charset guard (`^[A-Za-z0-9_-]+$`) at config construction is
/// tracked by #241 and will compose with this slug-level guard.
///
/// Examples:
///   `"nomic-embed-text"`   → `Ok("skills__nomic-embed-text")`
///   `"qwen3-embedding:4b"` → `Ok("skills__qwen3-embedding-4b")`
///   `"some/model:latest"`  → `Ok("skills__some-model-latest")`
///   `""` / `"/:@"`         → `Err(InvalidConfiguration(...))`
pub fn model_keyed_collection_name(model: &str) -> Result<String, QdrantError> {
    // Replace every non-alphanumeric, non-hyphen character with a hyphen, then
    // split on hyphens and discard empty segments to collapse consecutive ones.
    // Using `split` + `filter` + `join` avoids the redundant intermediate `String`
    // allocation that the previous `collect::<String>().split(...)` chain incurred.
    let lowered = model.to_ascii_lowercase();
    let slug = lowered
        .split(|c: char| !c.is_ascii_alphanumeric() && c != '-')
        .flat_map(|part| part.split('-'))
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>()
        .join("-");

    if slug.is_empty() {
        return Err(QdrantError::InvalidConfiguration(format!(
            "OLLAMA_EMBED_MODEL produced an empty collection slug for model='{model}'; \
             the model name must contain at least one alphanumeric character"
        )));
    }

    Ok(format!("skills__{slug}"))
}

/// Sparse vector representation for hybrid search.
///
/// A sparse vector has a small number of non-zero components relative to its
/// nominal vocabulary size. In BM25-style retrieval, `indices` are term IDs and
/// `values` are the corresponding TF-IDF or BM25 weights. Qdrant accepts this
/// format in the `sparse` named-vector slot for IDF-modified sparse collections.
#[derive(Debug, Clone)]
pub struct SparseVector {
    /// Positions of non-zero components. Must be the same length as `values`.
    pub indices: Vec<u32>,
    /// Non-zero component weights corresponding to each index in `indices`.
    pub values: Vec<f32>,
}

/// A single result point from a hybrid Query-API call.
///
/// Qdrant's `fusion: rrf` step reciprocal-rank-fuses the dense and sparse
/// result lists, then returns at most `limit` points ordered by descending RRF
/// score. The `score` here is the RRF-fused value (not a raw cosine or dot
/// product), so it is comparable only within a single query result set.
#[derive(Debug, Clone)]
pub struct HybridHit {
    /// The numeric point ID stored in Qdrant.
    pub point_id: u64,
    /// RRF-fused relevance score (higher is better within this result set).
    pub score: f32,
    /// The full payload JSON attached to this point when it was upserted.
    pub payload: Value,
}

/// Derives the Qdrant collection name for the hybrid arm scoped to a model.
///
/// The hybrid arm stores both a named dense vector (`"dense"`) and a sparse
/// vector with an IDF modifier (`"sparse"`) in the same Qdrant collection.
/// Qdrant's named-vector schema is INCOMPATIBLE with the unnamed-vector schema
/// used by the existing dense-only collection (`skills__<model>`), so the
/// hybrid arm uses a distinct collection name to prevent schema collisions:
///
///   `skills__<slug>__hybrid`
///
/// The `__hybrid` suffix is composed entirely of ASCII lowercase letters and
/// underscores, which keeps the full name within the `^[A-Za-z0-9_-]+$` charset
/// enforced by `QdrantAdapter::new`. The base slug is derived by the same
/// algorithm as [`model_keyed_collection_name`].
///
/// # Errors
///
/// Propagates any error from `model_keyed_collection_name` (empty or all-symbol
/// model name).
///
/// Examples:
///   `"nomic-embed-text"`   → `Ok("skills__nomic-embed-text__hybrid")`
///   `"qwen3-embedding:4b"` → `Ok("skills__qwen3-embedding-4b__hybrid")`
pub fn model_keyed_hybrid_collection_name(model: &str) -> Result<String, QdrantError> {
    let base = model_keyed_collection_name(model)?;
    Ok(format!("{base}__hybrid"))
}

/// Configuration for the Qdrant REST HTTP adapter.
///
/// Qdrant exposes two interfaces on separate ports:
///   - REST (HTTP/JSON): port 6333 (container) / 16333 (host-mapped in test compose)
///   - gRPC:             port 6334 (container) / 16334 (host-mapped in test compose)
///
/// This adapter uses the REST interface exclusively. The `endpoint` field must
/// point to the REST base URL (`:6333` / `:16333`). Using the gRPC port here
/// will cause parse errors because gRPC uses binary framing, not HTTP/1.1.
///
/// Invariant: `QDRANT_URL` env var must export the REST base URL. Any
/// `docker-compose` environment block or test-runner export that sets
/// `QDRANT_URL` to a `:6334` (gRPC) address will break `check_connectivity`
/// with a `hyper::Parse(Version)` error.
#[derive(Debug, Clone)]
pub struct QdrantConfig {
    pub endpoint: String,
    pub timeout_ms: u64,
    pub collection_name: String,
}

impl Default for QdrantConfig {
    fn default() -> Self {
        Self {
            // Port 6333 is the Qdrant REST interface. Do NOT use 6334 (gRPC).
            endpoint: "http://127.0.0.1:6333".to_owned(),
            timeout_ms: 500,
            // LEGACY SENTINEL — never use this directly.
            // Real collections are always model-keyed via `model_keyed_collection_name(...)`,
            // which produces `skills__<slug>` names that encode the embedding dimension.
            // Any call site that spreads `..QdrantConfig::default()` MUST override
            // `collection_name` explicitly, or the adapter will target this sentinel
            // and fail at runtime when the collection does not exist.
            collection_name: "UNCONFIGURED__use_model_keyed_collection_name".to_owned(),
        }
    }
}

#[derive(Debug, Error)]
pub enum QdrantError {
    #[error("invalid qdrant configuration: {0}")]
    InvalidConfiguration(String),
    #[error("qdrant connectivity failure: {0}")]
    Connectivity(#[from] reqwest::Error),
    #[error("qdrant request timed out after {timeout_ms}ms")]
    Timeout { timeout_ms: u64 },
    #[error("qdrant endpoint returned unexpected status {status}")]
    UnexpectedStatus { status: StatusCode },
    #[error("qdrant response status field must be 'ok', got '{status}'")]
    UnexpectedResponse { status: String },
    /// The existing collection was created with a different vector size than the
    /// caller expects. Silently reusing a wrong-dimension collection corrupts
    /// cosine-similarity ranking; the only safe action is to fail loud and let
    /// the operator drop the old collection or change the model.
    #[error(
        "qdrant collection '{collection}' has dimension {observed} but caller expects {expected}; \
         drop the collection or change the embedder model — mixed-dimension vectors are invalid"
    )]
    DimensionMismatch {
        collection: String,
        observed: u64,
        expected: u64,
    },
}

/// Validates that a Qdrant collection name consists only of ASCII letters, digits,
/// hyphens, and underscores (`^[A-Za-z0-9_-]+$`).
///
/// Collection names are interpolated directly into Qdrant REST path segments via
/// `format!` (e.g. `/collections/{name}/points`). A name like `skills/../../admin`
/// would silently traverse the Qdrant path hierarchy and target arbitrary endpoints.
///
/// Returns `Err(QdrantError::InvalidConfiguration)` on violation. Callers should
/// invoke this at method entry on any `collection_name` argument that will be
/// used in a URL segment, not just at adapter construction time.
fn validate_collection_name(name: &str) -> Result<(), QdrantError> {
    if !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') {
        return Err(QdrantError::InvalidConfiguration(format!(
            "collection name {:?} contains characters outside [A-Za-z0-9_-]; \
             collection names are used as Qdrant REST path segments and must not \
             contain path traversal characters like '/', '.', or whitespace",
            name
        )));
    }
    Ok(())
}

/// Validates a QDRANT_URL value: requires an `http://` or `https://` scheme and
/// emits a loud warning when the host is not in the local-only allowlist.
///
/// # Scheme check (hard fail)
///
/// The value must start with `http://` or `https://`. Any other prefix (e.g. a
/// bare hostname, an empty string, or a gRPC `grpc://` address) returns
/// `Err(QdrantError::InvalidConfiguration)` immediately. This prevents the adapter
/// from silently making requests to unintended endpoints when the env var is wrong.
///
/// # Non-local host warning (loud warn, continue)
///
/// If the host extracted from the URL is not one of `localhost`, `127.0.0.1`,
/// `::1`, or `qdrant`, a `warn!` is emitted so the operator is alerted that skill
/// vectors will be sent to an external host. The function still returns `Ok(())` so
/// intentional remote deployments are not blocked.
///
/// Host parsing is done with lightweight `str` operations — no new crate dependency.
pub fn validate_qdrant_url(url: &str) -> Result<(), QdrantError> {
    let after_scheme = if let Some(rest) = url.strip_prefix("https://") {
        rest
    } else if let Some(rest) = url.strip_prefix("http://") {
        rest
    } else {
        return Err(QdrantError::InvalidConfiguration(format!(
            "QDRANT_URL {:?} must start with http:// or https://; \
             bare hostnames, grpc://, and other schemes are not supported",
            url
        )));
    };

    // Extract the host portion: everything before the first '/' or ':' that
    // follows the scheme+authority. IPv6 addresses are wrapped in brackets, e.g.
    // `[::1]:6333`, so we detect and strip brackets before the local-host check.
    let host_and_port = after_scheme.split('/').next().unwrap_or("");
    let host = if host_and_port.starts_with('[') {
        // IPv6 literal: `[::1]` or `[::1]:6333` — take the part inside `[…]`.
        host_and_port
            .trim_start_matches('[')
            .split(']')
            .next()
            .unwrap_or(host_and_port)
    } else {
        // IPv4 or hostname: strip optional port.
        host_and_port.split(':').next().unwrap_or(host_and_port)
    };

    const LOCAL_HOSTS: &[&str] = &["localhost", "127.0.0.1", "::1", "qdrant"];
    if !LOCAL_HOSTS.contains(&host) {
        tracing::warn!(
            qdrant_url = url,
            host,
            "QDRANT_URL points at non-local host {}; skill vectors will be sent externally \
             — counter to local-first; set QDRANT_URL to a local address unless you intend \
             a remote deployment",
            host
        );
    }

    Ok(())
}

#[derive(Debug, Clone)]
pub struct QdrantAdapter {
    client: reqwest::Client,
    pub config: QdrantConfig,
}

impl QdrantAdapter {
    pub fn from_config(config: QdrantConfig) -> Result<Self, QdrantError> {
        Self::new(reqwest::Client::new(), config)
    }

    pub fn new(client: reqwest::Client, config: QdrantConfig) -> Result<Self, QdrantError> {
        if config.endpoint.trim().is_empty() {
            return Err(QdrantError::InvalidConfiguration(
                "endpoint must not be blank".to_owned(),
            ));
        }

        validate_qdrant_url(&config.endpoint)?;

        if config.timeout_ms == 0 {
            return Err(QdrantError::InvalidConfiguration(
                "timeout_ms must be greater than zero".to_owned(),
            ));
        }

        // Charset guard for the collection name: only ASCII letters, digits, hyphens, and
        // underscores are permitted.  Qdrant collection names are interpolated directly into
        // REST path segments via `format!` (e.g. `/collections/{name}/points`), so a value
        // like `skills/../../admin` would silently traverse the Qdrant path hierarchy.
        //
        // This guard subsumes #234's empty-slug guard for the QDRANT_COLLECTION override path
        // (the model-keyed path already produces safe `[a-z0-9-]` slugs, but environment
        // overrides are operator-supplied and must be validated at construction).
        validate_collection_name(&config.collection_name)?;

        // An empty collection name passes the charset check (vacuously true) but is
        // also invalid — reject it explicitly so the error message is unambiguous.
        if config.collection_name.trim().is_empty() {
            return Err(QdrantError::InvalidConfiguration(
                "collection_name must not be blank".to_owned(),
            ));
        }

        Ok(Self { client, config })
    }

    pub async fn check_connectivity(&self) -> Result<(), QdrantError> {
        let endpoint = format!("{}/collections", self.config.endpoint.trim_end_matches('/'));
        let response = timeout(
            Duration::from_millis(self.config.timeout_ms),
            self.client.get(endpoint).send(),
        )
        .await
        .map_err(|_| QdrantError::Timeout {
            timeout_ms: self.config.timeout_ms,
        })??;

        if response.status() != StatusCode::OK {
            return Err(QdrantError::UnexpectedStatus {
                status: response.status(),
            });
        }

        let body: QdrantCollectionsResponse = response.json().await?;
        if body.status != "ok" {
            return Err(QdrantError::UnexpectedResponse {
                status: body.status,
            });
        }

        Ok(())
    }

    /// Ensures the named Qdrant collection exists with the given vector size.
    ///
    /// Idempotent under concurrent callers: if another process creates the collection
    /// between the GET probe and the PUT create, Qdrant returns `409 Conflict`.
    /// This method treats 409 as success — the collection exists, which is the goal.
    ///
    /// Both `mcp-server` and `graph-builder` call this on startup, so the race
    /// is a real cold-start scenario (bug #157).
    ///
    /// # Dimension guard
    ///
    /// If the collection already exists (HTTP 200), the observed vector size is
    /// extracted from the response and compared to `vector_size`. A mismatch
    /// returns `QdrantError::DimensionMismatch` — silently reusing a
    /// wrong-dimension collection corrupts cosine rankings and must never
    /// succeed quietly. When the body does not contain a parseable dimension
    /// (e.g. an older Qdrant version), the guard logs a warning and continues
    /// rather than blocking boot on a missing field.
    pub async fn ensure_collection(
        &self,
        collection_name: &str,
        vector_size: u64,
    ) -> Result<(), QdrantError> {
        let endpoint = format!(
            "{}/collections/{collection_name}",
            self.config.endpoint.trim_end_matches('/')
        );
        let response = self.client.get(&endpoint).send().await?;
        if response.status() == StatusCode::OK {
            // Collection already exists — verify its dimension matches expectations.
            // A mismatch means nomic and qwen vectors would be mixed in a single
            // collection; that silently corrupts cosine-similarity ranking.
            //
            // Note: if the JSON response body cannot be parsed (network/framing
            // error after headers), we log the real parse error so the operator
            // can distinguish a transient read failure from a genuinely missing
            // dimension field in an older Qdrant version.
            let body: Value = response.json().await.unwrap_or_else(|parse_err| {
                tracing::warn!(
                    error = %parse_err,
                    collection_name,
                    "could not parse Qdrant collection-info response body; \
                     falling through to missing-dimension path"
                );
                Value::Null
            });
            let observed_size: Option<u64> = body
                .pointer("/result/config/params/vectors/size")
                .and_then(Value::as_u64);
            match observed_size {
                Some(observed) if observed != vector_size => {
                    return Err(QdrantError::DimensionMismatch {
                        collection: collection_name.to_owned(),
                        observed,
                        expected: vector_size,
                    });
                }
                Some(_) => {
                    // Dimension matches — collection is correct; nothing to do.
                }
                None => {
                    // Qdrant response did not include a parseable dimension field.
                    // This can happen for two reasons: (1) the body failed to parse
                    // (logged above via unwrap_or_else) or (2) an older Qdrant
                    // version omits the size field. Log a warning and continue
                    // rather than blocking boot on a missing field.
                    tracing::warn!(
                        collection_name,
                        expected_dimension = vector_size,
                        "could not extract dimension from Qdrant collection-info; \
                         skipping dimension guard (upgrade Qdrant or check logs above)"
                    );
                }
            }
            return Ok(());
        }

        let create_endpoint = format!(
            "{}/collections/{collection_name}",
            self.config.endpoint.trim_end_matches('/')
        );
        let body = json!({
            "vectors": {
                "size": vector_size,
                "distance": "Cosine"
            }
        });
        let create_response = self
            .send_with_timeout(self.client.put(create_endpoint).json(&body))
            .await?;

        // 409 Conflict means a concurrent caller already created the collection;
        // the collection exists, which is the postcondition we need.
        if create_response.status() == StatusCode::CONFLICT {
            tracing::info!(
                collection_name,
                "qdrant collection already created by a concurrent caller (409 Conflict); treating as success"
            );
            return Ok(());
        }

        self.expect_ok_status(create_response).await?;
        Ok(())
    }

    /// Creates a hybrid Qdrant collection with a named dense vector and a sparse
    /// IDF-modified vector slot.
    ///
    /// # Named-vector vs unnamed-vector shape
    ///
    /// The existing dense-only collection uses Qdrant's *unnamed* vector shape:
    /// `{"vectors":{"size":N,"distance":"Cosine"}}`. A hybrid collection requires
    /// the *named* shape — `{"vectors":{"dense":{…}},"sparse_vectors":{"sparse":{…}}}` —
    /// which Qdrant treats as a different schema. The two shapes are incompatible
    /// under the same collection name; this method creates a *distinct* collection
    /// (use `model_keyed_hybrid_collection_name` for the name) so the dense-only
    /// path is never touched.
    ///
    /// The `"modifier":"idf"` on the sparse slot enables Qdrant's server-side
    /// IDF re-weighting, which improves BM25-style scoring without shipping raw
    /// term frequencies from the client.
    ///
    /// # Idempotency
    ///
    /// Like `ensure_collection`, this method treats HTTP 200 (collection exists)
    /// and HTTP 409 Conflict (concurrent create) as success — the collection
    /// existing is the postcondition. It does NOT re-check vector dimensions on
    /// 200 because the hybrid schema uses named vectors whose config JSON path
    /// differs from the unnamed-vector path; dimension checking for hybrid
    /// collections is left to C3.
    pub async fn ensure_hybrid_collection(
        &self,
        collection_name: &str,
        dense_vector_size: u64,
    ) -> Result<(), QdrantError> {
        validate_collection_name(collection_name)?;
        let base_url = self.config.endpoint.trim_end_matches('/');
        let probe_endpoint = format!("{base_url}/collections/{collection_name}");

        let probe_response = self.client.get(&probe_endpoint).send().await?;
        if probe_response.status() == StatusCode::OK {
            // Collection already exists with some schema — treat as success.
            // Dimension re-checking for named vectors is left to the C3 query arm.
            return Ok(());
        }

        let create_endpoint = format!("{base_url}/collections/{collection_name}");
        let body = json!({
            "vectors": {
                "dense": {
                    "size": dense_vector_size,
                    "distance": "Cosine"
                }
            },
            "sparse_vectors": {
                "sparse": {
                    "modifier": "idf"
                }
            }
        });
        let create_response = self
            .send_with_timeout(self.client.put(create_endpoint).json(&body))
            .await?;

        // 409 Conflict means a concurrent caller already created the collection.
        if create_response.status() == StatusCode::CONFLICT {
            tracing::info!(
                collection_name,
                "hybrid collection already created by a concurrent caller (409 Conflict); \
                 treating as success"
            );
            return Ok(());
        }

        self.expect_ok_status(create_response).await?;
        Ok(())
    }

    /// Upserts a single point with both a dense and a sparse vector into a
    /// hybrid collection.
    ///
    /// The point body uses Qdrant's named-vector form, which is required for
    /// collections created with `ensure_hybrid_collection`:
    ///
    /// ```json
    /// {"points":[{"id":<id>,
    ///   "vector":{"dense":[…],"sparse":{"indices":[…],"values":[…]}},
    ///   "payload":{…}}]}
    /// ```
    ///
    /// This method is a complement to `upsert_vector` (which uses the unnamed
    /// form for the existing dense-only collection). It does NOT modify
    /// `upsert_vector` or any caller of that method.
    ///
    /// `?wait=true` ensures the point is visible to subsequent reads by the
    /// same process, matching the existing upsert contract.
    pub async fn upsert_hybrid_point(
        &self,
        collection_name: &str,
        point_id: u64,
        dense: &[f32],
        sparse: &SparseVector,
        payload: &Value,
    ) -> Result<(), QdrantError> {
        validate_collection_name(collection_name)?;
        let endpoint = format!(
            "{}/collections/{collection_name}/points?wait=true",
            self.config.endpoint.trim_end_matches('/')
        );
        let body = json!({
            "points": [{
                "id": point_id,
                "vector": {
                    "dense": dense,
                    "sparse": {
                        "indices": sparse.indices,
                        "values": sparse.values
                    }
                },
                "payload": payload
            }]
        });
        let response = self
            .send_with_timeout(self.client.put(endpoint).json(&body))
            .await?;
        self.expect_ok_status(response).await?;
        Ok(())
    }

    /// Queries a hybrid collection using Qdrant's Query API with RRF fusion.
    ///
    /// Issues a `POST /collections/{collection}/points/query` request with a
    /// two-arm prefetch (dense cosine + sparse IDF) and a reciprocal-rank-fusion
    /// step. The request body shape is:
    ///
    /// ```json
    /// {"prefetch":[
    ///     {"query":<dense_vec>,"using":"dense","limit":<L>},
    ///     {"query":{"indices":[…],"values":[…]},"using":"sparse","limit":<L>}],
    ///  "query":{"fusion":"rrf"},
    ///  "limit":<L>,"with_payload":true}
    /// ```
    ///
    /// Results are returned in descending RRF-score order. The RRF score is an
    /// artifact of rank fusion and is NOT a raw cosine similarity or dot product;
    /// it is comparable only within a single query's result set.
    ///
    /// # Errors
    ///
    /// Returns `QdrantError::UnexpectedStatus` for non-200 responses, or
    /// `QdrantError::Connectivity`/`QdrantError::Timeout` for transport errors.
    pub async fn query_hybrid(
        &self,
        collection_name: &str,
        dense_query: &[f32],
        sparse_query: &SparseVector,
        limit: u64,
    ) -> Result<Vec<HybridHit>, QdrantError> {
        validate_collection_name(collection_name)?;
        let endpoint = format!(
            "{}/collections/{collection_name}/points/query",
            self.config.endpoint.trim_end_matches('/')
        );
        let body = json!({
            "prefetch": [
                {
                    "query": dense_query,
                    "using": "dense",
                    "limit": limit
                },
                {
                    "query": {
                        "indices": sparse_query.indices,
                        "values": sparse_query.values
                    },
                    "using": "sparse",
                    "limit": limit
                }
            ],
            "query": {"fusion": "rrf"},
            "limit": limit,
            "with_payload": true
        });
        let response = self
            .send_with_timeout(self.client.post(endpoint).json(&body))
            .await?;
        let parsed = self.expect_ok_status(response).await?;

        let points = parsed
            .pointer("/result/points")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();

        let hits = points
            .into_iter()
            .filter_map(|point| {
                let point_id = point.get("id").and_then(Value::as_u64)?;
                let score = point
                    .get("score")
                    .and_then(Value::as_f64)
                    .map(|s| s as f32)?;
                let payload = point
                    .get("payload")
                    .cloned()
                    .unwrap_or(Value::Object(serde_json::Map::new()));
                Some(HybridHit {
                    point_id,
                    score,
                    payload,
                })
            })
            .collect();

        Ok(hits)
    }

    async fn send_with_timeout(
        &self,
        request: reqwest::RequestBuilder,
    ) -> Result<reqwest::Response, QdrantError> {
        timeout(
            Duration::from_millis(self.config.timeout_ms),
            request.send(),
        )
        .await
        .map_err(|_| QdrantError::Timeout {
            timeout_ms: self.config.timeout_ms,
        })?
        .map_err(QdrantError::Connectivity)
    }

    async fn expect_ok_status(&self, response: reqwest::Response) -> Result<Value, QdrantError> {
        if response.status() != StatusCode::OK {
            return Err(QdrantError::UnexpectedStatus {
                status: response.status(),
            });
        }
        let body: Value = response.json().await?;
        let status = body.get("status").and_then(Value::as_str).unwrap_or("");
        if status != "ok" {
            return Err(QdrantError::UnexpectedResponse {
                status: status.to_owned(),
            });
        }
        Ok(body)
    }
}

#[derive(Debug, Deserialize)]
struct QdrantCollectionsResponse {
    status: String,
}

#[async_trait::async_trait]
impl OutboxVectorStore for QdrantAdapter {
    async fn upsert_vector(
        &self,
        point_id: u64,
        vector: &[f32],
        payload: &Value,
    ) -> Result<(), String> {
        let collection = &self.config.collection_name;
        let endpoint = format!(
            "{}/collections/{collection}/points?wait=true",
            self.config.endpoint.trim_end_matches('/')
        );
        let body = json!({
            "points": [
                {
                    "id": point_id,
                    "vector": vector,
                    "payload": payload
                }
            ]
        });
        let response = self
            .send_with_timeout(self.client.put(endpoint).json(&body))
            .await
            .map_err(|error| error.to_string())?;
        let result = self.expect_ok_status(response).await;
        if let Err(ref error) = result {
            // Surface write failures with vector size so an operator can distinguish
            // a dimension mismatch (the primary offline-bypass corruption path) from
            // transient network errors. This is especially important when Qdrant was
            // offline at boot and the dimension guard was skipped — the first write
            // batch surfaces the mismatch here rather than silently recording it only
            // in the outbox `failed` column.
            tracing::error!(
                collection,
                vector_size = vector.len(),
                %error,
                "qdrant upsert failed — if this mentions vector size, the collection \
                 may have a dimension mismatch (dimension guard was skipped at boot \
                 because Qdrant was offline; drop the collection or change OLLAMA_EMBED_MODEL)"
            );
        }
        result.map_err(|error| error.to_string())?;
        Ok(())
    }

    async fn has_vector(&self, point_id: u64) -> Result<bool, String> {
        let collection = &self.config.collection_name;
        let endpoint = format!(
            "{}/collections/{collection}/points/{}",
            self.config.endpoint.trim_end_matches('/'),
            point_id
        );
        let response = self
            .send_with_timeout(self.client.get(endpoint))
            .await
            .map_err(|error| error.to_string())?;

        if response.status() == StatusCode::NOT_FOUND {
            return Ok(false);
        }
        let body = self
            .expect_ok_status(response)
            .await
            .map_err(|error| error.to_string())?;
        Ok(body.get("result").is_some())
    }

    async fn list_point_ids(&self) -> Result<VectorPointListing, String> {
        let collection = &self.config.collection_name;
        let endpoint = format!(
            "{}/collections/{collection}/points/scroll",
            self.config.endpoint.trim_end_matches('/')
        );
        let mut all_point_ids = Vec::new();
        let mut next_page_offset: Option<Value> = None;
        let mut seen_offsets = std::collections::HashSet::new();

        loop {
            let mut body = json!({
                "limit": 256,
                "with_vector": false,
                "with_payload": false
            });
            if let Some(offset) = &next_page_offset {
                body["offset"] = offset.clone();
            }
            let response = self
                .send_with_timeout(self.client.post(endpoint.as_str()).json(&body))
                .await
                .map_err(|error| error.to_string())?;
            let parsed = self
                .expect_ok_status(response)
                .await
                .map_err(|error| error.to_string())?;
            let points = parsed
                .get("result")
                .and_then(|value| value.get("points"))
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            all_point_ids.extend(points.iter().filter_map(|point| {
                point.get("id").and_then(|id| match id {
                    Value::Number(number) => number.as_u64(),
                    Value::String(text) => text.parse::<u64>().ok(),
                    _ => None,
                })
            }));

            let page_offset = parsed
                .get("result")
                .and_then(|value| value.get("next_page_offset"))
                .cloned()
                .filter(|offset| !offset.is_null());
            match page_offset {
                Some(offset) => {
                    let offset_key = offset.to_string();
                    if !seen_offsets.insert(offset_key) {
                        return Err("qdrant scroll returned a repeated next_page_offset".to_owned());
                    }
                    next_page_offset = Some(offset);
                }
                None => {
                    return Ok(VectorPointListing {
                        point_ids: all_point_ids,
                        is_complete: true,
                    });
                }
            }
        }
    }

    /// Upserts a point with dense + sparse vectors into the named hybrid collection.
    ///
    /// Delegates to `QdrantAdapter::upsert_hybrid_point`, which uses Qdrant's
    /// named-vector form required for hybrid collections. The `hybrid_collection`
    /// argument overrides `self.config.collection_name` so the relay can target
    /// the `skills__<model>__hybrid` collection directly.
    async fn upsert_hybrid(
        &self,
        hybrid_collection: &str,
        point_id: u64,
        dense: &[f32],
        sparse_indices: &[u32],
        sparse_values: &[f32],
        payload: &Value,
    ) -> Result<(), String> {
        let sparse = SparseVector {
            indices: sparse_indices.to_vec(),
            values: sparse_values.to_vec(),
        };
        self.upsert_hybrid_point(hybrid_collection, point_id, dense, &sparse, payload)
            .await
            .map_err(|e| e.to_string())
    }

    async fn delete_points(&self, point_ids: &[u64]) -> Result<(), String> {
        if point_ids.is_empty() {
            return Ok(());
        }
        let collection = &self.config.collection_name;
        let endpoint = format!(
            "{}/collections/{collection}/points/delete?wait=true",
            self.config.endpoint.trim_end_matches('/')
        );
        let body = json!({ "points": point_ids });
        let response = self
            .send_with_timeout(self.client.post(endpoint).json(&body))
            .await
            .map_err(|error| error.to_string())?;
        self.expect_ok_status(response)
            .await
            .map_err(|error| error.to_string())?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
        task::JoinHandle,
    };

    /// Starts a one-shot TCP server that responds to the next request with the supplied status/body.
    async fn spawn_single_response_server(
        status_line: &str,
        body: &str,
    ) -> (String, JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("test listener should bind");
        let address = listener
            .local_addr()
            .expect("bound listener should have local addr");
        let status_line = status_line.to_owned();
        let body = body.to_owned();

        let server = tokio::spawn(async move {
            let (mut socket, _) = listener
                .accept()
                .await
                .expect("server should accept one connection");
            let mut request_buffer = vec![0_u8; 2048];
            let _ = socket.read(&mut request_buffer).await;

            let response = format!(
                "HTTP/1.1 {status_line}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                body.len(),
                body
            );

            socket
                .write_all(response.as_bytes())
                .await
                .expect("server should write response");
        });

        (format!("http://{address}"), server)
    }

    /// Starts a TCP server that serves one response per accepted connection.
    async fn spawn_sequence_response_server(
        responses: Vec<(String, String)>,
    ) -> (String, JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("test listener should bind");
        let address = listener
            .local_addr()
            .expect("bound listener should have local addr");

        let server = tokio::spawn(async move {
            for (status_line, body) in responses {
                let (mut socket, _) = listener
                    .accept()
                    .await
                    .expect("server should accept one connection per response");
                let mut request_buffer = vec![0_u8; 2048];
                let _ = socket.read(&mut request_buffer).await;

                let response = format!(
                    "HTTP/1.1 {status_line}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );

                socket
                    .write_all(response.as_bytes())
                    .await
                    .expect("server should write response");
            }
        });

        (format!("http://{address}"), server)
    }

    #[tokio::test]
    async fn qdrant_adapter_rejects_blank_endpoint() {
        let config = QdrantConfig {
            endpoint: " ".to_owned(),
            ..QdrantConfig::default()
        };

        let error = QdrantAdapter::new(reqwest::Client::new(), config)
            .expect_err("blank endpoint should fail config validation");

        assert!(matches!(error, QdrantError::InvalidConfiguration(_)));
    }

    /// Proves the collection-name charset guard rejects path-traversal characters.
    ///
    /// A `QDRANT_COLLECTION` override like `skills/../../admin` would be interpolated
    /// directly into Qdrant REST paths via `format!`.  The guard must catch this at
    /// construction, before any network I/O.
    #[test]
    fn qdrant_adapter_rejects_collection_name_with_path_traversal() {
        let dangerous_names = [
            "skills/../../admin",
            "skills/../secrets",
            "collections/evil",
            "a..b",
            "name with spaces",
            "name\nnewline",
            "name%2Fslash",
        ];

        for name in &dangerous_names {
            let config = QdrantConfig {
                collection_name: (*name).to_owned(),
                ..QdrantConfig::default()
            };
            let error = QdrantAdapter::new(reqwest::Client::new(), config).expect_err(&format!(
                "collection name {name:?} must be rejected by the charset guard"
            ));
            assert!(
                matches!(error, QdrantError::InvalidConfiguration(_)),
                "error must be InvalidConfiguration for name {name:?}, got: {error:?}"
            );
            let msg = error.to_string();
            assert!(
                msg.contains("collection name"),
                "error message must mention 'collection name' for input {name:?}, got: {msg}"
            );
        }
    }

    /// Proves the charset guard accepts well-formed collection names used in production.
    ///
    /// Model-keyed names like `skills__nomic-embed-text` and operator-supplied
    /// test-isolation names like `skills_ns_abc123` must pass without error.
    #[test]
    fn qdrant_adapter_accepts_valid_collection_names() {
        let valid_names = [
            "skills",
            "skills__nomic-embed-text",
            "skills__qwen3-embedding-4b",
            "skills_ns_abc123",
            "UNCONFIGURED__use_model_keyed_collection_name",
            "A1-z9_",
        ];

        for name in &valid_names {
            let config = QdrantConfig {
                collection_name: (*name).to_owned(),
                ..QdrantConfig::default()
            };
            QdrantAdapter::new(reqwest::Client::new(), config).unwrap_or_else(|_| {
                panic!("valid collection name {name:?} must be accepted by the charset guard")
            });
        }
    }

    #[tokio::test]
    async fn qdrant_adapter_checks_connectivity_against_ok_response() {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("test listener should bind");
        let address = listener
            .local_addr()
            .expect("bound listener should have local addr");

        let server = tokio::spawn(async move {
            let (mut socket, _) = listener
                .accept()
                .await
                .expect("server should accept one connection");
            let mut request_buffer = vec![0_u8; 1024];
            let _ = socket.read(&mut request_buffer).await;

            let body = r#"{"status":"ok","time":0.0,"result":{"collections":[]}}"#;
            let response = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                body.len(),
                body
            );

            socket
                .write_all(response.as_bytes())
                .await
                .expect("server should write response");
        });

        let adapter = QdrantAdapter::new(
            reqwest::Client::new(),
            QdrantConfig {
                endpoint: format!("http://{address}"),
                timeout_ms: 1_000,
                collection_name: "skills".to_owned(),
            },
        )
        .expect("test config should be valid");
        adapter
            .check_connectivity()
            .await
            .expect("mock server should satisfy connectivity contract");

        server.await.expect("mock server should complete");
    }

    #[tokio::test]
    async fn qdrant_adapter_surfaces_connection_failures() {
        let adapter = QdrantAdapter::new(
            reqwest::Client::new(),
            QdrantConfig {
                endpoint: "http://127.0.0.1:1".to_owned(),
                timeout_ms: 100,
                ..QdrantConfig::default()
            },
        )
        .expect("config should be valid");

        let error = adapter
            .check_connectivity()
            .await
            .expect_err("closed port should fail connectivity check");

        assert!(matches!(error, QdrantError::Connectivity(_)));
    }

    #[tokio::test]
    async fn qdrant_adapter_upsert_vector_treats_404_as_error() {
        let (endpoint, server) =
            spawn_single_response_server("404 Not Found", r#"{"status":"not_found"}"#).await;
        let adapter = QdrantAdapter::new(
            reqwest::Client::new(),
            QdrantConfig {
                endpoint,
                timeout_ms: 1_000,
                ..QdrantConfig::default()
            },
        )
        .expect("test config should be valid");

        let error = adapter
            .upsert_vector(42, &[0.1, 0.2], &json!({"name":"skill"}))
            .await
            .expect_err("upsert should fail when qdrant returns 404");

        assert!(
            error.contains("unexpected status 404"),
            "upsert error should surface 404 status, got: {error}"
        );

        server.await.expect("mock server should complete");
    }

    #[tokio::test]
    async fn qdrant_adapter_delete_points_treats_404_as_error() {
        let (endpoint, server) =
            spawn_single_response_server("404 Not Found", r#"{"status":"not_found"}"#).await;
        let adapter = QdrantAdapter::new(
            reqwest::Client::new(),
            QdrantConfig {
                endpoint,
                timeout_ms: 1_000,
                ..QdrantConfig::default()
            },
        )
        .expect("test config should be valid");

        let error = adapter
            .delete_points(&[1, 2, 3])
            .await
            .expect_err("delete should fail when qdrant returns 404");

        assert!(
            error.contains("unexpected status 404"),
            "delete error should surface 404 status, got: {error}"
        );

        server.await.expect("mock server should complete");
    }

    #[tokio::test]
    async fn qdrant_adapter_list_point_ids_treats_404_as_error() {
        let (endpoint, server) =
            spawn_single_response_server("404 Not Found", r#"{"status":"not_found"}"#).await;
        let adapter = QdrantAdapter::new(
            reqwest::Client::new(),
            QdrantConfig {
                endpoint,
                timeout_ms: 1_000,
                ..QdrantConfig::default()
            },
        )
        .expect("test config should be valid");

        let error = adapter
            .list_point_ids()
            .await
            .expect_err("list should fail when qdrant returns 404");

        assert!(
            error.contains("unexpected status 404"),
            "list error should surface 404 status, got: {error}"
        );

        server.await.expect("mock server should complete");
    }

    #[tokio::test]
    async fn qdrant_adapter_list_point_ids_paginates_until_complete() {
        let first_page =
            r#"{"status":"ok","result":{"points":[{"id":1},{"id":"2"}],"next_page_offset":2}}"#;
        let second_page =
            r#"{"status":"ok","result":{"points":[{"id":3}],"next_page_offset":null}}"#;
        let (endpoint, server) = spawn_sequence_response_server(vec![
            ("200 OK".to_owned(), first_page.to_owned()),
            ("200 OK".to_owned(), second_page.to_owned()),
        ])
        .await;
        let adapter = QdrantAdapter::new(
            reqwest::Client::new(),
            QdrantConfig {
                endpoint,
                timeout_ms: 1_000,
                ..QdrantConfig::default()
            },
        )
        .expect("test config should be valid");

        let listing = adapter
            .list_point_ids()
            .await
            .expect("list should paginate through every page");

        assert_eq!(listing.point_ids, vec![1, 2, 3]);
        assert!(listing.is_complete);

        server.await.expect("mock server should complete");
    }

    #[tokio::test]
    async fn qdrant_adapter_has_vector_returns_false_on_404() {
        let (endpoint, server) =
            spawn_single_response_server("404 Not Found", r#"{"status":"not_found"}"#).await;
        let adapter = QdrantAdapter::new(
            reqwest::Client::new(),
            QdrantConfig {
                endpoint,
                timeout_ms: 1_000,
                ..QdrantConfig::default()
            },
        )
        .expect("test config should be valid");

        let has_vector = adapter
            .has_vector(99)
            .await
            .expect("has_vector should map 404 to false");

        assert!(!has_vector);

        server.await.expect("mock server should complete");
    }

    /// Proves `ensure_collection` treats HTTP 409 Conflict as success.
    ///
    /// When `mcp-server` and `graph-builder` race to create the same collection
    /// on cold start, the losing caller receives 409 from Qdrant. The collection
    /// exists — that is the goal — so 409 must be a benign success (bug #157).
    #[tokio::test]
    async fn ensure_collection_treats_409_as_success() {
        // First request (GET probe) → 404 Not Found: collection does not exist yet.
        // Second request (PUT create) → 409 Conflict: a concurrent caller created it.
        let (endpoint, server) = spawn_sequence_response_server(vec![
            (
                "404 Not Found".to_owned(),
                r#"{"status":"not_found"}"#.to_owned(),
            ),
            (
                "409 Conflict".to_owned(),
                r#"{"status":"conflict","description":"already exists"}"#.to_owned(),
            ),
        ])
        .await;

        let adapter = QdrantAdapter::new(
            reqwest::Client::new(),
            QdrantConfig {
                endpoint,
                timeout_ms: 1_000,
                ..QdrantConfig::default()
            },
        )
        .expect("test config should be valid");

        adapter
            .ensure_collection("skills", 768)
            .await
            .expect("409 Conflict must be treated as success — collection already exists");

        server.await.expect("mock server should complete");
    }

    /// Proves `ensure_collection` skips creation when the collection already exists (200 on probe).
    #[tokio::test]
    async fn ensure_collection_is_noop_when_collection_exists() {
        // Only one request expected: GET probe returns 200.
        let (endpoint, server) =
            spawn_single_response_server("200 OK", r#"{"status":"ok","result":{"name":"skills"}}"#)
                .await;

        let adapter = QdrantAdapter::new(
            reqwest::Client::new(),
            QdrantConfig {
                endpoint,
                timeout_ms: 1_000,
                ..QdrantConfig::default()
            },
        )
        .expect("test config should be valid");

        adapter
            .ensure_collection("skills", 768)
            .await
            .expect("200 on probe must return Ok immediately without a create request");

        server.await.expect("mock server should complete");
    }

    /// Proves `model_keyed_collection_name` derives a safe collection name from a model string.
    #[test]
    fn model_keyed_collection_name_derives_safe_identifier_from_model() {
        assert_eq!(
            model_keyed_collection_name("nomic-embed-text").unwrap(),
            "skills__nomic-embed-text"
        );
        assert_eq!(
            model_keyed_collection_name("qwen3-embedding:4b").unwrap(),
            "skills__qwen3-embedding-4b"
        );
        assert_eq!(
            model_keyed_collection_name("some/model:latest").unwrap(),
            "skills__some-model-latest"
        );
    }

    /// Proves the two live V1.7 embedding arms slug to DISTINCT collections so
    /// an operator running both arms simultaneously cannot corrupt one with the
    /// other's vectors. This is the honesty invariant for the arm comparison.
    #[test]
    fn model_keyed_collection_name_live_arms_are_distinct() {
        let nomic = model_keyed_collection_name("nomic-embed-text")
            .expect("nomic-embed-text must produce a valid slug");
        let qwen = model_keyed_collection_name("qwen3-embedding:4b")
            .expect("qwen3-embedding:4b must produce a valid slug");

        assert_ne!(
            nomic, qwen,
            "live V1.7 arms must map to distinct collections; \
             collision would silently mix vectors from different models"
        );
        assert_eq!(nomic, "skills__nomic-embed-text");
        assert_eq!(qwen, "skills__qwen3-embedding-4b");
    }

    /// Proves that an empty model name fails loud rather than silently producing
    /// the bare `skills__` collection name, which would overlap with any other
    /// degenerate model name and corrupt the honest arm comparison.
    #[test]
    fn model_keyed_collection_name_empty_model_fails_loud() {
        let err = model_keyed_collection_name("")
            .expect_err("empty model name must fail loud, not produce skills__");
        assert!(
            matches!(err, QdrantError::InvalidConfiguration(_)),
            "error variant must be InvalidConfiguration, got: {err:?}"
        );
        let msg = err.to_string();
        assert!(
            msg.contains("empty collection slug"),
            "error message must mention empty collection slug, got: {msg}"
        );
    }

    /// Proves that a model name made entirely of non-alphanumeric characters (e.g.
    /// `"/:@#"`) also fails loud — same root cause as the empty-string case.
    #[test]
    fn model_keyed_collection_name_all_symbol_model_fails_loud() {
        let err = model_keyed_collection_name("/:@#")
            .expect_err("all-symbol model name must fail loud, not produce skills__");
        assert!(
            matches!(err, QdrantError::InvalidConfiguration(_)),
            "error variant must be InvalidConfiguration, got: {err:?}"
        );
    }

    /// Proves `ensure_collection` returns `Ok(())` — not an error — when the HTTP 200
    /// response body cannot be parsed as JSON.
    ///
    /// A network/framing error after the headers (e.g. truncated body) must NOT be
    /// silently routed to the "missing size field" warn-and-continue branch as if the
    /// field were merely absent in an older Qdrant. Both code paths result in `Ok(())`,
    /// but the `unwrap_or_else` variant logs the real parse error separately, allowing
    /// an operator to distinguish a transient failure from a genuinely missing field.
    #[tokio::test]
    async fn ensure_collection_200_with_invalid_json_body_returns_ok_not_error() {
        // The response body is deliberately not valid JSON — simulates a truncated
        // or corrupted response body after a 200 status.
        let (endpoint, server) =
            spawn_single_response_server("200 OK", "this is not json at all {{").await;

        let adapter = QdrantAdapter::new(
            reqwest::Client::new(),
            QdrantConfig {
                endpoint,
                timeout_ms: 1_000,
                ..QdrantConfig::default()
            },
        )
        .expect("test config should be valid");

        // Should return Ok — the parse failure is logged but not propagated as an error.
        adapter
            .ensure_collection("skills", 768)
            .await
            .expect("a 200 response with unparseable body must return Ok, not fail");

        server.await.expect("mock server should complete");
    }

    /// Proves that the dimension guard does NOT run on the 409 cold-start path.
    ///
    /// When two callers race to create the collection, the loser receives a 409 on
    /// the PUT. Since the GET probe returned 404 (not 200), no dimension check was
    /// performed, and the 409 is treated as success. This is correct: the creator
    /// (winner) already validated the collection on its own GET→200 or fresh create.
    ///
    /// This test confirms there is no regression to the #157 fix: 409 on PUT is
    /// benign success; dimension guard is gated on GET→200 only, never on PUT→409.
    #[tokio::test]
    async fn ensure_collection_409_create_does_not_run_dimension_check() {
        // GET probe → 404: collection does not exist yet (no dimension info available).
        // PUT create → 409: a concurrent caller already created it.
        // No dimension check should happen because the GET returned 404, not 200.
        // (Caller expects 2560 but we pass 768 here — if the check ran, it would fail;
        //  the test proves it does NOT run on the PUT→409 path.)
        let (endpoint, server) = spawn_sequence_response_server(vec![
            (
                "404 Not Found".to_owned(),
                r#"{"status":"not_found"}"#.to_owned(),
            ),
            (
                "409 Conflict".to_owned(),
                r#"{"status":"conflict","description":"already exists"}"#.to_owned(),
            ),
        ])
        .await;

        let adapter = QdrantAdapter::new(
            reqwest::Client::new(),
            QdrantConfig {
                endpoint,
                timeout_ms: 1_000,
                ..QdrantConfig::default()
            },
        )
        .expect("test config should be valid");

        // Caller expects 2560 — but no dimension check runs because the probe returned 404,
        // so there was no existing collection info to compare against.
        adapter.ensure_collection("skills", 2560).await.expect(
            "409 on PUT must be treated as success regardless of expected dimension; \
                     dimension guard only runs on GET→200",
        );

        server.await.expect("mock server should complete");
    }

    /// Proves `ensure_collection` fails loud when the existing collection has a
    /// different dimension than the caller expects.
    #[tokio::test]
    async fn ensure_collection_fails_loud_on_dimension_mismatch() {
        // GET probe returns 200 with a 768-dim collection; caller expects 2560.
        let body = r#"{"status":"ok","result":{"config":{"params":{"vectors":{"size":768,"distance":"Cosine"}}}}}"#;
        let (endpoint, server) = spawn_single_response_server("200 OK", body).await;

        let adapter = QdrantAdapter::new(
            reqwest::Client::new(),
            QdrantConfig {
                endpoint,
                timeout_ms: 1_000,
                ..QdrantConfig::default()
            },
        )
        .expect("test config should be valid");

        let error = adapter.ensure_collection("skills", 2560).await.expect_err(
            "dimension mismatch must fail loud, not silently reuse the wrong collection",
        );

        assert!(
            matches!(error, QdrantError::DimensionMismatch { .. }),
            "error variant must be DimensionMismatch, got: {error:?}"
        );

        server.await.expect("mock server should complete");
    }

    /// Proves `ensure_collection` succeeds when the existing collection's dimension
    /// matches the caller's expected dimension (happy path after the guard).
    #[tokio::test]
    async fn ensure_collection_succeeds_when_existing_dimension_matches() {
        let body = r#"{"status":"ok","result":{"config":{"params":{"vectors":{"size":768,"distance":"Cosine"}}}}}"#;
        let (endpoint, server) = spawn_single_response_server("200 OK", body).await;

        let adapter = QdrantAdapter::new(
            reqwest::Client::new(),
            QdrantConfig {
                endpoint,
                timeout_ms: 1_000,
                ..QdrantConfig::default()
            },
        )
        .expect("test config should be valid");

        adapter
            .ensure_collection("skills", 768)
            .await
            .expect("matching dimension must succeed");

        server.await.expect("mock server should complete");
    }

    // ---- Hybrid-adapter helpers and tests ----------------------------------------

    /// A mock TCP server that captures the raw request bytes before responding.
    ///
    /// Returns the captured request as a `String` alongside the server handle so
    /// tests can assert on the exact body the adapter sent.
    async fn spawn_capturing_response_server(
        status_line: &str,
        response_body: &str,
    ) -> (String, JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("test listener should bind");
        let address = listener
            .local_addr()
            .expect("bound listener should have local addr");
        let status_line = status_line.to_owned();
        let response_body = response_body.to_owned();

        let server = tokio::spawn(async move {
            let (mut socket, _) = listener
                .accept()
                .await
                .expect("server should accept one connection");
            // Use a larger buffer so we capture realistic JSON bodies.
            let mut request_buffer = vec![0_u8; 16_384];
            let bytes_read = socket
                .read(&mut request_buffer)
                .await
                .expect("server should read request");
            let captured = String::from_utf8_lossy(&request_buffer[..bytes_read]).into_owned();

            let response = format!(
                "HTTP/1.1 {status_line}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                response_body.len(),
                response_body
            );
            socket
                .write_all(response.as_bytes())
                .await
                .expect("server should write response");
            captured
        });

        (format!("http://{address}"), server)
    }

    /// Like `spawn_sequence_response_server` but captures each request body.
    async fn spawn_capturing_sequence_server(
        responses: Vec<(String, String)>,
    ) -> (String, JoinHandle<Vec<String>>) {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("test listener should bind");
        let address = listener
            .local_addr()
            .expect("bound listener should have local addr");

        let server = tokio::spawn(async move {
            let mut captured_requests = Vec::new();
            for (status_line, body) in responses {
                let (mut socket, _) = listener
                    .accept()
                    .await
                    .expect("server should accept one connection per response");
                let mut request_buffer = vec![0_u8; 16_384];
                let bytes_read = socket
                    .read(&mut request_buffer)
                    .await
                    .expect("server should read request");
                let captured = String::from_utf8_lossy(&request_buffer[..bytes_read]).into_owned();
                captured_requests.push(captured);

                let response = format!(
                    "HTTP/1.1 {status_line}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                socket
                    .write_all(response.as_bytes())
                    .await
                    .expect("server should write response");
            }
            captured_requests
        });

        (format!("http://{address}"), server)
    }

    // ---- model_keyed_hybrid_collection_name tests --------------------------------

    /// Proves the hybrid name is the dense name with a `__hybrid` suffix.
    #[test]
    fn hybrid_collection_name_appends_hybrid_suffix() {
        assert_eq!(
            model_keyed_hybrid_collection_name("nomic-embed-text").unwrap(),
            "skills__nomic-embed-text__hybrid"
        );
        assert_eq!(
            model_keyed_hybrid_collection_name("qwen3-embedding:4b").unwrap(),
            "skills__qwen3-embedding-4b__hybrid"
        );
    }

    /// Proves the hybrid name stays within the `^[A-Za-z0-9_-]+$` charset so it
    /// passes `QdrantAdapter::new`'s collection-name charset guard.
    #[test]
    fn hybrid_collection_name_passes_charset_guard() {
        let name = model_keyed_hybrid_collection_name("nomic-embed-text")
            .expect("nomic-embed-text must produce a valid hybrid name");
        assert!(
            name.chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-'),
            "hybrid collection name must consist only of [A-Za-z0-9_-], got: {name}"
        );
    }

    /// Proves that the hybrid name derivation propagates errors from the base
    /// derivation (empty or all-symbol model names).
    #[test]
    fn hybrid_collection_name_fails_loud_on_empty_model() {
        let err = model_keyed_hybrid_collection_name("")
            .expect_err("empty model name must fail loud for hybrid derivation too");
        assert!(
            matches!(err, QdrantError::InvalidConfiguration(_)),
            "error must be InvalidConfiguration, got: {err:?}"
        );
    }

    /// Proves the dense and hybrid collections are distinct for each V1.7 arm
    /// so they can coexist without schema collision.
    #[test]
    fn hybrid_and_dense_collection_names_are_distinct_per_arm() {
        for model in &["nomic-embed-text", "qwen3-embedding:4b"] {
            let dense = model_keyed_collection_name(model).unwrap();
            let hybrid = model_keyed_hybrid_collection_name(model).unwrap();
            assert_ne!(
                dense, hybrid,
                "dense and hybrid collections for {model} must be distinct; \
                 they use incompatible named-vector schemas"
            );
            assert!(
                hybrid.ends_with("__hybrid"),
                "hybrid name must end with __hybrid, got: {hybrid}"
            );
        }
    }

    // ---- ensure_hybrid_collection mock tests -------------------------------------

    /// Proves `ensure_hybrid_collection` sends the named dense+sparse(idf) body
    /// on the PUT create request.
    ///
    /// The request JSON must contain `"sparse_vectors"` with `"modifier":"idf"`
    /// and `"vectors"` with a named `"dense"` key — NOT the unnamed `"size"` form
    /// used by `ensure_collection`.
    #[tokio::test]
    async fn ensure_hybrid_collection_sends_named_dense_and_sparse_idf_body() {
        // GET probe → 404: collection does not exist.
        // PUT create → 200 OK.
        let (endpoint, server) = spawn_capturing_sequence_server(vec![
            (
                "404 Not Found".to_owned(),
                r#"{"status":"not_found"}"#.to_owned(),
            ),
            (
                "200 OK".to_owned(),
                r#"{"status":"ok","result":true,"time":0.001}"#.to_owned(),
            ),
        ])
        .await;

        let adapter = QdrantAdapter::new(
            reqwest::Client::new(),
            QdrantConfig {
                endpoint,
                timeout_ms: 1_000,
                ..QdrantConfig::default()
            },
        )
        .expect("test config should be valid");

        adapter
            .ensure_hybrid_collection("skills__nomic-embed-text__hybrid", 768)
            .await
            .expect("ensure_hybrid_collection must succeed on 200 OK");

        let captured_requests = server.await.expect("mock server should complete");
        // The second captured request is the PUT create body.
        let create_request = &captured_requests[1];

        // Extract JSON body (everything after the double-CRLF header/body separator).
        let body_start = create_request
            .find("\r\n\r\n")
            .expect("request must contain header/body separator")
            + 4;
        let body_json: Value = serde_json::from_str(&create_request[body_start..])
            .expect("create body must be valid JSON");

        assert!(
            body_json.get("sparse_vectors").is_some(),
            "create body must contain sparse_vectors key; got: {body_json}"
        );
        assert_eq!(
            body_json.pointer("/sparse_vectors/sparse/modifier"),
            Some(&Value::String("idf".to_owned())),
            "sparse_vectors.sparse.modifier must be 'idf'; got: {body_json}"
        );
        assert!(
            body_json.pointer("/vectors/dense").is_some(),
            "create body must contain named vectors.dense key; got: {body_json}"
        );
        assert_eq!(
            body_json.pointer("/vectors/dense/size"),
            Some(&Value::Number(768.into())),
            "vectors.dense.size must be 768; got: {body_json}"
        );
        assert_eq!(
            body_json.pointer("/vectors/dense/distance"),
            Some(&Value::String("Cosine".to_owned())),
            "vectors.dense.distance must be Cosine; got: {body_json}"
        );
    }

    /// Proves `ensure_hybrid_collection` is idempotent when the collection
    /// already exists (GET probe → 200).
    #[tokio::test]
    async fn ensure_hybrid_collection_is_noop_when_collection_exists() {
        // Only a GET probe is expected; if a second request arrives the server panics.
        let (endpoint, server) = spawn_single_response_server(
            "200 OK",
            r#"{"status":"ok","result":{"name":"skills__nomic-embed-text__hybrid"}}"#,
        )
        .await;

        let adapter = QdrantAdapter::new(
            reqwest::Client::new(),
            QdrantConfig {
                endpoint,
                timeout_ms: 1_000,
                ..QdrantConfig::default()
            },
        )
        .expect("test config should be valid");

        adapter
            .ensure_hybrid_collection("skills__nomic-embed-text__hybrid", 768)
            .await
            .expect("200 on probe must return Ok immediately without a create request");

        server.await.expect("mock server should complete");
    }

    /// Proves `ensure_hybrid_collection` treats 409 Conflict on the PUT as success.
    #[tokio::test]
    async fn ensure_hybrid_collection_treats_409_as_success() {
        let (endpoint, server) = spawn_sequence_response_server(vec![
            (
                "404 Not Found".to_owned(),
                r#"{"status":"not_found"}"#.to_owned(),
            ),
            (
                "409 Conflict".to_owned(),
                r#"{"status":"conflict","description":"already exists"}"#.to_owned(),
            ),
        ])
        .await;

        let adapter = QdrantAdapter::new(
            reqwest::Client::new(),
            QdrantConfig {
                endpoint,
                timeout_ms: 1_000,
                ..QdrantConfig::default()
            },
        )
        .expect("test config should be valid");

        adapter
            .ensure_hybrid_collection("skills__nomic-embed-text__hybrid", 768)
            .await
            .expect("409 Conflict must be treated as success");

        server.await.expect("mock server should complete");
    }

    // ---- upsert_hybrid_point mock tests -----------------------------------------

    /// Proves `upsert_hybrid_point` sends a named-vector body with both `dense`
    /// and `sparse` (indices/values) fields.
    #[tokio::test]
    async fn upsert_hybrid_point_sends_named_vector_body() {
        let (endpoint, server) = spawn_capturing_response_server(
            "200 OK",
            r#"{"status":"ok","result":{"operation_id":0,"status":"completed"},"time":0.001}"#,
        )
        .await;

        let adapter = QdrantAdapter::new(
            reqwest::Client::new(),
            QdrantConfig {
                endpoint,
                timeout_ms: 1_000,
                ..QdrantConfig::default()
            },
        )
        .expect("test config should be valid");

        let sparse = SparseVector {
            indices: vec![5, 42, 100],
            values: vec![0.3, 0.7, 0.1],
        };
        adapter
            .upsert_hybrid_point(
                "skills__nomic-embed-text__hybrid",
                7,
                &[0.1_f32, 0.2, 0.3],
                &sparse,
                &json!({"name": "test-skill"}),
            )
            .await
            .expect("upsert_hybrid_point must succeed on 200 OK");

        let captured = server.await.expect("mock server should complete");
        let body_start = captured
            .find("\r\n\r\n")
            .expect("request must contain header/body separator")
            + 4;
        let body_json: Value =
            serde_json::from_str(&captured[body_start..]).expect("upsert body must be valid JSON");

        let point = body_json
            .pointer("/points/0")
            .expect("body must contain points[0]");
        assert_eq!(
            point.get("id"),
            Some(&Value::Number(7.into())),
            "point id must be 7; got: {point}"
        );
        assert!(
            point.pointer("/vector/dense").is_some(),
            "point must have vector.dense; got: {point}"
        );
        assert_eq!(
            point.pointer("/vector/sparse/indices"),
            Some(&json!([5_u32, 42_u32, 100_u32])),
            "point must have vector.sparse.indices [5,42,100]; got: {point}"
        );
        assert_eq!(
            point.pointer("/vector/sparse/values"),
            Some(&json!([0.3_f32, 0.7_f32, 0.1_f32])),
            "point must have vector.sparse.values [0.3,0.7,0.1]; got: {point}"
        );
    }

    // ---- query_hybrid mock tests -------------------------------------------------

    /// Proves `query_hybrid` sends a prefetch+fusion body and parses the result
    /// points into `HybridHit` structs.
    #[tokio::test]
    async fn query_hybrid_sends_prefetch_fusion_body_and_parses_hits() {
        let response_body = r#"{"status":"ok","result":{"points":[
            {"id":3,"score":0.95,"payload":{"name":"skill-c"}},
            {"id":1,"score":0.80,"payload":{"name":"skill-a"}}
        ]},"time":0.002}"#;
        let (endpoint, server) = spawn_capturing_response_server("200 OK", response_body).await;

        let adapter = QdrantAdapter::new(
            reqwest::Client::new(),
            QdrantConfig {
                endpoint,
                timeout_ms: 1_000,
                ..QdrantConfig::default()
            },
        )
        .expect("test config should be valid");

        let sparse = SparseVector {
            indices: vec![10, 20],
            values: vec![0.5, 0.5],
        };
        let hits = adapter
            .query_hybrid(
                "skills__nomic-embed-text__hybrid",
                &[0.1_f32, 0.2, 0.3],
                &sparse,
                5,
            )
            .await
            .expect("query_hybrid must succeed on 200 OK");

        assert_eq!(hits.len(), 2, "must return 2 hits; got: {hits:?}");
        assert_eq!(
            hits[0].point_id, 3,
            "first hit must be point 3 (highest RRF score)"
        );
        assert_eq!(hits[1].point_id, 1, "second hit must be point 1");
        assert!(
            (hits[0].score - 0.95).abs() < 1e-5,
            "first hit score must be ~0.95; got: {}",
            hits[0].score
        );

        let captured = server.await.expect("mock server should complete");
        let body_start = captured
            .find("\r\n\r\n")
            .expect("request must contain header/body separator")
            + 4;
        let body_json: Value =
            serde_json::from_str(&captured[body_start..]).expect("query body must be valid JSON");

        // Verify prefetch structure.
        let prefetch = body_json
            .get("prefetch")
            .and_then(Value::as_array)
            .expect("body must contain prefetch array");
        assert_eq!(prefetch.len(), 2, "prefetch must have exactly 2 arms");

        let dense_arm = &prefetch[0];
        assert_eq!(
            dense_arm.get("using"),
            Some(&Value::String("dense".to_owned())),
            "first prefetch arm must use 'dense'; got: {dense_arm}"
        );
        let sparse_arm = &prefetch[1];
        assert_eq!(
            sparse_arm.get("using"),
            Some(&Value::String("sparse".to_owned())),
            "second prefetch arm must use 'sparse'; got: {sparse_arm}"
        );

        // Verify fusion step.
        assert_eq!(
            body_json.pointer("/query/fusion"),
            Some(&Value::String("rrf".to_owned())),
            "query.fusion must be 'rrf'; got: {body_json}"
        );
        assert!(
            body_json.get("with_payload") == Some(&Value::Bool(true)),
            "body must include with_payload:true; got: {body_json}"
        );
    }

    /// Proves `query_hybrid` returns an empty vec when the result contains no points.
    #[tokio::test]
    async fn query_hybrid_returns_empty_vec_on_no_results() {
        let response_body = r#"{"status":"ok","result":{"points":[]},"time":0.001}"#;
        let (endpoint, server) = spawn_single_response_server("200 OK", response_body).await;

        let adapter = QdrantAdapter::new(
            reqwest::Client::new(),
            QdrantConfig {
                endpoint,
                timeout_ms: 1_000,
                ..QdrantConfig::default()
            },
        )
        .expect("test config should be valid");

        let sparse = SparseVector {
            indices: vec![],
            values: vec![],
        };
        let hits = adapter
            .query_hybrid("skills__nomic-embed-text__hybrid", &[0.0_f32], &sparse, 5)
            .await
            .expect("query_hybrid must succeed on empty result");

        assert!(hits.is_empty(), "empty result must yield empty hit vec");
        server.await.expect("mock server should complete");
    }

    // ---- live tests (require a running Qdrant at QDRANT_URL) ---------------------

    fn live_qdrant_url() -> String {
        std::env::var("QDRANT_URL").unwrap_or_else(|_| "http://127.0.0.1:6333".to_owned())
    }

    /// Creates an adapter pointed at the live Qdrant, using the provided
    /// collection name so live tests do not interfere with production collections.
    fn live_adapter(collection_name: &str) -> QdrantAdapter {
        QdrantAdapter::new(
            reqwest::Client::new(),
            QdrantConfig {
                endpoint: live_qdrant_url(),
                timeout_ms: 5_000,
                collection_name: collection_name.to_owned(),
            },
        )
        .expect("live adapter config must be valid")
    }

    /// Live: proves `ensure_hybrid_collection` creates a named dense+sparse(idf)
    /// collection and that a subsequent GET on the collection info shows both
    /// vector config sections.
    ///
    /// Requires Qdrant at `QDRANT_URL` (default :6333). Run with:
    /// ```
    /// QDRANT_URL=http://127.0.0.1:16333 cargo test -p infrastructure \
    ///   --features test-utils live_ensure_hybrid_collection_creates_named_collection \
    ///   -- --ignored --nocapture
    /// ```
    #[tokio::test]
    #[ignore = "requires live qdrant"]
    async fn live_ensure_hybrid_collection_creates_named_collection() {
        // Uses a test-specific name so this test does not collide with sibling
        // live tests that may run in parallel under the same process.
        let collection = "skills__test-ensure__hybrid";
        let adapter = live_adapter(collection);

        // Clean up from any previous run so we exercise the create path.
        let _ = reqwest::Client::new()
            .delete(format!(
                "{}/collections/{collection}",
                live_qdrant_url().trim_end_matches('/')
            ))
            .send()
            .await;

        adapter
            .ensure_hybrid_collection(collection, 4)
            .await
            .expect("ensure_hybrid_collection must succeed against live Qdrant");

        // Verify Qdrant shows both vector config sections.
        let info: Value = reqwest::Client::new()
            .get(format!(
                "{}/collections/{collection}",
                live_qdrant_url().trim_end_matches('/')
            ))
            .send()
            .await
            .expect("GET collection must succeed")
            .json()
            .await
            .expect("collection info must parse as JSON");

        assert!(
            info.pointer("/result/config/params/vectors/dense")
                .is_some()
                || info.pointer("/result/config/params/vectors").is_some(),
            "collection info must contain named dense vector config; got: {info}"
        );
        assert!(
            info.pointer("/result/config/params/sparse_vectors/sparse")
                .is_some(),
            "collection info must contain sparse_vectors.sparse config; got: {info}"
        );

        // Idempotency: second call must also succeed (200 on probe).
        adapter
            .ensure_hybrid_collection(collection, 4)
            .await
            .expect("second ensure_hybrid_collection call must be idempotent");

        // Clean up.
        let _ = reqwest::Client::new()
            .delete(format!(
                "{}/collections/{collection}",
                live_qdrant_url().trim_end_matches('/')
            ))
            .send()
            .await;
    }

    /// Live: proves `upsert_hybrid_point` stores a dense+sparse point and that
    /// the point is retrievable afterward.
    ///
    /// Requires Qdrant at `QDRANT_URL`. Run with:
    /// ```
    /// QDRANT_URL=http://127.0.0.1:16333 cargo test -p infrastructure \
    ///   --features test-utils live_upsert_hybrid_point_stores_and_retrieves_point \
    ///   -- --ignored --nocapture
    /// ```
    #[tokio::test]
    #[ignore = "requires live qdrant"]
    async fn live_upsert_hybrid_point_stores_and_retrieves_point() {
        // Uses a test-specific name to avoid parallel-execution collisions.
        let collection = "skills__test-upsert__hybrid";
        let adapter = live_adapter(collection);
        let http = reqwest::Client::new();
        let base_url = live_qdrant_url();
        let base_url = base_url.trim_end_matches('/');

        // Ensure the collection exists.
        let _ = http
            .delete(format!("{base_url}/collections/{collection}"))
            .send()
            .await;
        adapter
            .ensure_hybrid_collection(collection, 4)
            .await
            .expect("collection must be created before upsert");

        let sparse = SparseVector {
            indices: vec![5, 42],
            values: vec![0.8, 0.3],
        };
        adapter
            .upsert_hybrid_point(
                collection,
                101,
                &[0.1_f32, 0.2, 0.3, 0.4],
                &sparse,
                &json!({"name": "live-test-skill", "source": "c1-unit-test"}),
            )
            .await
            .expect("upsert_hybrid_point must succeed against live Qdrant");

        // Retrieve the point and verify it exists.
        let point_info: Value = http
            .get(format!("{base_url}/collections/{collection}/points/101"))
            .send()
            .await
            .expect("GET point must succeed")
            .json()
            .await
            .expect("point info must parse as JSON");

        assert_eq!(
            point_info.pointer("/result/id"),
            Some(&Value::Number(101.into())),
            "retrieved point must have id=101; got: {point_info}"
        );
        assert_eq!(
            point_info.pointer("/result/payload/name"),
            Some(&Value::String("live-test-skill".to_owned())),
            "retrieved point must have payload.name='live-test-skill'; got: {point_info}"
        );

        println!("live upsert point info: {point_info:#}");

        // Clean up.
        let _ = http
            .delete(format!("{base_url}/collections/{collection}"))
            .send()
            .await;
    }

    /// Live: proves `query_hybrid` returns fused hits from the Query API, with
    /// correct ordering by RRF score.
    ///
    /// Upserts 3 points with known dense/sparse vectors, then issues a query
    /// designed to favor point 2. Asserts point 2 appears first.
    ///
    /// Requires Qdrant at `QDRANT_URL`. Run with:
    /// ```
    /// QDRANT_URL=http://127.0.0.1:16333 cargo test -p infrastructure \
    ///   --features test-utils live_query_hybrid_returns_fused_hits_ordered_by_rrf \
    ///   -- --ignored --nocapture
    /// ```
    #[tokio::test]
    #[ignore = "requires live qdrant"]
    async fn live_query_hybrid_returns_fused_hits_ordered_by_rrf() {
        // Uses a test-specific name to avoid parallel-execution collisions.
        let collection = "skills__test-query__hybrid";
        let adapter = live_adapter(collection);
        let http = reqwest::Client::new();
        let base_url = live_qdrant_url();
        let base_url = base_url.trim_end_matches('/');

        // Create fresh collection.
        let _ = http
            .delete(format!("{base_url}/collections/{collection}"))
            .send()
            .await;
        adapter
            .ensure_hybrid_collection(collection, 4)
            .await
            .expect("collection must be created before query");

        // Upsert 3 points with different dense and sparse vectors.
        // Point 2 is designed to score best under the test query.
        #[allow(clippy::type_complexity)]
        let point_configs: &[(u64, [f32; 4], Vec<u32>, Vec<f32>, &str)] = &[
            (1, [0.1, 0.0, 0.0, 0.0], vec![1], vec![0.1], "skill-a"),
            (
                2,
                [0.9, 0.9, 0.9, 0.9],
                vec![5, 42],
                vec![0.9, 0.9],
                "skill-b",
            ),
            (3, [0.0, 0.1, 0.0, 0.0], vec![99], vec![0.1], "skill-c"),
        ];
        for (id, dense, indices, values, name) in point_configs {
            let sparse = SparseVector {
                indices: indices.clone(),
                values: values.clone(),
            };
            adapter
                .upsert_hybrid_point(collection, *id, dense, &sparse, &json!({"name": name}))
                .await
                .unwrap_or_else(|e| panic!("upsert point {id} failed: {e}"));
        }

        // Query with a vector strongly aligned to point 2.
        let query_dense = [0.9_f32, 0.9, 0.9, 0.9];
        let query_sparse = SparseVector {
            indices: vec![5, 42],
            values: vec![0.9, 0.9],
        };
        let hits = adapter
            .query_hybrid(collection, &query_dense, &query_sparse, 3)
            .await
            .expect("query_hybrid must succeed against live Qdrant");

        println!("live query_hybrid hits: {hits:?}");

        assert!(
            !hits.is_empty(),
            "query must return at least one hit; got empty vec"
        );
        assert_eq!(
            hits[0].point_id, 2,
            "point 2 must rank first (highest alignment to query); got hits: {hits:?}"
        );
        assert!(
            hits.len() <= 3,
            "result count must not exceed limit=3; got {} hits",
            hits.len()
        );

        // Clean up.
        let _ = http
            .delete(format!("{base_url}/collections/{collection}"))
            .send()
            .await;
    }

    /// Proves the `QdrantConfig` default endpoint uses the REST port (6333),
    /// NOT the gRPC port (6334). Using the gRPC port for REST requests causes
    /// `hyper::Parse(Version)` errors in `check_connectivity`.
    ///
    /// This is a named deletion guard: if the default endpoint is ever changed
    /// to `:6334` (gRPC), this test will catch the regression immediately.
    #[test]
    fn qdrant_config_default_endpoint_uses_rest_port_not_grpc() {
        let config = QdrantConfig::default();
        assert!(
            config.endpoint.contains(":6333"),
            "QdrantConfig default endpoint must use REST port 6333, got: {}",
            config.endpoint
        );
        assert!(
            !config.endpoint.contains(":6334"),
            "QdrantConfig default endpoint must NOT use gRPC port 6334, got: {}",
            config.endpoint
        );
    }
}
