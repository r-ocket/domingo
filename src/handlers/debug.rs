//! Debug handlers for development/testing
//!
//! These routes are NOT auth-protected and should only be enabled in development.

use axum::{
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::domain::{CreateCaregiverRequest, CreateElderRequest, UserRole};
use crate::repositories::postgres::{CaregiverRepository, ElderRepository};
use crate::services::AuthService;
use crate::AppState;
use crate::clients::TwilioError;

/// Request to create a debug caregiver
#[derive(Debug, Deserialize)]
pub struct CreateDebugCaregiverRequest {
    pub name: String,
    pub email: String,
    pub password: String,
    #[serde(default = "default_role")]
    pub role: String,
}

fn default_role() -> String {
    "caregiver".to_string()
}

/// Request to create a debug elder
#[derive(Debug, Deserialize)]
pub struct CreateDebugElderRequest {
    pub caregiver_id: Uuid,
    pub name: String,
    #[serde(default = "default_relationship")]
    pub relationship: String,
    pub phone_number: String,
    #[serde(default = "default_timezone")]
    pub timezone: String,
    #[serde(default = "default_language")]
    pub language: String,
}

fn default_timezone() -> String {
    "America/Mexico_City".to_string()
}

fn default_language() -> String {
    "es".to_string()
}

fn default_relationship() -> String {
    "familiar".to_string()
}

/// Response for created caregiver
#[derive(Debug, Serialize)]
pub struct DebugCaregiverResponse {
    pub id: Uuid,
    pub name: String,
    pub email: String,
    pub role: String,
    pub created_at: String,
}

/// Response for created elder
#[derive(Debug, Serialize)]
pub struct DebugElderResponse {
    pub id: Uuid,
    pub caregiver_id: Uuid,
    pub name: String,
    pub relationship: String,
    pub phone_number: String,
    pub timezone: String,
    pub status: String,
    pub created_at: String,
}

/// Health check response
#[derive(Debug, Serialize)]
pub struct HealthResponse {
    pub status: String,
    pub database: String,
    pub services: ServicesStatus,
}

#[derive(Debug, Serialize)]
pub struct ServicesStatus {
    pub twilio: String,
    pub openai: String,
    pub uber: String,
    pub stripe: String,
}

/// Create a debug caregiver (no auth required)
#[tracing::instrument(skip(state), fields(email = %req.email, role = %req.role))]
pub async fn create_caregiver(
    State(state): State<AppState>,
    Json(req): Json<CreateDebugCaregiverRequest>,
) -> Result<impl IntoResponse, DebugError> {
    tracing::info!("Creating debug caregiver");
    
    // Parse and validate role
    let role: UserRole = req.role.parse()
        .map_err(|_| DebugError::Validation(format!("Invalid role: {}. Must be 'admin' or 'caregiver'", req.role)))?;
    
    // Create caregiver using auth service (handles password hashing)
    let create_req = CreateCaregiverRequest {
        name: req.name.clone(),
        email: req.email.clone(),
        password: req.password.clone(),
    };
    
    let caregiver = AuthService::register(&state.db, &create_req).await
        .map_err(|e| DebugError::Domain(e.to_string()))?;
    
    // Update role if not default caregiver
    if role == UserRole::Admin {
        CaregiverRepository::update_role(&state.db, caregiver.id, role).await
            .map_err(|e| DebugError::Domain(e.to_string()))?;
    }
    
    tracing::info!(caregiver_id = %caregiver.id, "Debug caregiver created");
    
    Ok((StatusCode::CREATED, Json(DebugCaregiverResponse {
        id: caregiver.id,
        name: caregiver.name,
        email: caregiver.email,
        role: role.to_string(),
        created_at: caregiver.created_at.to_rfc3339(),
    })))
}

/// Create a debug elder (no auth required)
#[tracing::instrument(skip(state), fields(caregiver_id = %req.caregiver_id, phone = %req.phone_number))]
pub async fn create_elder(
    State(state): State<AppState>,
    Json(req): Json<CreateDebugElderRequest>,
) -> Result<impl IntoResponse, DebugError> {
    tracing::info!("Creating debug elder");
    
    // Verify caregiver exists
    CaregiverRepository::find_by_id(&state.db, req.caregiver_id).await
        .map_err(|e| DebugError::Domain(format!("Caregiver not found: {}", e)))?;
    
    // Create elder
    let create_req = CreateElderRequest {
        name: req.name.clone(),
        relationship: req.relationship.clone(),
        phone_number: req.phone_number.clone(),
        timezone: req.timezone.clone(),
        language: req.language.clone(),
    };
    
    let elder = ElderRepository::create(&state.db, req.caregiver_id, &create_req).await
        .map_err(|e| DebugError::Domain(e.to_string()))?;
    
    tracing::info!(elder_id = %elder.id, "Debug elder created");
    
    Ok((StatusCode::CREATED, Json(DebugElderResponse {
        id: elder.id,
        caregiver_id: elder.caregiver_id,
        name: elder.name,
        relationship: elder.relationship,
        phone_number: elder.phone_number,
        timezone: elder.timezone,
        status: elder.status.to_string(),
        created_at: elder.created_at.to_rfc3339(),
    })))
}

/// Request to initiate a call to an elder
#[derive(Debug, Deserialize)]
pub struct InitiateCallRequest {
    pub elder_id: Uuid,
    /// Voice provider to use (default: xai_grok)
    #[serde(default)]
    pub voice_provider: Option<String>,
    /// Optional per-call prompt override (appended after the standard system prompt + dynamic elder context)
    #[serde(default)]
    pub prompt_override: Option<String>,
    /// Optional Gemini Live setup overrides (generationConfig / VAD / transcription knobs)
    #[serde(default)]
    pub gemini: Option<GeminiLiveDebugConfig>,
    /// Optional xAI Grok per-call config (voice selection, etc.)
    #[serde(default)]
    pub xai: Option<XaiDebugConfig>,
}

#[derive(Debug, Deserialize, Default)]
pub struct XaiDebugConfig {
    /// Voice selection: Ara, Rex, Sal, Eve, Leo
    #[serde(default)]
    pub voice: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct GeminiLiveDebugConfig {
    /// Merged into default `generationConfig`
    #[serde(default)]
    pub generation_config: Option<Value>,
    /// Merged into default `realtimeInputConfig`
    #[serde(default)]
    pub realtime_input_config: Option<Value>,
    /// Used as both `inputAudioTranscription` and `outputAudioTranscription` if provided
    #[serde(default)]
    pub transcription_config: Option<Value>,
}

/// Response for initiated call
#[derive(Debug, Serialize)]
pub struct InitiateCallResponse {
    pub call_sid: String,
    pub status: String,
    pub elder_name: String,
    pub phone_number: String,
    pub voice_provider: String,
}

/// Initiate an outbound call to an elder (no auth required)
#[tracing::instrument(skip(state), fields(elder_id = %req.elder_id, voice_provider = ?req.voice_provider))]
pub async fn initiate_call(
    State(state): State<AppState>,
    Json(req): Json<InitiateCallRequest>,
) -> Result<impl IntoResponse, DebugError> {
    tracing::info!("Initiating outbound call to elder");
    
    // Parse voice provider (default: xai_grok)
    let voice_provider = req
        .voice_provider
        .as_deref()
        .unwrap_or("xai_grok")
        .to_string();
    
    // Look up elder
    let elder = ElderRepository::find_by_id(&state.db, req.elder_id).await
        .map_err(|e| DebugError::Domain(format!("Elder not found: {}", e)))?;
    
    // Build the TwiML URL for the outbound call (include voice_provider)
    let twiml_url = format!("{}/api/twilio/outbound-voice?elder_id={}&voice_provider={}", 
        state.config.base_url, req.elder_id, voice_provider);
    
    // Initiate the call via Twilio
    let call_response = state.twilio.make_call(&elder.phone_number, &twiml_url).await
        .map_err(|e| DebugError::Twilio(e))?;
    
    // Stash per-call overrides in memory; we'll persist to call_sessions.metadata once Twilio hits our webhook.
    // This keeps the "command center" controls tied to the call.
    let per_call_config = serde_json::json!({
        "prompt_override": req.prompt_override.unwrap_or_default(),
        "gemini": {
            "generationConfig": req.gemini.as_ref().and_then(|g| g.generation_config.clone()),
            "realtimeInputConfig": req.gemini.as_ref().and_then(|g| g.realtime_input_config.clone()),
            "transcriptionConfig": req.gemini.as_ref().and_then(|g| g.transcription_config.clone()),
        },
        // xai per-call overrides (expanded later by ui)
        "xai": {
            "voice": req.xai.as_ref().and_then(|x| x.voice.clone())
        }
    });
    state.call_state.set_pending_call_config(call_response.sid.clone(), per_call_config);

    tracing::info!(call_sid = %call_response.sid, voice_provider = %voice_provider, "Outbound call initiated");
    
    Ok((StatusCode::OK, Json(InitiateCallResponse {
        call_sid: call_response.sid,
        status: call_response.status,
        elder_name: elder.name,
        phone_number: elder.phone_number,
        voice_provider: voice_provider.to_string(),
    })))
}

/// Health check endpoint
#[tracing::instrument(skip(state))]
pub async fn health(
    State(state): State<AppState>,
) -> impl IntoResponse {
    tracing::debug!("Health check requested");
    
    // Check database connectivity
    let db_status = match state.db.get().await {
        Ok(client) => {
            match client.query_one("SELECT 1", &[]).await {
                Ok(_) => "connected".to_string(),
                Err(e) => format!("error: {}", e),
            }
        }
        Err(e) => format!("pool error: {}", e),
    };
    
    let status = if db_status == "connected" { "healthy" } else { "degraded" };
    
    Json(HealthResponse {
        status: status.to_string(),
        database: db_status,
        services: ServicesStatus {
            twilio: "configured".to_string(),
            openai: "configured".to_string(),
            uber: "configured".to_string(),
            stripe: "configured".to_string(),
        },
    })
}

// Error handling

#[derive(Debug)]
pub enum DebugError {
    Validation(String),
    Domain(String),
    Twilio(TwilioError),
}

impl IntoResponse for DebugError {
    fn into_response(self) -> axum::response::Response {
        let (status, message) = match self {
            DebugError::Validation(msg) => (StatusCode::BAD_REQUEST, msg),
            DebugError::Domain(msg) => (StatusCode::INTERNAL_SERVER_ERROR, msg),
            DebugError::Twilio(e) => (StatusCode::BAD_GATEWAY, format!("Twilio error: {}", e)),
        };
        
        (status, Json(serde_json::json!({ "error": message }))).into_response()
    }
}

