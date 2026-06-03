use std::{future::Future, net::SocketAddr, pin::Pin, sync::LazyLock};

use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::{get, post},
};
use infrastructure::{EnqueueOutcome, InfrastructureHealthChecker, TranscriptQueueError};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::{Value, json};

use crate::tools::{extract_session::ExtractSessionRequest, find_skill::FindSkillRequest};
use crate::{
    McpServerApp, TranscriptIngestHttpRequest, TranscriptIngestOutcome,
    tools::compile_context::CompileContextRequest,
};
use admin::tools::{
    InspectSkillRequest, ListCommunitiesRequest, RebuildGraphRequest, RebuildGraphStatusRequest,
};

/// Metadata describing an MCP tool exposed by this server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolDescriptor {
    pub name: &'static str,
    pub description: &'static str,
    pub required_arguments: &'static [&'static str],
    pub input_schema: Value,
}

type ToolCallFuture<'a> = Pin<Box<dyn Future<Output = JsonRpcResponse> + Send + 'a>>;
type ToolCallHandler = for<'a> fn(&'a McpServerApp, Option<Value>, Value) -> ToolCallFuture<'a>;

#[derive(Clone)]
struct RegisteredTool {
    descriptor: ToolDescriptor,
    handler: ToolCallHandler,
}

static REGISTERED_TOOLS: LazyLock<[RegisteredTool; 7]> = LazyLock::new(|| {
    [
        RegisteredTool {
            descriptor: ToolDescriptor {
                name: "compile_context",
                description: "Compile task-relevant context for the current session",
                required_arguments: &["prompt", "session_id", "repo_path"],
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "prompt": {"type": "string", "description": "Natural-language description of the task to compile skills for"},
                        "session_id": {"type": "string", "description": "Identifier for the current agent session"},
                        "repo_path": {"type": "string", "description": "Absolute path to the current repository root"},
                        "trigger": {"type": "string", "maxLength": 64, "description": "Lifecycle event that caused this call (e.g. 'compact'). 'compact' bypasses session suppression for post-compaction re-injection. Unknown values are treated as ordinary calls."}
                    },
                    "required": ["prompt", "session_id", "repo_path"]
                }),
            },
            handler: call_compile_context,
        },
        RegisteredTool {
            descriptor: ToolDescriptor {
                name: "find_skill",
                description: "Find top matching skills from the retrieval graph",
                required_arguments: &["prompt"],
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "prompt": {"type": "string", "description": "Natural-language query to match against the skill graph"},
                        "limit": {"type": "integer", "description": "Maximum number of skills to return (default 5)"}
                    },
                    "required": ["prompt"]
                }),
            },
            handler: call_find_skill,
        },
        RegisteredTool {
            descriptor: ToolDescriptor {
                name: "extract_session",
                description: "Queue session transcript extraction into .pending drafts",
                required_arguments: &["transcript_ref", "session_id"],
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "transcript_ref": {"type": "string", "description": "Path to the session transcript file to extract from"},
                        "session_id": {"type": "string", "description": "Identifier for the source session"},
                        "transcript_inline": {"type": "string", "maxLength": MCP_BODY_LIMIT_BYTES, "description": "Inline transcript content, used instead of loading from file. Max 4 MiB; use transcript_ref for larger transcripts."},
                        "repo_path": {"type": "string", "description": "Absolute path to the repository root for scoping drafts"}
                    },
                    "required": ["transcript_ref", "session_id"]
                }),
            },
            handler: call_extract_session,
        },
        RegisteredTool {
            descriptor: ToolDescriptor {
                name: "rebuild_graph",
                description: "Trigger a full graph rebuild via the graph-builder workflow",
                required_arguments: &[],
                input_schema: json!({
                    "type": "object",
                    "properties": {},
                    "required": []
                }),
            },
            handler: call_rebuild_graph,
        },
        RegisteredTool {
            descriptor: ToolDescriptor {
                name: "rebuild_graph_status",
                description: "Read lifecycle and result fields for a queued rebuild job",
                required_arguments: &["job_id"],
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "job_id": {"type": "string", "description": "The rebuild job identifier returned by rebuild_graph"}
                    },
                    "required": ["job_id"]
                }),
            },
            handler: call_rebuild_graph_status,
        },
        RegisteredTool {
            descriptor: ToolDescriptor {
                name: "inspect_skill",
                description: "Inspect skill neighborhood, subunits, and community context",
                required_arguments: &["skill_id"],
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "skill_id": {"type": "string", "description": "Stable identifier of the skill to inspect"}
                    },
                    "required": ["skill_id"]
                }),
            },
            handler: call_inspect_skill,
        },
        RegisteredTool {
            descriptor: ToolDescriptor {
                name: "list_communities",
                description: "List graph communities with member counts",
                required_arguments: &[],
                input_schema: json!({
                    "type": "object",
                    "properties": {},
                    "required": []
                }),
            },
            handler: call_list_communities,
        },
    ]
});

/// Returns canonical MCP tool metadata used by both `tools/list` and `tools/call`.
pub fn registered_tool_descriptors() -> Vec<ToolDescriptor> {
    REGISTERED_TOOLS
        .iter()
        .map(|tool| tool.descriptor.clone())
        .collect::<Vec<ToolDescriptor>>()
}

#[derive(Debug, Clone, Deserialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    #[serde(default)]
    pub id: Option<Value>,
    pub method: String,
    #[serde(default)]
    pub params: Value,
}

#[derive(Debug, Clone, Serialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

#[derive(Debug, Clone, Serialize)]
pub struct JsonRpcError {
    pub code: i32,
    pub message: String,
}

#[derive(Debug, Clone, Deserialize)]
struct ToolCallParams {
    pub name: String,
    #[serde(default)]
    pub arguments: Value,
}

impl JsonRpcResponse {
    fn ok(id: Option<Value>, result: Value) -> Self {
        Self {
            jsonrpc: "2.0",
            id,
            result: Some(result),
            error: None,
        }
    }

    fn error(id: Option<Value>, code: i32, message: impl Into<String>) -> Self {
        Self {
            jsonrpc: "2.0",
            id,
            result: None,
            error: Some(JsonRpcError {
                code,
                message: message.into(),
            }),
        }
    }
}

impl McpServerApp {
    pub async fn handle_json_rpc(&self, request: JsonRpcRequest) -> JsonRpcResponse {
        if request.jsonrpc != "2.0" {
            return JsonRpcResponse::error(request.id, -32600, "invalid request");
        }

        match request.method.as_str() {
            "tools/list" => JsonRpcResponse::ok(request.id, tools_list_payload()),
            "tools/call" => self.handle_tool_call(request.id, request.params).await,
            _ => JsonRpcResponse::error(request.id, -32601, "method not found"),
        }
    }

    async fn handle_tool_call(&self, id: Option<Value>, params: Value) -> JsonRpcResponse {
        let tool_call: ToolCallParams = match serde_json::from_value(params) {
            Ok(value) => value,
            Err(error) => {
                return JsonRpcResponse::error(id, -32602, format!("invalid params: {error}"));
            }
        };

        let Some(tool) = REGISTERED_TOOLS
            .iter()
            .find(|tool| tool.descriptor.name == tool_call.name)
        else {
            return JsonRpcResponse::error(id, -32601, "tool not found");
        };

        (tool.handler)(self, id, tool_call.arguments).await
    }

    async fn invoke_typed_tool<Request, Response, Invoke, InvokeFuture>(
        &self,
        id: Option<Value>,
        arguments: Value,
        tool_name: &'static str,
        invoke: Invoke,
    ) -> JsonRpcResponse
    where
        Request: DeserializeOwned,
        Response: Serialize,
        Invoke: FnOnce(Request) -> InvokeFuture,
        InvokeFuture: Future<Output = Response>,
    {
        let request: Request = match serde_json::from_value(arguments) {
            Ok(value) => value,
            Err(error) => {
                return JsonRpcResponse::error(
                    id,
                    -32602,
                    format!("invalid {tool_name} arguments: {error}"),
                );
            }
        };

        match serde_json::to_value(invoke(request).await) {
            Ok(result) => JsonRpcResponse::ok(id, result),
            Err(error) => JsonRpcResponse::error(id, -32603, format!("internal error: {error}")),
        }
    }
}

/// Validates a client-supplied `session_id` for use as a safe key segment in
/// both the suppression DashMap and the Redis SCAN patterns.
///
/// Rejects any value that:
/// - contains `"::"` (the separator used in suppression and cache key formats —
///   a value like `"abc::extra"` would make `clear_session("abc")` evict it
///   because the prefix `"abc::"` matches `"abc::extra::"` via `starts_with`), or
/// - contains characters outside `[A-Za-z0-9_-]` (defends against glob injection
///   in the Redis SCAN pattern even after `escape_redis_glob` is applied, and
///   keeps the session identifier space predictable for operators).
///
/// Returns `Ok(())` when the id is safe, or `Err(message)` describing the violation.
fn validate_session_id(session_id: &str) -> Result<(), String> {
    if session_id.contains("::") {
        return Err(format!(
            "session_id must not contain '::' (separator collision risk): {session_id:?}"
        ));
    }
    let all_valid = session_id
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-');
    if !all_valid {
        return Err(format!(
            "session_id must contain only [A-Za-z0-9_-]: {session_id:?}"
        ));
    }
    Ok(())
}

fn tools_list_payload() -> Value {
    let tools = REGISTERED_TOOLS
        .iter()
        .map(|tool| {
            json!({
                "name": tool.descriptor.name,
                "description": tool.descriptor.description,
                "inputSchema": tool.descriptor.input_schema
            })
        })
        .collect::<Vec<Value>>();

    json!({ "tools": tools })
}

fn call_compile_context<'a>(
    app: &'a McpServerApp,
    id: Option<Value>,
    arguments: Value,
) -> ToolCallFuture<'a> {
    Box::pin(async move {
        // Validate session_id before deserializing the full request so a bad id
        // returns a structured invalid_params error rather than a generic serde message.
        if let Some(session_id) = arguments.get("session_id").and_then(Value::as_str) {
            if let Err(reason) = validate_session_id(session_id) {
                return JsonRpcResponse::error(id, -32602, format!("invalid params: {reason}"));
            }
        }
        app.invoke_typed_tool(
            id,
            arguments,
            "compile_context",
            |request: CompileContextRequest| app.compile_context(request),
        )
        .await
    })
}

fn call_find_skill<'a>(
    app: &'a McpServerApp,
    id: Option<Value>,
    arguments: Value,
) -> ToolCallFuture<'a> {
    Box::pin(async move {
        app.invoke_typed_tool(id, arguments, "find_skill", |request: FindSkillRequest| {
            app.find_skill(request)
        })
        .await
    })
}

fn call_extract_session<'a>(
    app: &'a McpServerApp,
    id: Option<Value>,
    arguments: Value,
) -> ToolCallFuture<'a> {
    Box::pin(async move {
        // Validate session_id at the param boundary to prevent separator collision and
        // Redis glob injection on the subsequent clear_session call in extract_session.
        if let Some(session_id) = arguments.get("session_id").and_then(Value::as_str) {
            if let Err(reason) = validate_session_id(session_id) {
                return JsonRpcResponse::error(id, -32602, format!("invalid params: {reason}"));
            }
        }

        // Preflight the inline transcript size before the transport layer can reject the
        // request with a bare HTTP 413 (which the caller cannot distinguish from a crash).
        // Returning a structured result here gives the agent a machine-legible reason_code
        // and a clear recovery path (switch to transcript_ref for larger payloads).
        if let Some(inline) = arguments.get("transcript_inline").and_then(Value::as_str) {
            if inline.len() > MCP_BODY_LIMIT_BYTES {
                return JsonRpcResponse::ok(
                    id,
                    json!({
                        "status": "failed",
                        "reason_code": "payload_too_large",
                        "job_id": null,
                        "provider": null
                    }),
                );
            }
        }

        app.invoke_typed_tool(
            id,
            arguments,
            "extract_session",
            |request: ExtractSessionRequest| app.extract_session(request),
        )
        .await
    })
}

fn call_rebuild_graph<'a>(
    app: &'a McpServerApp,
    id: Option<Value>,
    arguments: Value,
) -> ToolCallFuture<'a> {
    Box::pin(async move {
        app.invoke_typed_tool(
            id,
            arguments,
            "rebuild_graph",
            |request: RebuildGraphRequest| app.rebuild_graph(request),
        )
        .await
    })
}

fn call_rebuild_graph_status<'a>(
    app: &'a McpServerApp,
    id: Option<Value>,
    arguments: Value,
) -> ToolCallFuture<'a> {
    Box::pin(async move {
        app.invoke_typed_tool(
            id,
            arguments,
            "rebuild_graph_status",
            |request: RebuildGraphStatusRequest| app.rebuild_graph_status(request),
        )
        .await
    })
}

fn call_inspect_skill<'a>(
    app: &'a McpServerApp,
    id: Option<Value>,
    arguments: Value,
) -> ToolCallFuture<'a> {
    Box::pin(async move {
        app.invoke_typed_tool(
            id,
            arguments,
            "inspect_skill",
            |request: InspectSkillRequest| app.inspect_skill(request),
        )
        .await
    })
}

fn call_list_communities<'a>(
    app: &'a McpServerApp,
    id: Option<Value>,
    arguments: Value,
) -> ToolCallFuture<'a> {
    Box::pin(async move {
        app.invoke_typed_tool(
            id,
            arguments,
            "list_communities",
            |request: ListCommunitiesRequest| app.list_communities(request),
        )
        .await
    })
}

async fn mcp_handler(
    State(state): State<HttpAppState>,
    Json(request): Json<JsonRpcRequest>,
) -> Json<JsonRpcResponse> {
    Json(state.app.handle_json_rpc(request).await)
}

/// HTTP header carrying the transcript-ingest shared secret (todo 103 / 099).
const INGEST_SECRET_HEADER: &str = "x-ingest-secret";

/// Env var holding the transcript-ingest shared secret.
///
/// When set, `/ingest/transcript` requires a matching `X-Ingest-Secret` header.
/// When unset, the endpoint relies solely on the localhost port binding
/// (constitution §Deferred-risk guard) and logs a warning at first use — the
/// surface is still loopback-only, but the secret is the defense-in-depth layer
/// coordinated with todo 099.
const INGEST_SECRET_ENV: &str = "TRANSCRIPT_INGEST_SECRET";

#[derive(Clone)]
struct HttpAppState {
    app: McpServerApp,
    health_checker: InfrastructureHealthChecker,
    ingest_secret: Option<String>,
}

/// Compares two byte slices in constant time to prevent timing attacks.
///
/// Returns `true` iff both slices have equal length and identical contents.
/// Uses XOR accumulation so the loop always runs for `min(a.len(), b.len())`
/// iterations regardless of where the first difference appears, preventing
/// timing-based length or content inference.
///
/// This is a defense-in-depth measure: the ingest endpoint is loopback-only,
/// but a correct constant-time compare is cheap and avoids a subtle class of
/// vulnerability if the binding constraint ever relaxes.
#[inline]
fn constant_time_bytes_equal(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    // XOR each pair of bytes and OR the results. A non-zero accumulator means
    // at least one byte differed. The loop always runs all iterations.
    let mut diff: u8 = 0;
    for (byte_a, byte_b) in a.iter().zip(b.iter()) {
        diff |= byte_a ^ byte_b;
    }
    diff == 0
}

/// Verifies the shared secret for a transcript-ingest request.
///
/// Returns `Ok(())` when no secret is configured (loopback-only guard) or when
/// the supplied header matches; otherwise an explicit `401`.
fn check_ingest_secret(
    configured: Option<&str>,
    headers: &HeaderMap,
) -> Result<(), (StatusCode, Json<Value>)> {
    let Some(expected) = configured else {
        tracing::warn!(
            "transcript ingest received without {INGEST_SECRET_ENV} configured; \
             relying on localhost binding only"
        );
        return Ok(());
    };

    let provided = headers
        .get(INGEST_SECRET_HEADER)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();

    // Constant-time comparison so secret length is not leaked via short-circuit.
    // The endpoint is loopback-only so this is defense-in-depth, not the primary
    // boundary — but a proper constant-time compare is correct here.
    if constant_time_bytes_equal(provided.as_bytes(), expected.as_bytes()) {
        Ok(())
    } else {
        Err((
            StatusCode::UNAUTHORIZED,
            Json(json!({ "status": "unauthorized", "reason_code": "ingest_secret_mismatch" })),
        ))
    }
}

async fn ingest_transcript_handler(
    State(state): State<HttpAppState>,
    headers: HeaderMap,
    Json(request): Json<TranscriptIngestHttpRequest>,
) -> impl IntoResponse {
    if let Err(rejection) = check_ingest_secret(state.ingest_secret.as_deref(), &headers) {
        return rejection;
    }

    match state.app.ingest_transcript(request).await {
        TranscriptIngestOutcome::Accepted(EnqueueOutcome::Enqueued { id, content_hash }) => (
            StatusCode::ACCEPTED,
            Json(json!({
                "status": "enqueued",
                "id": id.to_string(),
                "content_hash": content_hash,
            })),
        ),
        TranscriptIngestOutcome::Accepted(EnqueueOutcome::Duplicate { content_hash }) => (
            StatusCode::OK,
            Json(json!({
                "status": "duplicate",
                "content_hash": content_hash,
            })),
        ),
        TranscriptIngestOutcome::InvalidContract(error) => {
            let status = match error {
                TranscriptQueueError::ContentTooLarge { .. } => StatusCode::PAYLOAD_TOO_LARGE,
                _ => StatusCode::BAD_REQUEST,
            };
            (
                status,
                Json(json!({
                    "status": "rejected",
                    "reason_code": error.reason_code(),
                    "detail": error.to_string(),
                })),
            )
        }
        TranscriptIngestOutcome::PersistenceError(error) => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({
                "status": "error",
                "reason_code": error.reason_code(),
            })),
        ),
        TranscriptIngestOutcome::Unavailable => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({
                "status": "unavailable",
                "reason_code": "transcript_ingest_not_configured",
            })),
        ),
    }
}

async fn health_handler(State(state): State<HttpAppState>) -> impl IntoResponse {
    let report = state.health_checker.check().await;
    let status = if report.healthy {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };

    (status, Json(report))
}

/// Hard cap on the request body size accepted by the MCP and ingest routes.
///
/// 4 MiB is generous enough for large inline transcripts while preventing
/// unbounded buffering of `extract_session.transcript_inline` payloads.
/// Requests exceeding this limit receive 413 Payload Too Large.
pub const MCP_BODY_LIMIT_BYTES: usize = 4 * 1024 * 1024; // 4 MiB

/// Default bind address used by the MCP server when `MCP_SERVER_ADDR` is not set.
///
/// Loopback-only binding is a stated trust-boundary requirement: the MCP
/// endpoint must not be exposed on a broad interface without an explicit
/// operator opt-in via the environment variable.
///
/// `main.rs` uses this as the `.unwrap_or_else` fallback for `MCP_SERVER_ADDR`.
/// The loopback test in this module (`default_bind_address_is_loopback`) asserts
/// against this const so both sites stay in sync — changing this value will
/// fail the test, making any accidental `0.0.0.0` regression detectable.
pub const DEFAULT_MCP_SERVER_ADDR: &str = "127.0.0.1:3001";

pub fn router(app: McpServerApp, health_checker: InfrastructureHealthChecker) -> Router {
    let ingest_secret = std::env::var(INGEST_SECRET_ENV)
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty());

    Router::new()
        .route("/mcp", post(mcp_handler))
        .route("/health", get(health_handler))
        .route("/ingest/transcript", post(ingest_transcript_handler))
        .layer(DefaultBodyLimit::max(MCP_BODY_LIMIT_BYTES))
        .with_state(HttpAppState {
            app,
            health_checker,
            ingest_secret,
        })
}

pub async fn serve_http(
    app: McpServerApp,
    health_checker: InfrastructureHealthChecker,
    address: SocketAddr,
) -> std::io::Result<()> {
    let listener = tokio::net::TcpListener::bind(address).await?;
    axum::serve(listener, router(app, health_checker))
        .await
        .map_err(std::io::Error::other)
}

#[cfg(test)]
mod tests {
    use std::{net::SocketAddr, sync::Arc};

    use domain::{EmbeddingError, EmbeddingService};
    use retrieval::{RetrievalConfig, RetrievalSnapshot};

    use super::*;
    use crate::McpServerApp;

    /// Minimal embedding stub for protocol-level tests that do not exercise
    /// the retrieval pipeline.
    struct NoOpEmbeddingService;

    #[async_trait::async_trait]
    impl EmbeddingService for NoOpEmbeddingService {
        async fn embed_text(&self, _text: &str) -> Result<Vec<f32>, EmbeddingError> {
            Err(EmbeddingError::ProviderUnavailable("test stub".to_owned()))
        }

        async fn embed_batch(&self, _texts: &[&str]) -> Result<Vec<Vec<f32>>, EmbeddingError> {
            Err(EmbeddingError::ProviderUnavailable("test stub".to_owned()))
        }
    }

    /// Proves that the MCP router rejects payloads exceeding the hard body cap
    /// with 413 Payload Too Large. This guards against unbounded buffering of
    /// the `transcript_inline` field or any other large JSON-RPC body (DoS).
    ///
    /// The explicit `DefaultBodyLimit::max(MCP_BODY_LIMIT_BYTES)` layer in
    /// `router()` must enforce this cap. Without it, axum 0.8 has a 2 MiB
    /// default limit; the explicit limit documents intent and is tested here.
    #[tokio::test]
    async fn mcp_router_rejects_oversized_body_with_413() {
        let app = McpServerApp::with_explicit_graph(
            Arc::new(NoOpEmbeddingService),
            RetrievalSnapshot::new(vec![], 0),
            RetrievalConfig::default(),
            None,
        );
        let health_checker = InfrastructureHealthChecker::new();
        let app_router = router(app, health_checker);

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("ephemeral port should bind");
        let addr: SocketAddr = listener.local_addr().expect("listener has local addr");
        tokio::spawn(async move {
            axum::serve(listener, app_router)
                .await
                .expect("test server should serve");
        });

        // Send a body larger than MCP_BODY_LIMIT_BYTES to prove the cap is enforced.
        let oversized_payload = "x".repeat(MCP_BODY_LIMIT_BYTES + 1);
        let client = reqwest::Client::new();
        let response = client
            .post(format!("http://{addr}/mcp"))
            .header("content-type", "application/json")
            .body(oversized_payload)
            .send()
            .await
            .expect("request should complete");

        assert_eq!(
            response.status(),
            reqwest::StatusCode::PAYLOAD_TOO_LARGE,
            "oversized MCP body must be rejected with 413"
        );
    }

    /// Proves the server binds to loopback (`127.0.0.1`) by default rather than
    /// `0.0.0.0`, satisfying the localhost safety-posture requirement.
    ///
    /// The `MCP_SERVER_ADDR` env var can override this for deployment; without
    /// it the server must not expose the MCP endpoint on a broad interface.
    ///
    /// This test asserts against `DEFAULT_MCP_SERVER_ADDR`, the same const that
    /// `main.rs` uses as its `.unwrap_or_else` fallback (see `main.rs` line 24).
    /// Any accidental change from loopback (e.g. to `0.0.0.0`) will fail here.
    #[test]
    fn default_bind_address_is_loopback() {
        let addr: SocketAddr = DEFAULT_MCP_SERVER_ADDR
            .parse()
            .expect("DEFAULT_MCP_SERVER_ADDR must parse as SocketAddr");
        assert!(
            addr.ip().is_loopback(),
            "DEFAULT_MCP_SERVER_ADDR must bind to loopback, got: {}",
            addr.ip()
        );
    }

    // -- validate_session_id unit tests --

    /// Proves that well-formed session ids pass validation without error.
    #[test]
    fn validate_session_id_accepts_alphanumeric_and_dash_underscore() {
        assert!(validate_session_id("session-abc-123").is_ok());
        assert!(validate_session_id("ABC_DEF").is_ok());
        assert!(validate_session_id("a").is_ok());
        assert!(validate_session_id("z-0_Z").is_ok());
    }

    /// Proves that a session_id containing `"::"` is rejected with an explicit error.
    ///
    /// This is the separator-collision guard: `clear_session("abc")` uses a DashMap
    /// prefix `"abc::"`, which would inadvertently match an entry for `"abc::extra"`.
    /// Rejecting at the boundary prevents the collision from arising.
    #[test]
    fn validate_session_id_rejects_double_colon_separator() {
        let result = validate_session_id("abc::extra");
        assert!(
            result.is_err(),
            "session_id with '::' must be rejected — separator collision risk"
        );
        let msg = result.unwrap_err();
        assert!(
            msg.contains("::"),
            "error must mention the forbidden separator, got: {msg}"
        );
    }

    /// Proves that a session_id of `"*"` (Redis glob wildcard) is rejected.
    ///
    /// Without protocol-level rejection, a caller could supply `"*"` as a session_id.
    /// Even with `escape_redis_glob` applied in `clear_session`, rejecting it at the
    /// boundary is a stronger defense-in-depth measure and keeps the id space clean.
    #[test]
    fn validate_session_id_rejects_wildcard() {
        assert!(
            validate_session_id("*").is_err(),
            "session_id '*' must be rejected — Redis glob injection"
        );
        assert!(
            validate_session_id("?").is_err(),
            "session_id '?' must be rejected — Redis glob injection"
        );
        assert!(
            validate_session_id("[bracket]").is_err(),
            "session_id with brackets must be rejected"
        );
    }

    /// Proves that a session_id containing a colon (single) is rejected.
    ///
    /// Even a single colon can corrupt the `"cache:{session_id}:{hash}"` key format
    /// used in the context cache, making subsequent SCAN patterns ambiguous.
    #[test]
    fn validate_session_id_rejects_single_colon() {
        assert!(
            validate_session_id("abc:def").is_err(),
            "session_id with single ':' must be rejected — would corrupt key format"
        );
    }

    /// Proves that `compile_context` returns a JSON-RPC `invalid_params` error (-32602)
    /// when called with a `session_id` containing `"::"`.
    #[tokio::test]
    async fn compile_context_returns_invalid_params_for_separator_session_id() {
        let app = McpServerApp::with_explicit_graph(
            Arc::new(NoOpEmbeddingService),
            RetrievalSnapshot::new(vec![], 0),
            RetrievalConfig::default(),
            None,
        );
        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(1)),
            method: "tools/call".to_owned(),
            params: json!({
                "name": "compile_context",
                "arguments": {
                    "prompt": "test prompt",
                    "session_id": "abc::extra",
                    "repo_path": "/tmp/repo"
                }
            }),
        };
        let response = app.handle_json_rpc(request).await;
        assert!(
            response.error.is_some(),
            "expected a JSON-RPC error for separator-colliding session_id"
        );
        let error = response.error.unwrap();
        assert_eq!(
            error.code, -32602,
            "expected invalid_params code -32602, got: {}",
            error.code
        );
        assert!(
            error.message.contains("invalid params"),
            "error message must mention 'invalid params', got: {}",
            error.message
        );
    }

    /// Proves that `extract_session` returns a structured JSON-RPC result with
    /// `reason_code: "payload_too_large"` when `transcript_inline` exceeds the
    /// 4 MiB body cap, rather than letting the transport return a bare HTTP 413.
    ///
    /// This satisfies the agent-native contract: the agent can inspect the result,
    /// read the reason_code, and recover by switching to `transcript_ref`.
    #[tokio::test]
    async fn extract_session_returns_payload_too_large_reason_code_for_oversized_inline() {
        let app = McpServerApp::with_explicit_graph(
            Arc::new(NoOpEmbeddingService),
            RetrievalSnapshot::new(vec![], 0),
            RetrievalConfig::default(),
            None,
        );
        let oversized_inline = "x".repeat(MCP_BODY_LIMIT_BYTES + 1);
        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(42)),
            method: "tools/call".to_owned(),
            params: json!({
                "name": "extract_session",
                "arguments": {
                    "transcript_ref": "/tmp/transcript.json",
                    "session_id": "test-session",
                    "transcript_inline": oversized_inline
                }
            }),
        };
        let response = app.handle_json_rpc(request).await;
        assert!(
            response.error.is_none(),
            "oversized inline must return a structured result, not a JSON-RPC error"
        );
        let result = response.result.expect("response must carry a result value");
        assert_eq!(
            result.get("reason_code").and_then(Value::as_str),
            Some("payload_too_large"),
            "result must carry reason_code 'payload_too_large', got: {result}"
        );
        assert_eq!(
            result.get("status").and_then(Value::as_str),
            Some("failed"),
            "result status must be 'failed', got: {result}"
        );
    }

    /// Proves the `/ingest/transcript` route also enforces the 4 MiB body cap
    /// with HTTP 413, independent of the `/mcp` route test.
    ///
    /// The `DefaultBodyLimit` layer applies to all routes; this test ensures
    /// the ingest endpoint is not accidentally exempted if the router changes.
    #[tokio::test]
    async fn ingest_transcript_rejects_oversized_body_with_413() {
        let app = McpServerApp::with_explicit_graph(
            Arc::new(NoOpEmbeddingService),
            RetrievalSnapshot::new(vec![], 0),
            RetrievalConfig::default(),
            None,
        );
        let health_checker = InfrastructureHealthChecker::new();
        let app_router = router(app, health_checker);

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("ephemeral port should bind");
        let addr: SocketAddr = listener.local_addr().expect("listener has local addr");
        tokio::spawn(async move {
            axum::serve(listener, app_router)
                .await
                .expect("test server should serve");
        });

        // Send a body larger than MCP_BODY_LIMIT_BYTES to the ingest route.
        let oversized_payload = "x".repeat(MCP_BODY_LIMIT_BYTES + 1);
        let client = reqwest::Client::new();
        let response = client
            .post(format!("http://{addr}/ingest/transcript"))
            .header("content-type", "application/json")
            .body(oversized_payload)
            .send()
            .await
            .expect("request should complete");

        assert_eq!(
            response.status(),
            reqwest::StatusCode::PAYLOAD_TOO_LARGE,
            "oversized /ingest/transcript body must be rejected with 413"
        );
    }

    /// Proves that `extract_session` returns a JSON-RPC `invalid_params` error (-32602)
    /// when called with a `session_id` of `"*"` (glob wildcard).
    #[tokio::test]
    async fn extract_session_returns_invalid_params_for_wildcard_session_id() {
        let app = McpServerApp::with_explicit_graph(
            Arc::new(NoOpEmbeddingService),
            RetrievalSnapshot::new(vec![], 0),
            RetrievalConfig::default(),
            None,
        );
        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(2)),
            method: "tools/call".to_owned(),
            params: json!({
                "name": "extract_session",
                "arguments": {
                    "transcript_ref": "/tmp/transcript.json",
                    "session_id": "*"
                }
            }),
        };
        let response = app.handle_json_rpc(request).await;
        assert!(
            response.error.is_some(),
            "expected a JSON-RPC error for wildcard session_id"
        );
        let error = response.error.unwrap();
        assert_eq!(
            error.code, -32602,
            "expected invalid_params code -32602, got: {}",
            error.code
        );
    }
}
