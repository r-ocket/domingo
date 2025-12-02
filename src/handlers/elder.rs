//! Elder management handlers

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use serde::Serialize;
use uuid::Uuid;

use crate::domain::{CreateElderRequest, UpdateElderRequest};
use crate::handlers::auth::ApiError;
use crate::middleware::AuthUser;
use crate::services::ElderService;
use crate::AppState;

/// Create a new elder profile
pub async fn create_elder(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(req): Json<CreateElderRequest>,
) -> Result<impl IntoResponse, ApiError> {
    let elder = ElderService::create_elder(
        &state.db,
        auth.session.caregiver_id,
        &req,
    ).await?;
    
    Ok((StatusCode::CREATED, Json(ElderResponse::from(elder))))
}

/// Get elder by ID
pub async fn get_elder(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(elder_id): Path<Uuid>,
) -> Result<impl IntoResponse, ApiError> {
    let elder = ElderService::get_elder(
        &state.db,
        elder_id,
        auth.session.caregiver_id,
        auth.is_admin(),
    ).await?;
    
    Ok(Json(ElderResponse::from(elder)))
}

/// Update elder profile
pub async fn update_elder(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(elder_id): Path<Uuid>,
    Json(req): Json<UpdateElderRequest>,
) -> Result<impl IntoResponse, ApiError> {
    let elder = ElderService::update_elder(
        &state.db,
        elder_id,
        auth.session.caregiver_id,
        auth.is_admin(),
        &req,
    ).await?;
    
    Ok(Json(ElderResponse::from(elder)))
}

// Response types

#[derive(Serialize)]
pub struct ElderResponse {
    pub id: String,
    pub caregiver_id: String,
    pub name: String,
    pub phone_number: String,
    pub timezone: String,
    pub language: String,
    pub status: String,
    pub created_at: String,
    pub updated_at: String,
}

impl From<crate::domain::Elder> for ElderResponse {
    fn from(e: crate::domain::Elder) -> Self {
        Self {
            id: e.id.to_string(),
            caregiver_id: e.caregiver_id.to_string(),
            name: e.name,
            phone_number: e.phone_number,
            timezone: e.timezone,
            language: e.language,
            status: e.status.to_string(),
            created_at: e.created_at.to_rfc3339(),
            updated_at: e.updated_at.to_rfc3339(),
        }
    }
}

