use std::net::SocketAddr;

use axum::{Json, Router, extract::State, routing::post};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{
    McpServerApp,
    tools::{
        compile_context::CompileContextRequest, extract_session::ExtractSessionRequest,
        find_skill::FindSkillRequest,
    },
};

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
            "tools/list" => JsonRpcResponse::ok(
                request.id,
                json!({
                    "tools": [
                        {
                            "name": "compile_context",
                            "description": "Compile task-relevant context for the current session",
                            "inputSchema": {
                                "type": "object",
                                "required": ["prompt", "session_id", "repo_path"]
                            }
                        },
                        {
                            "name": "find_skill",
                            "description": "Find top matching skills from the retrieval graph",
                            "inputSchema": {
                                "type": "object",
                                "required": ["prompt"]
                            }
                        },
                        {
                            "name": "extract_session",
                            "description": "Queue session transcript extraction into .pending drafts",
                            "inputSchema": {
                                "type": "object",
                                "required": ["transcript_ref", "session_id"]
                            }
                        }
                    ]
                }),
            ),
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

        match tool_call.name.as_str() {
            "compile_context" => {
                let request: CompileContextRequest =
                    match serde_json::from_value(tool_call.arguments) {
                        Ok(value) => value,
                        Err(error) => {
                            return JsonRpcResponse::error(
                                id,
                                -32602,
                                format!("invalid compile_context arguments: {error}"),
                            );
                        }
                    };

                match serde_json::to_value(self.compile_context(request).await) {
                    Ok(result) => JsonRpcResponse::ok(id, result),
                    Err(error) => {
                        JsonRpcResponse::error(id, -32603, format!("internal error: {error}"))
                    }
                }
            }
            "find_skill" => {
                let request: FindSkillRequest = match serde_json::from_value(tool_call.arguments) {
                    Ok(value) => value,
                    Err(error) => {
                        return JsonRpcResponse::error(
                            id,
                            -32602,
                            format!("invalid find_skill arguments: {error}"),
                        );
                    }
                };

                match serde_json::to_value(self.find_skill(request).await) {
                    Ok(result) => JsonRpcResponse::ok(id, result),
                    Err(error) => {
                        JsonRpcResponse::error(id, -32603, format!("internal error: {error}"))
                    }
                }
            }
            "extract_session" => {
                let request: ExtractSessionRequest =
                    match serde_json::from_value(tool_call.arguments) {
                        Ok(value) => value,
                        Err(error) => {
                            return JsonRpcResponse::error(
                                id,
                                -32602,
                                format!("invalid extract_session arguments: {error}"),
                            );
                        }
                    };

                match serde_json::to_value(self.extract_session(request).await) {
                    Ok(result) => JsonRpcResponse::ok(id, result),
                    Err(error) => {
                        JsonRpcResponse::error(id, -32603, format!("internal error: {error}"))
                    }
                }
            }
            _ => JsonRpcResponse::error(id, -32601, "tool not found"),
        }
    }
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
