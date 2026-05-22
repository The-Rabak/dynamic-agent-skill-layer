use std::time::Duration;

use reqwest::StatusCode;
use serde::Deserialize;
use thiserror::Error;
use tokio::time::timeout;

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
}

#[derive(Debug, Deserialize)]
struct QdrantCollectionsResponse {
    status: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
    };

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
}
