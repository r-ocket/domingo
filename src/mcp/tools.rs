//! MCP Tool definitions and execution
//!
//! Defines the tools available through the MCP server:
//! - request_ride: Request an Uber ride
//! - call_contact: Transfer the call to a contact
//!
//! Also provides resource access for elder context:
//! - elder://info - Basic elder info
//! - elder://medications - Medication schedule
//! - elder://contacts - Contact list
//! - elder://locations - Saved locations

use std::sync::Arc;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::domain::Pagination;
use crate::repositories::postgres::PostgresPool;
use crate::clients::{TwilioClient, UberClient};
use crate::services::{ContactService, ElderService, LocationService, MedicationService, RideService};
use super::types::{ToolDefinition, CallToolResult, ResourceContent};

/// Pagination that returns all results (for MCP resource reads)
fn all_results() -> Pagination {
    Pagination { page: 1, per_page: 1000 }
}

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
                    "additionalProperties": false,
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
                    "additionalProperties": false,
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
            let result = json!({
                "success": false,
                "error": {
                    "code": "RIDE_BOOK_FAILED",
                    "message": e.to_string()
                }
            });
            CallToolResult {
                content: vec![crate::mcp::ToolResultContent::Text { text: serde_json::to_string_pretty(&result).unwrap_or_default() }],
                is_error: Some(true),
            }
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
        Ok(contacts) if contacts.len() > 1 => {
            let matches: Vec<_> = contacts.iter().take(5).map(|c| {
                json!({
                    "name": c.name,
                    "relationship": c.relationship,
                    "is_emergency": c.is_emergency
                })
            }).collect();

            let result = json!({
                "success": false,
                "error": {
                    "code": "CONTACT_AMBIGUOUS",
                    "message": format!("Encontré varios contactos que coinciden con '{}'. Pídele al usuario que confirme el nombre EXACTO.", query)
                },
                "matches": matches
            });
            return CallToolResult {
                content: vec![crate::mcp::ToolResultContent::Text { text: serde_json::to_string_pretty(&result).unwrap_or_default() }],
                is_error: Some(true),
            };
        }
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
                    let result = json!({
                        "success": false,
                        "error": {
                            "code": "TRANSFER_FAILED",
                            "message": format!("No pude transferir la llamada: {}", e)
                        }
                    });
                    CallToolResult {
                        content: vec![crate::mcp::ToolResultContent::Text { text: serde_json::to_string_pretty(&result).unwrap_or_default() }],
                        is_error: Some(true),
                    }
                }
            }
        }
        Ok(_) => {
            let result = json!({
                "success": false,
                "error": {
                    "code": "CONTACT_NOT_FOUND",
                    "message": format!(
                        "No encontré un contacto llamado '{}'. Revisa los nombres en la lista de contactos.",
                        query
                    )
                }
            });
            CallToolResult {
                content: vec![crate::mcp::ToolResultContent::Text { text: serde_json::to_string_pretty(&result).unwrap_or_default() }],
                is_error: Some(true),
            }
        }
        Err(e) => {
            let result = json!({
                "success": false,
                "error": {
                    "code": "CONTACT_SEARCH_FAILED",
                    "message": e.to_string()
                }
            });
            CallToolResult {
                content: vec![crate::mcp::ToolResultContent::Text { text: serde_json::to_string_pretty(&result).unwrap_or_default() }],
                is_error: Some(true),
            }
        }
    }
}

// ============================================================================
// Resource Reading
// ============================================================================

/// Read an MCP resource by URI
pub async fn read_resource(ctx: &ToolContext, uri: &str) -> Result<ResourceContent, String> {
    match uri {
        "elder://info" => read_elder_info(ctx).await,
        "elder://medications" => read_medications(ctx).await,
        "elder://contacts" => read_contacts(ctx).await,
        "elder://locations" => read_locations(ctx).await,
        _ => Err(format!("Unknown resource: {}", uri)),
    }
}

/// Read elder basic info
async fn read_elder_info(ctx: &ToolContext) -> Result<ResourceContent, String> {
    // We only need the bypass for fetching our own elder data
    let elder = ElderService::get_elder(&ctx.db, ctx.elder_id, Uuid::nil(), true)
        .await
        .map_err(|e| format!("Failed to fetch elder: {}", e))?;
    
    let info = json!({
        "name": elder.name,
        "phone": elder.phone_number,
    });
    
    Ok(ResourceContent {
        uri: "elder://info".to_string(),
        mime_type: Some("application/json".to_string()),
        text: Some(serde_json::to_string_pretty(&info).unwrap_or_default()),
        blob: None,
    })
}

/// Read medications schedule
async fn read_medications(ctx: &ToolContext) -> Result<ResourceContent, String> {
    // Use admin bypass to fetch elder's medications during call
    let paginated = MedicationService::list_medications(&ctx.db, ctx.elder_id, Uuid::nil(), true, &all_results())
        .await
        .map_err(|e| format!("Failed to fetch medications: {}", e))?;
    let medications = paginated.items;
    
    let meds: Vec<_> = medications.iter().map(|m| {
        // Format schedules as readable strings
        let schedules: Vec<_> = m.schedules.iter().map(|s| {
            json!({
                "hora": s.time_of_day.format("%H:%M").to_string(),
                "dias": s.days_description(),
            })
        }).collect();
        
        json!({
            "nombre": m.medication.name,
            "dosis": m.medication.dosage,
            "instrucciones": m.medication.instructions,
            "horarios": schedules,
        })
    }).collect();
    
    let content = json!({
        "medicamentos": meds,
        "total": medications.len(),
    });
    
    Ok(ResourceContent {
        uri: "elder://medications".to_string(),
        mime_type: Some("application/json".to_string()),
        text: Some(serde_json::to_string_pretty(&content).unwrap_or_default()),
        blob: None,
    })
}

/// Read contacts list
async fn read_contacts(ctx: &ToolContext) -> Result<ResourceContent, String> {
    // Use admin bypass to fetch elder's contacts during call
    let paginated = ContactService::list_contacts(&ctx.db, ctx.elder_id, Uuid::nil(), true, &all_results())
        .await
        .map_err(|e| format!("Failed to fetch contacts: {}", e))?;
    let contacts = paginated.items;
    
    let contacts_json: Vec<_> = contacts.iter().map(|c| {
        json!({
            "nombre": c.name,
            "relacion": c.relationship,
            "es_emergencia": c.is_emergency,
        })
    }).collect();
    
    let content = json!({
        "contactos": contacts_json,
        "total": contacts.len(),
    });
    
    Ok(ResourceContent {
        uri: "elder://contacts".to_string(),
        mime_type: Some("application/json".to_string()),
        text: Some(serde_json::to_string_pretty(&content).unwrap_or_default()),
        blob: None,
    })
}

/// Read saved locations
async fn read_locations(ctx: &ToolContext) -> Result<ResourceContent, String> {
    // Use admin bypass to fetch elder's locations during call
    let paginated = LocationService::list_locations(&ctx.db, ctx.elder_id, Uuid::nil(), true, &all_results())
        .await
        .map_err(|e| format!("Failed to fetch locations: {}", e))?;
    let locations = paginated.items;
    
    let locs: Vec<_> = locations.iter().map(|l| {
        json!({
            "nombre": l.name,
            "tipo": format!("{:?}", l.location_type),
            "direccion": l.address,
            "es_casa": l.is_home,
        })
    }).collect();
    
    let content = json!({
        "ubicaciones": locs,
        "total": locations.len(),
    });
    
    Ok(ResourceContent {
        uri: "elder://locations".to_string(),
        mime_type: Some("application/json".to_string()),
        text: Some(serde_json::to_string_pretty(&content).unwrap_or_default()),
        blob: None,
    })
}

