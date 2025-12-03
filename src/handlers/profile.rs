//! Caregiver profile handlers

use axum::{
    extract::{Path, State},
    response::IntoResponse,
    Json,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::domain::{UpdateCaregiverRequest, UpdateRelationshipRequest};
use crate::handlers::auth::ApiError;
use crate::middleware::AuthUser;
use crate::repositories::postgres::CaregiverRepository;
use crate::services::ElderService;
use crate::AppState;

/// Get current caregiver's profile
pub async fn get_profile(
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<impl IntoResponse, ApiError> {
    let caregiver = CaregiverRepository::find_by_id(&state.db, auth.session.caregiver_id).await?;
    Ok(Json(CaregiverProfileResponse::from(caregiver)))
}

/// Update current caregiver's profile
pub async fn update_profile(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(req): Json<UpdateProfileRequest>,
) -> Result<impl IntoResponse, ApiError> {
    let update_req = UpdateCaregiverRequest {
        name: req.name,
        email: req.email,
        phone: req.phone,
        notes: req.notes,
    };
    
    let caregiver = CaregiverRepository::update(&state.db, auth.session.caregiver_id, &update_req).await?;
    Ok(Json(CaregiverProfileResponse::from(caregiver)))
}

// Request/Response types

#[derive(Debug, Deserialize)]
pub struct UpdateProfileRequest {
    pub name: Option<String>,
    pub email: Option<String>,
    pub phone: Option<String>,
    pub notes: Option<String>,
}

#[derive(Serialize)]
pub struct CaregiverProfileResponse {
    pub id: String,
    pub name: String,
    pub email: String,
    pub phone: Option<String>,
    pub notes: Option<String>,
    pub role: String,
    pub created_at: String,
}

impl From<crate::domain::Caregiver> for CaregiverProfileResponse {
    fn from(c: crate::domain::Caregiver) -> Self {
        Self {
            id: c.id.to_string(),
            name: c.name,
            email: c.email,
            phone: c.phone,
            notes: c.notes,
            role: c.role.to_string(),
            created_at: c.created_at.to_rfc3339(),
        }
    }
}

// === Relationship handlers ===

/// Get relationship with a specific elder
pub async fn get_relationship(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(elder_id): Path<Uuid>,
) -> Result<impl IntoResponse, ApiError> {
    // Verify access to elder
    let _ = ElderService::get_elder(
        &state.db,
        elder_id,
        auth.session.caregiver_id,
        auth.is_admin(),
    ).await?;
    
    let rel = CaregiverRepository::get_relationship(
        &state.db,
        auth.session.caregiver_id,
        elder_id,
    ).await?;
    
    Ok(Json(rel.map(RelationshipResponse::from)))
}

/// Update relationship with an elder
pub async fn update_relationship(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(elder_id): Path<Uuid>,
    Json(req): Json<UpdateRelationshipApiRequest>,
) -> Result<impl IntoResponse, ApiError> {
    // Verify access to elder
    let _ = ElderService::get_elder(
        &state.db,
        elder_id,
        auth.session.caregiver_id,
        auth.is_admin(),
    ).await?;
    
    let update_req = UpdateRelationshipRequest {
        relationship: req.relationship,
        notes: req.notes,
    };
    
    let rel = CaregiverRepository::upsert_relationship(
        &state.db,
        auth.session.caregiver_id,
        elder_id,
        &update_req,
    ).await?;
    
    Ok(Json(RelationshipResponse::from(rel)))
}

/// List all relationships for the current caregiver
pub async fn list_relationships(
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<impl IntoResponse, ApiError> {
    let relationships = CaregiverRepository::list_relationships(
        &state.db,
        auth.session.caregiver_id,
    ).await?;
    
    Ok(Json(relationships.into_iter().map(RelationshipResponse::from).collect::<Vec<_>>()))
}

#[derive(Debug, Deserialize)]
pub struct UpdateRelationshipApiRequest {
    pub relationship: Option<String>,
    pub notes: Option<String>,
}

#[derive(Serialize)]
pub struct RelationshipResponse {
    pub id: String,
    pub elder_id: String,
    pub relationship: String,
    pub notes: Option<String>,
    pub updated_at: String,
}

impl From<crate::domain::CaregiverElderRelationship> for RelationshipResponse {
    fn from(r: crate::domain::CaregiverElderRelationship) -> Self {
        Self {
            id: r.id.to_string(),
            elder_id: r.elder_id.to_string(),
            relationship: r.relationship,
            notes: r.notes,
            updated_at: r.updated_at.to_rfc3339(),
        }
    }
}

