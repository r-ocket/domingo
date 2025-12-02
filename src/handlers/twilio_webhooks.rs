//! Twilio webhook handlers

use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::IntoResponse,
    Form,
};
use serde::Deserialize;

use crate::clients::{IncomingCallPayload, CallStatusPayload};
use crate::domain::CallStatus;
use crate::services::{CallService, ElderService};
use crate::AppState;

/// Handle incoming voice call from Twilio
#[tracing::instrument(skip(state), fields(call_sid = %payload.call_sid, from = %payload.from))]
pub async fn incoming_call(
    State(state): State<AppState>,
    Form(payload): Form<IncomingCallPayload>,
) -> impl IntoResponse {
    tracing::info!("Processing incoming call");
    
    // Look up elder by phone number
    match ElderService::get_elder_by_phone(&state.db, &payload.from).await {
        Ok(elder) => {
            // Create call session
            if let Err(e) = CallService::start_session(
                &state.db,
                elder.id,
                &payload.call_sid,
                &payload.from,
            ).await {
                tracing::error!("Failed to create call session: {}", e);
            }
            
            // Generate TwiML to stream audio to our WebSocket
            let stream_url = format!(
                "wss://{}/api/twilio/media-stream/{}",
                state.config.base_url.replace("http://", "").replace("https://", ""),
                payload.call_sid
            );
            
            let twiml = state.twilio.generate_stream_twiml(&stream_url);
            
            (
                StatusCode::OK,
                [("Content-Type", "application/xml")],
                twiml,
            )
        }
        Err(_) => {
            // Unknown caller - play error message
            tracing::warn!("Unknown caller: {}", payload.from);
            
            let twiml = state.twilio.generate_error_twiml(
                "Lo siento, no reconozco este número de teléfono. Por favor contacta a tu cuidador para configurar tu cuenta."
            );
            
            (
                StatusCode::OK,
                [("Content-Type", "application/xml")],
                twiml,
            )
        }
    }
}

/// Handle call status callback from Twilio
#[tracing::instrument(skip(state), fields(call_sid = %payload.call_sid, status = %payload.call_status))]
pub async fn call_status(
    State(state): State<AppState>,
    Form(payload): Form<CallStatusPayload>,
) -> impl IntoResponse {
    tracing::info!("Processing call status update");
    
    // Update call session if call completed
    if payload.call_status == "completed" || payload.call_status == "failed" {
        if let Ok(session) = CallService::get_session_by_sid(&state.db, &payload.call_sid).await {
            let status = if payload.call_status == "completed" {
                CallStatus::Completed
            } else {
                CallStatus::Failed
            };
            
            if let Err(e) = CallService::end_session(
                &state.db,
                session.id,
                status,
                None,
                None,
            ).await {
                tracing::error!("Failed to update call session: {}", e);
            }
        }
    }
    
    StatusCode::OK
}

/// Generate TwiML for medication reminder calls
#[tracing::instrument(skip(state), fields(reminder_id = ?params.reminder_id, medication = ?params.med))]
pub async fn reminder_twiml(
    State(state): State<AppState>,
    Query(params): Query<ReminderTwimlParams>,
) -> impl IntoResponse {
    tracing::info!("Generating reminder TwiML");
    let medication_name = params.med.unwrap_or_else(|| "tu medicamento".to_string());
    let dosage = params.dosage.unwrap_or_else(|| "según lo recetado".to_string());
    let reminder_id = params.reminder_id.unwrap_or_default();
    
    let twiml = state.twilio.generate_reminder_twiml(&medication_name, &dosage, &reminder_id);
    
    (
        StatusCode::OK,
        [("Content-Type", "application/xml")],
        twiml,
    )
}

#[derive(Debug, Deserialize)]
pub struct ReminderTwimlParams {
    pub reminder_id: Option<String>,
    pub med: Option<String>,
    pub dosage: Option<String>,
}

/// Handle reminder confirmation callback
#[tracing::instrument(skip(state), fields(reminder_id = ?params.reminder_id, digits = ?form.digits))]
pub async fn reminder_confirm(
    State(state): State<AppState>,
    Query(params): Query<ReminderConfirmParams>,
    Form(form): Form<ReminderConfirmForm>,
) -> impl IntoResponse {
    tracing::info!("Processing reminder confirmation");
    
    let confirmed = form.digits.as_deref() == Some("1");
    
    if let Some(reminder_id_str) = &params.reminder_id {
        if let Ok(reminder_id) = uuid::Uuid::parse_str(reminder_id_str) {
            if let Err(e) = crate::services::update_reminder_status(
                &state.db,
                reminder_id,
                confirmed,
            ).await {
                tracing::error!("Failed to update reminder status: {}", e);
            }
        }
    }
    
    let message = if confirmed {
        "Gracias por confirmar. ¡Que tengas un excelente día!"
    } else {
        "Por favor recuerda tomar tu medicamento. Hasta luego."
    };
    
    let twiml = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<Response>
    <Say voice="Polly.Mia" language="es-MX">{}</Say>
    <Hangup/>
</Response>"#,
        message
    );
    
    (
        StatusCode::OK,
        [("Content-Type", "application/xml")],
        twiml,
    )
}

#[derive(Debug, Deserialize)]
pub struct ReminderConfirmParams {
    pub reminder_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ReminderConfirmForm {
    #[serde(rename = "Digits")]
    pub digits: Option<String>,
}

