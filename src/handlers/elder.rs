//! Elder management handlers

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use axum_extra::extract::{cookie::Cookie, CookieJar};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::domain::{CreateElderRequest, DomainError, UpdateElderRequest};
use crate::handlers::auth::ApiError;
use crate::middleware::AuthUser;
use crate::repositories::postgres::ElderRepository;
use crate::services::ElderService;
use crate::AppState;

/// Cookie name for selected elder
pub const SELECTED_ELDER_COOKIE: &str = "selected_elder_id";

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

/// List all elders for the current caregiver
pub async fn list_caregiver_elders(
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<impl IntoResponse, ApiError> {
    let elders = ElderRepository::list_by_caregiver(&state.db, auth.session.caregiver_id).await?;
    
    let response: Vec<ElderResponse> = elders.into_iter().map(ElderResponse::from).collect();
    Ok(Json(response))
}

/// Select an elder (sets cookie)
#[derive(Debug, Deserialize)]
pub struct SelectElderRequest {
    pub elder_id: Uuid,
}

pub async fn select_elder(
    State(state): State<AppState>,
    auth: AuthUser,
    jar: CookieJar,
    Json(req): Json<SelectElderRequest>,
) -> Result<impl IntoResponse, ApiError> {
    // Verify the elder belongs to this caregiver
    let elders = ElderRepository::list_by_caregiver(&state.db, auth.session.caregiver_id).await?;
    
    let elder = elders.iter().find(|e| e.id == req.elder_id)
        .ok_or_else(|| DomainError::NotFound("Adulto mayor no encontrado".to_string()))?;
    
    // Set cookie
    let cookie = Cookie::build((SELECTED_ELDER_COOKIE, elder.id.to_string()))
        .path("/")
        .http_only(true)
        .build();
    
    let updated_jar = jar.add(cookie);
    
    Ok((updated_jar, Json(ElderResponse::from(elder.clone()))))
}

// Response types

#[derive(Serialize)]
pub struct ElderResponse {
    pub id: String,
    pub caregiver_id: String,
    pub name: String,
    pub relationship: String,
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
            relationship: e.relationship,
            phone_number: e.phone_number,
            timezone: e.timezone,
            language: e.language,
            status: e.status.to_string(),
            created_at: e.created_at.to_rfc3339(),
            updated_at: e.updated_at.to_rfc3339(),
        }
    }
}

