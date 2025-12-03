//! MCP Server implementation
//!
//! Handles JSON-RPC 2.0 requests for the Model Context Protocol

use serde_json::{json, Value};

use super::types::*;
use super::tools::{ToolRegistry, ToolContext, execute_tool};

/// MCP Server that processes JSON-RPC requests
pub struct McpServer {
    initialized: bool,
}

impl McpServer {
    pub fn new() -> Self {
        Self { initialized: false }
    }

    /// Handle a JSON-RPC request and return a response
    pub async fn handle_request(
        &mut self,
        request: JsonRpcRequest,
        tool_ctx: Option<&ToolContext>,
    ) -> JsonRpcResponse {
        match request.method.as_str() {
            "initialize" => self.handle_initialize(request),
            "initialized" => self.handle_initialized(request),
            "tools/list" => self.handle_list_tools(request),
            "tools/call" => self.handle_call_tool(request, tool_ctx).await,
            "ping" => self.handle_ping(request),
            _ => JsonRpcResponse::error(
                request.id,
                METHOD_NOT_FOUND,
                format!("Method not found: {}", request.method),
            ),
        }
    }

    /// Handle initialize request
    fn handle_initialize(&mut self, request: JsonRpcRequest) -> JsonRpcResponse {
        // Parse initialize params (optional validation)
        if let Some(params) = &request.params {
            if let Ok(_init_params) = serde_json::from_value::<InitializeParams>(params.clone()) {
                tracing::debug!("MCP client initializing");
            }
        }

        let result = InitializeResult {
            protocol_version: "2024-11-05".to_string(),
            capabilities: ServerCapabilities {
                tools: Some(ToolsCapability { list_changed: false }),
                resources: None,
                prompts: None,
            },
            server_info: ServerInfo {
                name: "domingo-mcp".to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
            },
        };

        JsonRpcResponse::success(
            request.id,
            serde_json::to_value(result).unwrap_or(Value::Null),
        )
    }

    /// Handle initialized notification
    fn handle_initialized(&mut self, request: JsonRpcRequest) -> JsonRpcResponse {
        self.initialized = true;
        tracing::info!("MCP server initialized");
        
        // Notifications don't get responses, but we return success for consistency
        JsonRpcResponse::success(request.id, Value::Null)
    }

    /// Handle tools/list request
    fn handle_list_tools(&self, request: JsonRpcRequest) -> JsonRpcResponse {
        let tools = ToolRegistry::list_tools();
        let result = ListToolsResult { tools };
        
        JsonRpcResponse::success(
            request.id,
            serde_json::to_value(result).unwrap_or(Value::Null),
        )
    }

    /// Handle tools/call request
    async fn handle_call_tool(
        &self,
        request: JsonRpcRequest,
        tool_ctx: Option<&ToolContext>,
    ) -> JsonRpcResponse {
        // Parse call params
        let params: CallToolParams = match request.params {
            Some(p) => match serde_json::from_value(p) {
                Ok(params) => params,
                Err(e) => {
                    return JsonRpcResponse::error(
                        request.id,
                        INVALID_PARAMS,
                        format!("Invalid params: {}", e),
                    );
                }
            },
            None => {
                return JsonRpcResponse::error(
                    request.id,
                    INVALID_PARAMS,
                    "Missing params".to_string(),
                );
            }
        };

        // Check if we have tool context
        let ctx = match tool_ctx {
            Some(c) => c,
            None => {
                return JsonRpcResponse::error(
                    request.id,
                    INTERNAL_ERROR,
                    "Tool context not available".to_string(),
                );
            }
        };

        // Execute the tool
        let result = execute_tool(ctx, &params.name, params.arguments).await;
        
        JsonRpcResponse::success(
            request.id,
            serde_json::to_value(result).unwrap_or(Value::Null),
        )
    }

    /// Handle ping request (for health checks)
    fn handle_ping(&self, request: JsonRpcRequest) -> JsonRpcResponse {
        JsonRpcResponse::success(request.id, json!({}))
    }

    /// Parse a JSON-RPC request from raw JSON
    pub fn parse_request(json_str: &str) -> Result<JsonRpcRequest, JsonRpcResponse> {
        serde_json::from_str(json_str).map_err(|e| {
            JsonRpcResponse::error(
                RequestId::Null,
                PARSE_ERROR,
                format!("Parse error: {}", e),
            )
        })
    }
}

impl Default for McpServer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_request() {
        let json = r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#;
        let request = McpServer::parse_request(json).unwrap();
        assert_eq!(request.method, "tools/list");
        assert_eq!(request.id, RequestId::Number(1));
    }

    #[test]
    fn test_list_tools() {
        let tools = ToolRegistry::list_tools();
        assert_eq!(tools.len(), 2);
        assert_eq!(tools[0].name, "request_ride");
        assert_eq!(tools[1].name, "call_contact");
    }
}

