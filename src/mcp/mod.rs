//! MCP (Model Context Protocol) server implementation
//!
//! This module provides an MCP server that exposes tools for both
//! OpenAI Realtime and ElevenLabs Conversational AI to use.
//!
//! MCP uses JSON-RPC 2.0 over various transports. We implement:
//! - HTTP transport for web-based integrations
//! - The standard MCP protocol messages

mod server;
mod tools;
mod types;

pub use server::McpServer;
pub use tools::{ToolRegistry, ToolContext, execute_tool};
pub use types::{
    JsonRpcRequest, JsonRpcResponse,
    ToolDefinition, CallToolResult, ToolResultContent,
};

