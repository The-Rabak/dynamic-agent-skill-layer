use std::{future::Future, net::SocketAddr, pin::Pin};

use axum::{Json, Router, extract::State, routing::post};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::{Value, json};

use crate::tools::{extract_session::ExtractSessionRequest, find_skill::FindSkillRequest};
use crate::{McpServerApp, tools::compile_context::CompileContextRequest};
use admin::tools::{
    InspectSkillRequest, ListCommunitiesRequest, RebuildGraphRequest, RebuildGraphStatusRequest,
};

/// Metadata describing an MCP tool exposed by this server.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ToolDescriptor {
    pub name: &'static str,
    pub description: &'static str,
    pub required_arguments: &'static [&'static str],
}

type ToolCallFuture<'a> = Pin<Box<dyn Future<Output = JsonRpcResponse> + Send + 'a>>;
type ToolCallHandler = for<'a> fn(&'a McpServerApp, Option<Value>, Value) -> ToolCallFuture<'a>;

#[derive(Clone, Copy)]
struct RegisteredTool {
    descriptor: ToolDescriptor,
    handler: ToolCallHandler,
}

const REGISTERED_TOOLS: [RegisteredTool; 7] = [
    RegisteredTool {
        descriptor: ToolDescriptor {
            name: "compile_context",
            description: "Compile task-relevant context for the current session",
            required_arguments: &["prompt", "session_id", "repo_path"],
        },
        handler: call_compile_context,
    },
    RegisteredTool {
        descriptor: ToolDescriptor {
            name: "find_skill",
            description: "Find top matching skills from the retrieval graph",
            required_arguments: &["prompt"],
        },
        handler: call_find_skill,
    },
    RegisteredTool {
        descriptor: ToolDescriptor {
            name: "extract_session",
            description: "Queue session transcript extraction into .pending drafts",
            required_arguments: &["transcript_ref", "session_id"],
        },
        handler: call_extract_session,
    },
    RegisteredTool {
        descriptor: ToolDescriptor {
            name: "rebuild_graph",
            description: "Trigger a full graph rebuild via the graph-builder workflow",
            required_arguments: &[],
        },
        handler: call_rebuild_graph,
    },
    RegisteredTool {
        descriptor: ToolDescriptor {
            name: "rebuild_graph_status",
            description: "Read lifecycle and result fields for a queued rebuild job",
            required_arguments: &["job_id"],
        },
        handler: call_rebuild_graph_status,
    },
    RegisteredTool {
        descriptor: ToolDescriptor {
            name: "inspect_skill",
            description: "Inspect skill neighborhood, subunits, and community context",
            required_arguments: &["skill_id"],
        },
        handler: call_inspect_skill,
    },
    RegisteredTool {
        descriptor: ToolDescriptor {
            name: "list_communities",
            description: "List graph communities with member counts",
            required_arguments: &[],
        },
        handler: call_list_communities,
    },
];

/// Returns canonical MCP tool metadata used by both `tools/list` and `tools/call`.
pub fn registered_tool_descriptors() -> Vec<ToolDescriptor> {
    REGISTERED_TOOLS
        .iter()
        .map(|tool| tool.descriptor)
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
                "inputSchema": {
                    "type": "object",
                    "required": tool.descriptor.required_arguments
                }
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
    State(app): State<McpServerApp>,
    Json(request): Json<JsonRpcRequest>,
) -> Json<JsonRpcResponse> {
    Json(app.handle_json_rpc(request).await)
}

pub fn router(app: McpServerApp) -> Router {
    Router::new()
        .route("/mcp", post(mcp_handler))
        .with_state(app)
}

pub async fn serve_http(app: McpServerApp, address: SocketAddr) -> std::io::Result<()> {
    let listener = tokio::net::TcpListener::bind(address).await?;
    axum::serve(listener, router(app))
        .await
        .map_err(std::io::Error::other)
}
