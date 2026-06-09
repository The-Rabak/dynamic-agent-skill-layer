/// HTTP client for the real containerized `mcp-server` at `http://127.0.0.1:3001`.
///
/// All methods drive the **running container** over JSON-RPC 2.0 (`POST /mcp`)
/// or the REST endpoints (`GET /health`, `POST /ingest/transcript`).
///
/// # Transport
/// The app supports HTTP only — there is no stdio transport in production.
/// (`main.rs` always calls `serve_http`.)
///
/// # Secrets
/// The `TRANSCRIPT_INGEST_SECRET` travels as the `X-Ingest-Secret` header,
/// never in process arguments.
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::stack::MCP_SERVER_URL;

/// A JSON-RPC 2.0 response envelope returned by `POST /mcp`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    pub id: Value,
    /// Present when the call succeeded; absent on error.
    pub result: Option<Value>,
    /// Present when the call failed at the JSON-RPC layer.
    pub error: Option<Value>,
}

/// The structured fields returned by a `compile_context` tool call.
///
/// Mirrors `CompileContextResponse` in `crates/mcp-server/src/tools/compile_context.rs`
/// without creating a compile-time dependency on the production crate.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompileContextResponse {
    pub status: String,
    pub reason_code: Option<String>,
    pub additional_context: Option<String>,
    pub health: std::collections::BTreeMap<String, String>,
    pub scopes_considered: Vec<String>,
    pub graph_version: i64,
    pub latency_ms: u64,
    pub source: String,
}

/// Arguments for a `compile_context` JSON-RPC call.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompileContextArgs {
    pub prompt: String,
    pub session_id: String,
    pub repo_path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trigger: Option<String>,
}

/// Arguments for an `extract_session` JSON-RPC call.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractSessionArgs {
    pub transcript_ref: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transcript_inline: Option<String>,
    pub session_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repo_path: Option<String>,
}

/// Response returned by `extract_session`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractSessionResponse {
    pub status: String,
    pub reason_code: Option<String>,
    pub job_id: Option<String>,
    pub provider: Option<String>,
}

/// Body for `POST /ingest/transcript`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IngestTranscriptBody {
    pub session_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repo_path: Option<String>,
    pub source: String,
    pub content: String,
}

/// HTTP client that drives the real `mcp-server` container.
///
/// All requests target `http://127.0.0.1:3001`.  The client uses a generous
/// timeout (30 s) because live Ollama embedding calls can be slow on CPU-only
/// hosts.
pub struct McpClient {
    http: reqwest::Client,
    base_url: String,
}

impl McpClient {
    /// Creates a client pointed at the default host-mapped `mcp-server` address.
    pub fn new() -> Self {
        Self::with_base_url(MCP_SERVER_URL.to_owned())
    }

    /// Creates a client pointed at `base_url` (useful for overrides in tests).
    pub fn with_base_url(base_url: String) -> Self {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .expect("reqwest client should build");
        Self { http, base_url }
    }

    /// Sends an arbitrary MCP tool call and returns the raw JSON-RPC response.
    ///
    /// `name` is the tool name (e.g. `"compile_context"`).
    /// `args` is any JSON-serializable value that becomes `params.arguments`.
    pub async fn call_tool(
        &self,
        name: &str,
        args: impl Serialize,
    ) -> Result<JsonRpcResponse, String> {
        let body = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/call",
            "params": {
                "name": name,
                "arguments": args,
            }
        });

        let resp = self
            .http
            .post(format!("{}/mcp", self.base_url))
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("HTTP request to /mcp failed: {e}"))?;

        let status = resp.status();
        let text = resp
            .text()
            .await
            .map_err(|e| format!("failed to read /mcp response body: {e}"))?;

        if !status.is_success() {
            return Err(format!("/mcp returned HTTP {status}: {text}"));
        }

        serde_json::from_str::<JsonRpcResponse>(&text)
            .map_err(|e| format!("failed to deserialize JSON-RPC response: {e}\nbody: {text}"))
    }

    /// Calls `compile_context` and deserializes the result into a
    /// `CompileContextResponse`.
    ///
    /// Returns `Err` when the HTTP call fails, the JSON-RPC response carries an
    /// error, or the result cannot be deserialized.
    pub async fn compile_context(
        &self,
        args: CompileContextArgs,
    ) -> Result<CompileContextResponse, String> {
        let rpc = self.call_tool("compile_context", &args).await?;
        let result = rpc
            .result
            .ok_or_else(|| format!("compile_context RPC error: {:?}", rpc.error))?;
        serde_json::from_value::<CompileContextResponse>(result.clone()).map_err(|e| {
            format!("failed to deserialize compile_context result: {e}\nraw: {result}")
        })
    }

    /// Calls `extract_session` and deserializes the result into an
    /// `ExtractSessionResponse`.
    pub async fn extract_session(
        &self,
        args: ExtractSessionArgs,
    ) -> Result<ExtractSessionResponse, String> {
        let rpc = self.call_tool("extract_session", &args).await?;
        let result = rpc
            .result
            .ok_or_else(|| format!("extract_session RPC error: {:?}", rpc.error))?;
        serde_json::from_value::<ExtractSessionResponse>(result.clone()).map_err(|e| {
            format!("failed to deserialize extract_session result: {e}\nraw: {result}")
        })
    }

    /// Calls `GET /health` and returns `(status_code, body_json)`.
    ///
    /// A `200` with `{"healthy":true}` means the server is ready.
    /// A `503` means the server is up but unhealthy.
    pub async fn health(&self) -> Result<(u16, Value), String> {
        let resp = self
            .http
            .get(format!("{}/health", self.base_url))
            .send()
            .await
            .map_err(|e| format!("GET /health failed: {e}"))?;

        let status_code = resp.status().as_u16();
        let body = resp
            .json::<Value>()
            .await
            .map_err(|e| format!("failed to read /health body: {e}"))?;

        Ok((status_code, body))
    }

    /// Posts a transcript to `POST /ingest/transcript`.
    ///
    /// `secret` is sent as the `X-Ingest-Secret` header when `Some`.
    /// Returns the HTTP status code and raw body text.
    pub async fn ingest_transcript(
        &self,
        body: IngestTranscriptBody,
        secret: Option<&str>,
    ) -> Result<(u16, String), String> {
        let mut req = self
            .http
            .post(format!("{}/ingest/transcript", self.base_url))
            .json(&body);

        if let Some(s) = secret {
            req = req.header("X-Ingest-Secret", s);
        }

        let resp = req
            .send()
            .await
            .map_err(|e| format!("POST /ingest/transcript failed: {e}"))?;

        let status = resp.status().as_u16();
        let text = resp
            .text()
            .await
            .map_err(|e| format!("failed to read /ingest/transcript body: {e}"))?;

        Ok((status, text))
    }
}

impl Default for McpClient {
    fn default() -> Self {
        Self::new()
    }
}
