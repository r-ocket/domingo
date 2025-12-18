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
use crate::repositories::postgres::{PostgresPool, ContactRepository, LocationRepository};
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
                name: "search_contacts".to_string(),
                description: "Buscar contactos guardados por nombre. Devuelve una lista de candidatos con IDs. Usa call_contact_by_id con el ID elegido.".to_string(),
                input_schema: json!({
                    "type": "object",
                    "additionalProperties": false,
                    "properties": {
                        "query": {
                            "type": "string",
                            "description": "Nombre parcial del contacto (por ejemplo: 'Alejandro')"
                        }
                    },
                    "required": ["query"]
                }),
            },
            ToolDefinition {
                name: "call_contact_by_id".to_string(),
                description: "Transferir la llamada a un contacto guardado usando su contact_id. NUNCA inventes números: el sistema marca el teléfono guardado del contacto.".to_string(),
                input_schema: json!({
                    "type": "object",
                    "additionalProperties": false,
                    "properties": {
                        "contact_id": {
                            "type": "string",
                            "description": "UUID del contacto (de search_contacts)"
                        }
                    },
                    "required": ["contact_id"]
                }),
            },
            ToolDefinition {
                name: "search_locations".to_string(),
                description: "Buscar ubicaciones guardadas por nombre o dirección. Devuelve IDs. Usa request_ride_by_location_id con los IDs elegidos.".to_string(),
                input_schema: json!({
                    "type": "object",
                    "additionalProperties": false,
                    "properties": {
                        "query": {
                            "type": "string",
                            "description": "Nombre o parte de la dirección"
                        },
                        "include_home": {
                            "type": "boolean",
                            "description": "Si true, también incluye CASA en resultados",
                            "default": false
                        }
                    },
                    "required": ["query"]
                }),
            },
            ToolDefinition {
                name: "request_ride_by_location_id".to_string(),
                description: "Solicitar un viaje en Uber usando IDs de ubicaciones guardadas. Esto evita errores por nombres/direcciones no exactas.".to_string(),
                input_schema: json!({
                    "type": "object",
                    "additionalProperties": false,
                    "properties": {
                        "to_location_id": {
                            "type": "string",
                            "description": "UUID del destino (de search_locations)"
                        },
                        "from_location_id": {
                            "type": "string",
                            "description": "UUID opcional del punto de recogida (de search_locations). Si no se pasa, se usa CASA."
                        }
                    },
                    "required": ["to_location_id"]
                }),
            },
            ToolDefinition {
                name: "request_ride".to_string(),
                description: "Solicitar un viaje en Uber. Preferido: usa request_ride_by_location_id. Si usas este tool, pasa el nombre/dirección y el sistema intentará mapearlo a una ubicación guardada. Si hay ambigüedad, NO pedirá el Uber.".to_string(),
                input_schema: json!({
                    "type": "object",
                    "additionalProperties": false,
                    "properties": {
                        "to_location": {
                            "type": "string",
                            "description": "Destino: nombre o dirección guardada"
                        },
                        "from_location": {
                            "type": "string",
                            "description": "Opcional: punto de recogida (nombre o dirección guardada). Si no se especifica, se usa la casa."
                        }
                    },
                    "required": ["to_location"]
                }),
            },
            ToolDefinition {
                name: "call_contact".to_string(),
                description: "Transferir la llamada a un contacto. Preferido: usa search_contacts -> call_contact_by_id. Este tool NO transferirá si el nombre no es una coincidencia única.".to_string(),
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

fn normalize_spanish(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .map(|c| match c {
            'á' | 'à' | 'ä' | 'â' => 'a',
            'é' | 'è' | 'ë' | 'ê' => 'e',
            'í' | 'ì' | 'ï' | 'î' => 'i',
            'ó' | 'ò' | 'ö' | 'ô' => 'o',
            'ú' | 'ù' | 'ü' | 'û' => 'u',
            'ñ' => 'n',
            other => other,
        })
        .filter(|c| c.is_ascii_alphanumeric())
        .collect()
}

fn sanitize_phone_for_dial(raw: &str) -> String {
    let mut out = String::new();
    for (i, c) in raw.chars().enumerate() {
        if c == '+' && i == 0 {
            out.push(c);
        } else if c.is_ascii_digit() {
            out.push(c);
        }
    }
    out
}

/// Execute a tool by name with given arguments
pub async fn execute_tool(
    ctx: &ToolContext,
    tool_name: &str,
    arguments: Value,
) -> CallToolResult {
    match tool_name {
        "search_contacts" => execute_search_contacts(ctx, arguments).await,
        "call_contact_by_id" => execute_call_contact_by_id(ctx, arguments).await,
        "search_locations" => execute_search_locations(ctx, arguments).await,
        "request_ride_by_location_id" => execute_request_ride_by_location_id(ctx, arguments).await,
        "request_ride" => execute_request_ride(ctx, arguments).await,
        "call_contact" => execute_call_contact(ctx, arguments).await,
        _ => CallToolResult::error(format!("Unknown tool: {}", tool_name)),
    }
}

async fn execute_search_contacts(ctx: &ToolContext, args: Value) -> CallToolResult {
    let query = match args.get("query").and_then(|v| v.as_str()) {
        Some(q) => q,
        None => return CallToolResult::error("Missing required parameter: query".to_string()),
    };

    let results = match ContactService::search_contacts(&ctx.db, ctx.elder_id, query).await {
        Ok(r) => r,
        Err(e) => return CallToolResult::error(e.to_string()),
    };

    let items: Vec<_> = results
        .iter()
        .take(8)
        .map(|c| {
            json!({
                "id": c.id,
                "name": c.name,
                "relationship": c.relationship,
                "is_emergency": c.is_emergency
            })
        })
        .collect();

    let result = json!({
        "success": true,
        "query": query,
        "results": items
    });
    CallToolResult::text(serde_json::to_string_pretty(&result).unwrap_or_default())
}

async fn execute_call_contact_by_id(ctx: &ToolContext, args: Value) -> CallToolResult {
    let contact_id = match args.get("contact_id").and_then(|v| v.as_str()) {
        Some(id) => id,
        None => return CallToolResult::error("Missing required parameter: contact_id".to_string()),
    };

    let id = match Uuid::parse_str(contact_id) {
        Ok(u) => u,
        Err(_) => return CallToolResult::error("contact_id must be a UUID".to_string()),
    };

    let contact = match ContactRepository::find_by_id(&ctx.db, id).await {
        Ok(c) => c,
        Err(e) => return CallToolResult::error(e.to_string()),
    };

    if contact.elder_id != ctx.elder_id {
        return CallToolResult::error("contact_id does not belong to this elder".to_string());
    }

    let dial_to = sanitize_phone_for_dial(&contact.phone);
    if dial_to.is_empty() {
        return CallToolResult::error("invalid contact phone".to_string());
    }

    tracing::info!("Transferring call to {} at {}", contact.name, dial_to);
    let twiml = ctx.twilio.generate_dial_twiml(&dial_to);

    match ctx.twilio.update_call(&ctx.call_sid, &twiml).await {
        Ok(_) => {
            let _ = crate::services::CallService::mark_transferred(&ctx.db, ctx.session_id, &contact.name).await;
            let result = json!({
                "success": true,
                "mensaje": format!("Transfiriendo la llamada a {}", contact.name),
                "transferring_to": contact.name,
                "dial_to": dial_to
            });
            CallToolResult::text(serde_json::to_string_pretty(&result).unwrap_or_default())
        }
        Err(e) => CallToolResult::error(format!("No pude transferir la llamada: {}", e)),
    }
}

async fn execute_search_locations(ctx: &ToolContext, args: Value) -> CallToolResult {
    let query = match args.get("query").and_then(|v| v.as_str()) {
        Some(q) => q,
        None => return CallToolResult::error("Missing required parameter: query".to_string()),
    };
    let include_home = args.get("include_home").and_then(|v| v.as_bool()).unwrap_or(false);

    let all = match LocationService::get_all_locations(&ctx.db, ctx.elder_id).await {
        Ok(l) => l,
        Err(e) => return CallToolResult::error(e.to_string()),
    };
    let qn = normalize_spanish(query);

    let mut matches: Vec<_> = all
        .iter()
        .filter(|l| include_home || !l.is_home)
        .filter(|l| {
            let nn = normalize_spanish(&l.name);
            let na = normalize_spanish(&l.address);
            nn.contains(&qn) || na.contains(&qn) || qn.contains(&nn) || qn.contains(&na)
        })
        .map(|l| {
            json!({
                "id": l.id,
                "name": l.name,
                "address": l.address,
                "is_home": l.is_home,
                "location_type": l.location_type.to_string()
            })
        })
        .collect();

    if matches.len() > 12 {
        matches.truncate(12);
    }

    let result = json!({
        "success": true,
        "query": query,
        "results": matches
    });
    CallToolResult::text(serde_json::to_string_pretty(&result).unwrap_or_default())
}

async fn execute_request_ride_by_location_id(ctx: &ToolContext, args: Value) -> CallToolResult {
    let to_id = match args.get("to_location_id").and_then(|v| v.as_str()) {
        Some(id) => id,
        None => return CallToolResult::error("Missing required parameter: to_location_id".to_string()),
    };
    let to_uuid = match Uuid::parse_str(to_id) {
        Ok(u) => u,
        Err(_) => return CallToolResult::error("to_location_id must be a UUID".to_string()),
    };

    let from_uuid_opt = match args.get("from_location_id").and_then(|v| v.as_str()) {
        None => None,
        Some(s) => match Uuid::parse_str(s) {
            Ok(u) => Some(u),
            Err(_) => return CallToolResult::error("from_location_id must be a UUID".to_string()),
        },
    };

    // Resolve locations by ID (and verify elder ownership)
    let destination = match LocationRepository::find_by_id(&ctx.db, to_uuid).await {
        Ok(l) => l,
        Err(e) => return CallToolResult::error(e.to_string()),
    };
    if destination.elder_id != ctx.elder_id {
        return CallToolResult::error("to_location_id does not belong to this elder".to_string());
    }

    let pickup = match from_uuid_opt {
        Some(pid) => {
            let l = match LocationRepository::find_by_id(&ctx.db, pid).await {
                Ok(l) => l,
                Err(e) => return CallToolResult::error(e.to_string()),
            };
            if l.elder_id != ctx.elder_id {
                return CallToolResult::error("from_location_id does not belong to this elder".to_string());
            }
            l
        }
        None => match LocationRepository::find_home(&ctx.db, ctx.elder_id).await {
            Ok(Some(h)) => h,
            Ok(None) => return CallToolResult::error("No hay dirección de casa configurada.".to_string()),
            Err(e) => return CallToolResult::error(e.to_string()),
        },
    };

    // Delegate to existing service using canonical names (keeps DB ride logging consistent).
    match RideService::book_ride_flexible(
        &ctx.db,
        &ctx.uber,
        ctx.elder_id,
        Some(&pickup.name),
        &destination.name,
    ).await {
        Ok(ride_info) => {
            let result = json!({
                "success": true,
                "mensaje": format!("He pedido un Uber de {} a {}", ride_info.pickup_name, ride_info.destination_name),
                "status": ride_info.status.to_string(),
                "driver": ride_info.driver_name,
                "vehicle": ride_info.vehicle_description,
                "eta_minutes": ride_info.eta_minutes,
                "pickup_location_id": pickup.id,
                "to_location_id": destination.id
            });
            CallToolResult::text(serde_json::to_string_pretty(&result).unwrap_or_default())
        }
        Err(e) => CallToolResult::error(e.to_string()),
    }
}

/// Execute the request_ride tool
async fn execute_request_ride(ctx: &ToolContext, args: Value) -> CallToolResult {
    let to_location = match args.get("to_location").and_then(|v| v.as_str()) {
        Some(loc) => loc,
        None => return CallToolResult::error("Missing required parameter: to_location".to_string()),
    };
    let from_location = args.get("from_location").and_then(|v| v.as_str());

    // Wrapper: map strings -> ids via search_locations, then call request_ride_by_location_id if unambiguous.
    let include_home = from_location.is_some();
    let search_args = json!({"query": to_location, "include_home": include_home});
    let search_res = execute_search_locations(ctx, search_args).await;
    let parsed: serde_json::Value = match search_res.content.first() {
        Some(crate::mcp::ToolResultContent::Text { text }) => serde_json::from_str(text).unwrap_or(json!({})),
        _ => json!({}),
    };
    let results = parsed.get("results").and_then(|v| v.as_array()).cloned().unwrap_or_default();
    if results.len() != 1 {
        let msg = json!({
            "success": false,
            "error": {
                "code": "USE_ID_BASED_FLOW",
                "message": "usa search_locations y luego request_ride_by_location_id con el ID exacto para evitar errores."
            },
            "matches": results
        });
        return CallToolResult {
            content: vec![crate::mcp::ToolResultContent::Text { text: serde_json::to_string_pretty(&msg).unwrap_or_default() }],
            is_error: Some(true),
        };
    }
    let to_id = results[0].get("id").cloned().unwrap_or(json!(null));
    let mut args2 = json!({"to_location_id": to_id});
    if let Some(from) = from_location {
        // best-effort: also try to resolve pickup via search_locations (include_home=true)
        let search_from = execute_search_locations(ctx, json!({"query": from, "include_home": true})).await;
        let parsed_from: serde_json::Value = match search_from.content.first() {
            Some(crate::mcp::ToolResultContent::Text { text }) => serde_json::from_str(text).unwrap_or(json!({})),
            _ => json!({}),
        };
        let from_results = parsed_from.get("results").and_then(|v| v.as_array()).cloned().unwrap_or_default();
        if from_results.len() == 1 {
            if let Some(fid) = from_results[0].get("id") {
                args2["from_location_id"] = fid.clone();
            }
        }
    }
    execute_request_ride_by_location_id(ctx, args2).await
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

    // Wrapper: use search_contacts then call_contact_by_id when unambiguous.
    let search = execute_search_contacts(ctx, json!({"query": query})).await;
    let parsed: serde_json::Value = match search.content.first() {
        Some(crate::mcp::ToolResultContent::Text { text }) => serde_json::from_str(text).unwrap_or(json!({})),
        _ => json!({}),
    };
    let results = parsed.get("results").and_then(|v| v.as_array()).cloned().unwrap_or_default();
    if results.len() != 1 {
        let msg = json!({
            "success": false,
            "error": {
                "code": "USE_ID_BASED_FLOW",
                "message": "usa search_contacts y luego call_contact_by_id con el ID exacto para evitar transferencias equivocadas."
            },
            "matches": results
        });
        return CallToolResult {
            content: vec![crate::mcp::ToolResultContent::Text { text: serde_json::to_string_pretty(&msg).unwrap_or_default() }],
            is_error: Some(true),
        };
    }
    let cid = results[0].get("id").cloned().unwrap_or(json!(null));
    execute_call_contact_by_id(ctx, json!({"contact_id": cid})).await
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

