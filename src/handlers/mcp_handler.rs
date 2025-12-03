//! MCP (Model Context Protocol) HTTP handler
//!
//! Exposes the MCP server via HTTP POST for tool discovery and execution.
//! This allows both OpenAI Realtime and ElevenLabs to call our tools.
//!
//! ## Session-Scoped Endpoints
//!
//! For external AI providers (ElevenLabs), we provide session-scoped URLs:
//! `POST /api/mcp/session/{token}` - JSON-RPC endpoint with context bound to the session
//!
//! The token is generated when a call starts and maps to the elder context.

use axum::{
    extract::{Json, Path, State},
    http::StatusCode,
    response::IntoResponse,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::mcp::{McpServer, JsonRpcRequest, ToolContext};
use crate::AppState;

/// Request body for MCP calls with explicit context
#[derive(Debug, Deserialize)]
pub struct McpCallRequest {
    /// The JSON-RPC request
    #[serde(flatten)]
    pub request: JsonRpcRequest,
    /// Call context (required for tool execution on non-session endpoints)
    #[serde(default)]
    pub context: Option<CallContext>,
}

/// Call context for tool execution
#[derive(Debug, Clone, Deserialize)]
pub struct CallContext {
    pub elder_id: Uuid,
    pub session_id: Uuid,
    pub call_sid: String,
}

/// Handle MCP JSON-RPC requests (requires context in body)
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
    
    (StatusCode::OK, Json(response))
}

/// Handle session-scoped MCP requests
/// 
/// The session token is looked up in the call state to get the elder context.
/// This is the endpoint ElevenLabs should use.
#[tracing::instrument(skip(state, request), fields(session_token = %session_token))]
pub async fn handle_session_mcp_request(
    State(state): State<AppState>,
    Path(session_token): Path<String>,
    Json(request): Json<JsonRpcRequest>,
) -> impl IntoResponse {
    tracing::debug!("Session MCP request: {}", request.method);
    
    // Look up session context from token
    let session_ctx = match state.call_state.get_mcp_context(&session_token) {
        Some(ctx) => ctx,
        None => {
            tracing::warn!("Invalid or expired MCP session token: {}", session_token);
            return (
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": null,
                    "error": {
                        "code": -32001,
                        "message": "Invalid or expired session token"
                    }
                })),
            );
        }
    };
    
    let mut server = McpServer::new();
    
    let tool_ctx = ToolContext {
        db: state.db.clone(),
        twilio: state.twilio.clone(),
        uber: state.uber.clone(),
        elder_id: session_ctx.elder_id,
        session_id: session_ctx.session_id,
        call_sid: session_ctx.call_sid,
    };
    
    let response = server.handle_request(request, Some(&tool_ctx)).await;
    
    (StatusCode::OK, Json(serde_json::to_value(response).unwrap_or_default()))
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

/// List available resources
pub async fn list_resources() -> impl IntoResponse {
    #[derive(Serialize)]
    struct ResourcesResponse {
        resources: Vec<crate::mcp::ResourceDefinition>,
    }
    
    let resources = vec![
        crate::mcp::ResourceDefinition {
            uri: "elder://info".to_string(),
            name: "Elder Information".to_string(),
            description: Some("Basic information about the elder".to_string()),
            mime_type: Some("application/json".to_string()),
        },
        crate::mcp::ResourceDefinition {
            uri: "elder://medications".to_string(),
            name: "Medications Schedule".to_string(),
            description: Some("Complete medication list with dosage and schedule".to_string()),
            mime_type: Some("application/json".to_string()),
        },
        crate::mcp::ResourceDefinition {
            uri: "elder://contacts".to_string(),
            name: "Emergency Contacts".to_string(),
            description: Some("Contact list for call transfers".to_string()),
            mime_type: Some("application/json".to_string()),
        },
        crate::mcp::ResourceDefinition {
            uri: "elder://locations".to_string(),
            name: "Saved Locations".to_string(),
            description: Some("Saved locations for ride requests".to_string()),
            mime_type: Some("application/json".to_string()),
        },
    ];
    
    Json(ResourcesResponse { resources })
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
