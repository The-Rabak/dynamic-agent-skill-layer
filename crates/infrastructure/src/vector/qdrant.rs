use std::time::Duration;

use reqwest::StatusCode;
use serde::Deserialize;
use serde_json::{Value, json};
use thiserror::Error;
use tokio::time::timeout;

use crate::persistence::outbox::{OutboxVectorStore, VectorPointListing};

#[derive(Debug, Clone)]
pub struct QdrantConfig {
    pub endpoint: String,
    pub timeout_ms: u64,
}

impl Default for QdrantConfig {
    fn default() -> Self {
        Self {
            endpoint: "http://127.0.0.1:6333".to_owned(),
            timeout_ms: 500,
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
}

#[derive(Debug, Clone)]
pub struct QdrantAdapter {
    client: reqwest::Client,
    config: QdrantConfig,
}

impl QdrantAdapter {
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
        let endpoint = format!(
            "{}/collections/skills/points?wait=true",
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
        let endpoint = format!(
            "{}/collections/skills/points/{}",
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
        let endpoint = format!(
            "{}/collections/skills/points/scroll",
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
        let endpoint = format!(
            "{}/collections/skills/points/delete?wait=true",
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
        let mut config = QdrantConfig::default();
        config.endpoint = " ".to_owned();

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
        let first_page = r#"{"status":"ok","result":{"points":[{"id":1},{"id":"2"}],"next_page_offset":2}}"#;
        let second_page = r#"{"status":"ok","result":{"points":[{"id":3}],"next_page_offset":null}}"#;
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
}
