use std::{future::Future, net::SocketAddr, pin::Pin, sync::LazyLock};

use axum::{
    Json, Router,
    extract::State,
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
                        "transcript_inline": {"type": "string", "description": "Inline transcript content, used instead of loading from file"},
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

pub fn router(app: McpServerApp, health_checker: InfrastructureHealthChecker) -> Router {
    let ingest_secret = std::env::var(INGEST_SECRET_ENV)
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty());

    Router::new()
        .route("/mcp", post(mcp_handler))
        .route("/health", get(health_handler))
        .route("/ingest/transcript", post(ingest_transcript_handler))
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
