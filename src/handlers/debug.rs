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
use uuid::Uuid;

use crate::domain::{CreateCaregiverRequest, CreateElderRequest, UserRole};
use crate::repositories::postgres::{CaregiverRepository, ElderRepository};
use crate::services::AuthService;
use crate::AppState;

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
    pub phone_number: String,
    #[serde(default = "default_timezone")]
    pub timezone: String,
    #[serde(default = "default_language")]
    pub language: String,
}

fn default_timezone() -> String {
    "America/New_York".to_string()
}

fn default_language() -> String {
    "en".to_string()
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
        phone_number: elder.phone_number,
        timezone: elder.timezone,
        status: elder.status.to_string(),
        created_at: elder.created_at.to_rfc3339(),
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
}

impl IntoResponse for DebugError {
    fn into_response(self) -> axum::response::Response {
        let (status, message) = match self {
            DebugError::Validation(msg) => (StatusCode::BAD_REQUEST, msg),
            DebugError::Domain(msg) => (StatusCode::INTERNAL_SERVER_ERROR, msg),
        };
        
        (status, Json(serde_json::json!({ "error": message }))).into_response()
    }
}

