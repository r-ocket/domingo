//! MCP Tool definitions and execution
//!
//! Defines the tools available through the MCP server:
//! - request_ride: Request an Uber ride
//! - call_contact: Transfer the call to a contact

use std::sync::Arc;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::repositories::postgres::PostgresPool;
use crate::clients::{TwilioClient, UberClient};
use crate::services::{ContactService, RideService};
use super::types::{ToolDefinition, CallToolResult};

/// Context needed to execute tools
#[derive(Clone)]
pub struct ToolContext {
    pub db: PostgresPool,
    pub twilio: Arc<TwilioClient>,
    pub uber: Arc<UberClient>,
    /// Current elder ID for the call
    pub elder_id: Uuid,
    /// Current call session ID
    pub session_id: Uuid,
    /// Twilio call SID for transfers
    pub call_sid: String,
}

/// Registry of available tools
pub struct ToolRegistry;

impl ToolRegistry {
    /// Get all tool definitions for MCP
    pub fn list_tools() -> Vec<ToolDefinition> {
        vec![
            ToolDefinition {
                name: "request_ride".to_string(),
                description: "Solicitar un viaje en Uber. Usa los nombres de ubicaciones que están en el contexto.".to_string(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "to_location": {
                            "type": "string",
                            "description": "El nombre del destino (debe ser una ubicación guardada del contexto)"
                        },
                        "from_location": {
                            "type": "string",
                            "description": "Opcional: punto de recogida. Si no se especifica, se usa la casa."
                        }
                    },
                    "required": ["to_location"]
                }),
            },
            ToolDefinition {
                name: "call_contact".to_string(),
                description: "Transferir la llamada a un contacto. Usa el nombre exacto del contacto que aparece en el contexto.".to_string(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "contact_name": {
                            "type": "string",
                            "description": "El nombre EXACTO del contacto como aparece en la lista de contactos del contexto"
                        }
                    },
                    "required": ["contact_name"]
                }),
            },
        ]
    }
}

/// Execute a tool by name with given arguments
pub async fn execute_tool(
    ctx: &ToolContext,
    tool_name: &str,
    arguments: Value,
) -> CallToolResult {
    match tool_name {
        "request_ride" => execute_request_ride(ctx, arguments).await,
        "call_contact" => execute_call_contact(ctx, arguments).await,
        _ => CallToolResult::error(format!("Unknown tool: {}", tool_name)),
    }
}

/// Execute the request_ride tool
async fn execute_request_ride(ctx: &ToolContext, args: Value) -> CallToolResult {
    let to_location = match args.get("to_location").and_then(|v| v.as_str()) {
        Some(loc) => loc,
        None => return CallToolResult::error("Missing required parameter: to_location".to_string()),
    };
    let from_location = args.get("from_location").and_then(|v| v.as_str());

    match RideService::book_ride_flexible(
        &ctx.db,
        &ctx.uber,
        ctx.elder_id,
        from_location,
        to_location,
    ).await {
        Ok(ride_info) => {
            let result = json!({
                "success": true,
                "mensaje": format!("He pedido un Uber de {} a {}", 
                    ride_info.pickup_name, 
                    ride_info.destination_name),
                "status": ride_info.status.to_string(),
                "driver": ride_info.driver_name,
                "vehicle": ride_info.vehicle_description,
                "eta_minutes": ride_info.eta_minutes,
            });
            CallToolResult::text(serde_json::to_string_pretty(&result).unwrap_or_default())
        }
        Err(e) => {
            let result = json!({ "error": e.to_string() });
            CallToolResult::text(serde_json::to_string_pretty(&result).unwrap_or_default())
        }
    }
}

/// Execute the call_contact tool
async fn execute_call_contact(ctx: &ToolContext, args: Value) -> CallToolResult {
    // Support both "contact_name" (new) and "name_or_relationship" (old)
    let query = args.get("contact_name")
        .or_else(|| args.get("name_or_relationship"))
        .and_then(|v| v.as_str());
    
    let query = match query {
        Some(q) => q,
        None => return CallToolResult::error("Missing required parameter: contact_name".to_string()),
    };

    match ContactService::search_contacts(&ctx.db, ctx.elder_id, query).await {
        Ok(contacts) if !contacts.is_empty() => {
            let contact = &contacts[0];
            tracing::info!("Transferring call to {} at {}", contact.name, contact.phone);

            // Generate TwiML for call transfer
            let twiml = ctx.twilio.generate_dial_twiml(&contact.phone);

            match ctx.twilio.update_call(&ctx.call_sid, &twiml).await {
                Ok(_) => {
                    // Mark session as transferred
                    let _ = crate::services::CallService::mark_transferred(
                        &ctx.db,
                        ctx.session_id,
                        &contact.name,
                    ).await;

                    let result = json!({
                        "success": true,
                        "mensaje": format!("Transfiriendo la llamada a {}", contact.name),
                        "transferring_to": contact.name,
                    });
                    CallToolResult::text(serde_json::to_string_pretty(&result).unwrap_or_default())
                }
                Err(e) => {
                    let result = json!({ "error": format!("No pude transferir la llamada: {}", e) });
                    CallToolResult::text(serde_json::to_string_pretty(&result).unwrap_or_default())
                }
            }
        }
        Ok(_) => {
            let result = json!({ 
                "error": format!(
                    "No encontré un contacto llamado '{}'. Revisa los nombres en la lista de contactos.",
                    query
                )
            });
            CallToolResult::text(serde_json::to_string_pretty(&result).unwrap_or_default())
        }
        Err(e) => {
            let result = json!({ "error": e.to_string() });
            CallToolResult::text(serde_json::to_string_pretty(&result).unwrap_or_default())
        }
    }
}

