//! WebSocket handler for Twilio media stream and OpenAI Realtime relay

use std::sync::Arc;
use axum::{
    extract::{Path, State, WebSocketUpgrade},
    response::IntoResponse,
};
use futures_util::{SinkExt, StreamExt};
use serde_json::json;
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::clients::{
    TwilioStreamMessage, TwilioOutboundMedia,
    RealtimeServerEvent, build_assistant_tools, SYSTEM_PROMPT,
};
use crate::domain::CallStatus;
use crate::services::{
    CallService, ContactService, ElderService, LocationService,
    MedicationService, RideService,
};
use crate::AppState;

/// Handle Twilio media stream WebSocket connection
pub async fn media_stream(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
    Path(call_sid): Path<String>,
) -> impl IntoResponse {
    tracing::info!("Media stream connection for call {}", call_sid);
    
    ws.on_upgrade(move |socket| handle_media_stream(socket, state, call_sid))
}

async fn handle_media_stream(
    socket: axum::extract::ws::WebSocket,
    state: AppState,
    call_sid: String,
) {
    let (mut ws_sender, mut ws_receiver) = socket.split();
    
    // Get call session and elder
    let session = match CallService::get_session_by_sid(&state.db, &call_sid).await {
        Ok(s) => s,
        Err(e) => {
            tracing::error!("Failed to get call session: {}", e);
            return;
        }
    };
    
    let elder = match ElderService::get_elder(&state.db, session.elder_id, Uuid::nil(), true).await {
        Ok(e) => e,
        Err(e) => {
            tracing::error!("Failed to get elder: {}", e);
            return;
        }
    };
    
    tracing::info!("Starting voice session for elder {} ({})", elder.name, elder.id);
    
    // Connect to OpenAI Realtime
    let openai_session = match state.openai.connect_realtime(SYSTEM_PROMPT, build_assistant_tools()).await {
        Ok(s) => s,
        Err(e) => {
            tracing::error!("Failed to connect to OpenAI Realtime: {}", e);
            return;
        }
    };
    
    // Track stream SID for sending audio back
    let stream_sid = Arc::new(tokio::sync::RwLock::new(String::new()));
    let stream_sid_clone = stream_sid.clone();
    
    // Channel for sending audio to Twilio
    let (twilio_tx, mut twilio_rx) = mpsc::channel::<String>(100);
    
    // Spawn task to handle outbound audio to Twilio
    let stream_sid_for_sender = stream_sid.clone();
    tokio::spawn(async move {
        while let Some(audio_base64) = twilio_rx.recv().await {
            let sid = stream_sid_for_sender.read().await.clone();
            if !sid.is_empty() {
                let msg = TwilioOutboundMedia::new(&sid, &audio_base64);
                if let Ok(json) = serde_json::to_string(&msg) {
                    if ws_sender.send(axum::extract::ws::Message::Text(json)).await.is_err() {
                        break;
                    }
                }
            }
        }
    });
    
    // Clone state for the OpenAI handler
    let state_clone = state.clone();
    let elder_id = elder.id;
    let session_id = session.id;
    let call_sid_clone = call_sid.clone();
    
    // Spawn task to handle OpenAI responses
    let mut openai_session = openai_session;
    let twilio_tx_clone = twilio_tx.clone();
    tokio::spawn(async move {
        while let Some(event) = openai_session.recv_event().await {
            match event {
                RealtimeServerEvent::ResponseAudioDelta { delta } => {
                    // Send audio to Twilio
                    let _ = twilio_tx_clone.send(delta).await;
                }
                RealtimeServerEvent::ResponseFunctionCallArgumentsDone { call_id, name, arguments } => {
                    tracing::info!("Function call: {} with args: {}", name, arguments);
                    
                    // Execute the function
                    let result = execute_tool(
                        &state_clone,
                        elder_id,
                        &name,
                        &arguments,
                        session_id,
                        &call_sid_clone,
                    ).await;
                    
                    // Record tool usage
                    let _ = CallService::record_tool_usage(&state_clone.db, session_id, &name).await;
                    
                    // Send result back to OpenAI
                    if let Err(e) = openai_session.send_function_result(&call_id, result).await {
                        tracing::error!("Failed to send function result: {}", e);
                    }
                }
                RealtimeServerEvent::Error { error } => {
                    tracing::error!("OpenAI error: {} - {}", error.r#type, error.message);
                }
                _ => {}
            }
        }
    });
    
    // Handle incoming Twilio messages
    while let Some(msg) = ws_receiver.next().await {
        match msg {
            Ok(axum::extract::ws::Message::Text(text)) => {
                if let Ok(twilio_msg) = serde_json::from_str::<TwilioStreamMessage>(&text) {
                    match twilio_msg {
                        TwilioStreamMessage::Start { stream_sid: sid, start } => {
                            *stream_sid_clone.write().await = sid;
                            tracing::info!("Stream started for call {}", start.call_sid);
                        }
                        TwilioStreamMessage::Media { .. } => {
                            // Forward audio to OpenAI
                            // Note: Twilio sends mulaw 8kHz, OpenAI expects pcm16 24kHz
                            // In production, we'd need to resample
                            // For now, send as-is (OpenAI may handle some formats)
                            // TODO: Audio format conversion
                        }
                        TwilioStreamMessage::Stop { .. } => {
                            tracing::info!("Stream stopped");
                            break;
                        }
                        _ => {}
                    }
                }
            }
            Ok(axum::extract::ws::Message::Close(_)) => {
                tracing::info!("WebSocket closed");
                break;
            }
            Err(e) => {
                tracing::error!("WebSocket error: {}", e);
                break;
            }
            _ => {}
        }
    }
    
    // End the call session
    let _ = CallService::end_session(
        &state.db,
        session.id,
        CallStatus::Completed,
        None,
        None,
    ).await;
    
    tracing::info!("Voice session ended for call {}", call_sid);
}

/// Execute a tool call from the AI
async fn execute_tool(
    state: &AppState,
    elder_id: Uuid,
    tool_name: &str,
    arguments: &str,
    session_id: Uuid,
    call_sid: &str,
) -> serde_json::Value {
    let args: serde_json::Value = serde_json::from_str(arguments).unwrap_or_default();
    
    match tool_name {
        "get_saved_locations" => {
            match LocationService::get_all_locations(&state.db, elder_id).await {
                Ok(locations) => {
                    let location_list: Vec<_> = locations.iter()
                        .map(|l| json!({
                            "name": l.name,
                            "address": l.address,
                            "is_home": l.is_home,
                        }))
                        .collect();
                    json!({ "locations": location_list })
                }
                Err(e) => json!({ "error": e.to_string() }),
            }
        }
        
        "request_ride" => {
            let location_name = args["location_name"].as_str().unwrap_or("");
            
            match RideService::book_ride_by_name(&state.db, &state.uber, elder_id, location_name).await {
                Ok(ride_info) => {
                    json!({
                        "success": true,
                        "destination": ride_info.destination_name,
                        "status": ride_info.status.to_string(),
                        "driver": ride_info.driver_name,
                        "vehicle": ride_info.vehicle_description,
                        "eta_minutes": ride_info.eta_minutes,
                    })
                }
                Err(e) => json!({ "error": e.to_string() }),
            }
        }
        
        "get_upcoming_medications" => {
            match MedicationService::get_upcoming_medications(&state.db, elder_id).await {
                Ok(meds) => {
                    let med_list: Vec<_> = meds.iter()
                        .map(|m| json!({
                            "name": m.medication_name,
                            "dosage": m.dosage,
                            "time": m.scheduled_time.format("%H:%M").to_string(),
                            "instructions": m.instructions,
                        }))
                        .collect();
                    json!({ "upcoming_medications": med_list })
                }
                Err(e) => json!({ "error": e.to_string() }),
            }
        }
        
        "get_medication_schedule" => {
            match MedicationService::get_all_medications(&state.db, elder_id).await {
                Ok(meds) => {
                    let schedule: Vec<_> = meds.iter()
                        .map(|m| json!({
                            "name": m.medication.name,
                            "dosage": m.medication.dosage,
                            "instructions": m.medication.instructions,
                            "times": m.schedules.iter()
                                .map(|s| s.time_of_day.format("%H:%M").to_string())
                                .collect::<Vec<_>>(),
                        }))
                        .collect();
                    json!({ "medication_schedule": schedule })
                }
                Err(e) => json!({ "error": e.to_string() }),
            }
        }
        
        "get_contact_info" => {
            let query = args["name_or_relationship"].as_str().unwrap_or("");
            
            match ContactService::search_contacts(&state.db, elder_id, query).await {
                Ok(contacts) if !contacts.is_empty() => {
                    let contact = &contacts[0];
                    json!({
                        "name": contact.name,
                        "relationship": contact.relationship,
                        "phone": contact.phone,
                        "notes": contact.notes,
                    })
                }
                Ok(_) => json!({ "error": format!("No contact found matching '{}'", query) }),
                Err(e) => json!({ "error": e.to_string() }),
            }
        }
        
        "call_contact" => {
            let query = args["name_or_relationship"].as_str().unwrap_or("");
            
            match ContactService::search_contacts(&state.db, elder_id, query).await {
                Ok(contacts) if !contacts.is_empty() => {
                    let contact = &contacts[0];
                    
                    // Update call to transfer
                    let twiml = state.twilio.generate_dial_twiml(&contact.phone);
                    
                    match state.twilio.update_call(call_sid, &twiml).await {
                        Ok(_) => {
                            // Mark session as transferred
                            let _ = CallService::mark_transferred(
                                &state.db,
                                session_id,
                                &contact.name,
                            ).await;
                            
                            json!({
                                "success": true,
                                "transferring_to": contact.name,
                                "phone": contact.phone,
                            })
                        }
                        Err(e) => json!({ "error": format!("Failed to transfer call: {}", e) }),
                    }
                }
                Ok(_) => json!({ "error": format!("No contact found matching '{}'", query) }),
                Err(e) => json!({ "error": e.to_string() }),
            }
        }
        
        _ => json!({ "error": format!("Unknown tool: {}", tool_name) }),
    }
}

