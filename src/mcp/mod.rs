//! MCP (Model Context Protocol) server implementation
//!
//! This module provides an MCP server that exposes:
//! - **Tools**: request_ride, call_contact
//! - **Resources**: elder://info, elder://medications, elder://contacts, elder://locations
//!
//! Both OpenAI Realtime and ElevenLabs can connect to this server to:
//! - Discover and call tools
//! - Read elder context as resources
//!
//! MCP uses JSON-RPC 2.0 over HTTP.

mod server;
mod tools;
mod types;

pub use server::McpServer;
pub use tools::{ToolRegistry, ToolContext, execute_tool};
pub use types::{
    JsonRpcRequest,
    ToolDefinition, ToolResultContent,
    ResourceDefinition,
};

