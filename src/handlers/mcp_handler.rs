//! MCP (Model Context Protocol) HTTP handler
//!
//! Exposes the MCP server via HTTP POST for tool discovery and execution.
//! This allows both OpenAI Realtime and ElevenLabs to call our tools.

use axum::{
    extract::{Json, State},
    http::StatusCode,
    response::IntoResponse,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::mcp::{McpServer, JsonRpcRequest, JsonRpcResponse, ToolContext};
use crate::AppState;

/// Request body for MCP calls with session context
#[derive(Debug, Deserialize)]
pub struct McpCallRequest {
    /// The JSON-RPC request
    #[serde(flatten)]
    pub request: JsonRpcRequest,
    /// Call context (required for tool execution)
    #[serde(default)]
    pub context: Option<CallContext>,
}

/// Call context for tool execution
#[derive(Debug, Deserialize)]
pub struct CallContext {
    pub elder_id: Uuid,
    pub session_id: Uuid,
    pub call_sid: String,
}

/// Handle MCP JSON-RPC requests
#[tracing::instrument(skip(state, request), fields(method = %request.request.method))]
pub async fn handle_mcp_request(
    State(state): State<AppState>,
    Json(request): Json<McpCallRequest>,
) -> impl IntoResponse {
    tracing::debug!("MCP request received");
    
    let mut server = McpServer::new();
    
    // Build tool context if call context is provided
    let tool_ctx = request.context.map(|ctx| {
        ToolContext {
            db: state.db.clone(),
            twilio: state.twilio.clone(),
            uber: state.uber.clone(),
            elder_id: ctx.elder_id,
            session_id: ctx.session_id,
            call_sid: ctx.call_sid,
        }
    });
    
    let response = server.handle_request(request.request, tool_ctx.as_ref()).await;
    
    // Return JSON-RPC response
    (StatusCode::OK, Json(response))
}

/// List available tools (simplified endpoint)
pub async fn list_tools() -> impl IntoResponse {
    let tools = crate::mcp::ToolRegistry::list_tools();
    
    #[derive(Serialize)]
    struct ToolsResponse {
        tools: Vec<crate::mcp::ToolDefinition>,
    }
    
    Json(ToolsResponse { tools })
}

/// Execute a tool directly (simplified endpoint for internal use)
#[derive(Debug, Deserialize)]
pub struct ExecuteToolRequest {
    pub name: String,
    pub arguments: serde_json::Value,
    pub elder_id: Uuid,
    pub session_id: Uuid,
    pub call_sid: String,
}

#[tracing::instrument(skip(state, request), fields(tool = %request.name))]
pub async fn execute_tool(
    State(state): State<AppState>,
    Json(request): Json<ExecuteToolRequest>,
) -> impl IntoResponse {
    tracing::debug!("Direct tool execution");
    
    let ctx = ToolContext {
        db: state.db.clone(),
        twilio: state.twilio.clone(),
        uber: state.uber.clone(),
        elder_id: request.elder_id,
        session_id: request.session_id,
        call_sid: request.call_sid,
    };
    
    let result = crate::mcp::execute_tool(&ctx, &request.name, request.arguments).await;
    
    Json(result)
}

