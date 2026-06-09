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
/// Examples:
///   `"nomic-embed-text"`   → `"skills__nomic-embed-text"`
///   `"qwen3-embedding:4b"` → `"skills__qwen3-embedding-4b"`
///   `"some/model:latest"`  → `"skills__some-model-latest"`
pub fn model_keyed_collection_name(model: &str) -> String {
    let slug: String = model
        .to_ascii_lowercase()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c
            } else {
                '-'
            }
        })
        .collect::<String>()
        .split('-')
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    format!("skills__{slug}")
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
            collection_name: "skills".to_owned(),
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

        if config.timeout_ms == 0 {
            return Err(QdrantError::InvalidConfiguration(
                "timeout_ms must be greater than zero".to_owned(),
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
            let body: Value = response.json().await.unwrap_or(Value::Null);
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
                    // Log a warning and continue rather than blocking boot on a
                    // missing field from an older Qdrant version.
                    tracing::warn!(
                        collection_name,
                        expected_dimension = vector_size,
                        "could not parse collection dimension from Qdrant GET response; \
                         skipping dimension guard (upgrade Qdrant if this recurs)"
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
        self.expect_ok_status(response)
            .await
            .map_err(|error| error.to_string())?;
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
            model_keyed_collection_name("nomic-embed-text"),
            "skills__nomic-embed-text"
        );
        assert_eq!(
            model_keyed_collection_name("qwen3-embedding:4b"),
            "skills__qwen3-embedding-4b"
        );
        assert_eq!(
            model_keyed_collection_name("some/model:latest"),
            "skills__some-model-latest"
        );
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
