//! Location management handlers

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use serde::Serialize;
use uuid::Uuid;

use crate::domain::{CreateLocationRequest, Pagination, UpdateLocationRequest};
use crate::handlers::auth::ApiError;
use crate::handlers::contacts::PaginationQuery;
use crate::middleware::AuthUser;
use crate::services::LocationService;
use crate::AppState;

/// List locations for an elder
pub async fn list_locations(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(elder_id): Path<Uuid>,
    Query(pagination): Query<PaginationQuery>,
) -> Result<impl IntoResponse, ApiError> {
    let pagination = Pagination {
        page: pagination.page.unwrap_or(1),
        per_page: pagination.per_page.unwrap_or(20),
    };
    
    let locations = LocationService::list_locations(
        &state.db,
        elder_id,
        auth.session.caregiver_id,
        auth.is_admin(),
        &pagination,
    ).await?;
    
    Ok(Json(crate::handlers::contacts::PaginatedResponse {
        items: locations.items.into_iter().map(LocationResponse::from).collect(),
        total: locations.total,
        page: locations.page,
        per_page: locations.per_page,
        total_pages: locations.total_pages,
    }))
}

/// Create a new location
pub async fn create_location(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(elder_id): Path<Uuid>,
    Json(req): Json<CreateLocationRequest>,
) -> Result<impl IntoResponse, ApiError> {
    let location = LocationService::create_location(
        &state.db,
        elder_id,
        auth.session.caregiver_id,
        auth.is_admin(),
        &req,
    ).await?;
    
    Ok((StatusCode::CREATED, Json(LocationResponse::from(location))))
}

/// Get a location by ID
pub async fn get_location(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((_elder_id, location_id)): Path<(Uuid, Uuid)>,
) -> Result<impl IntoResponse, ApiError> {
    let location = LocationService::get_location(
        &state.db,
        location_id,
        auth.session.caregiver_id,
        auth.is_admin(),
    ).await?;
    
    Ok(Json(LocationResponse::from(location)))
}

/// Update a location
pub async fn update_location(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((_elder_id, location_id)): Path<(Uuid, Uuid)>,
    Json(req): Json<UpdateLocationRequest>,
) -> Result<impl IntoResponse, ApiError> {
    let location = LocationService::update_location(
        &state.db,
        location_id,
        auth.session.caregiver_id,
        auth.is_admin(),
        &req,
    ).await?;
    
    Ok(Json(LocationResponse::from(location)))
}

/// Delete a location
pub async fn delete_location(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((_elder_id, location_id)): Path<(Uuid, Uuid)>,
) -> Result<impl IntoResponse, ApiError> {
    LocationService::delete_location(
        &state.db,
        location_id,
        auth.session.caregiver_id,
        auth.is_admin(),
    ).await?;
    
    Ok(StatusCode::NO_CONTENT)
}

/// Get home location for an elder
pub async fn get_home_location(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(elder_id): Path<Uuid>,
) -> Result<impl IntoResponse, ApiError> {
    // Verify access - get elder to check ownership
    crate::services::ElderService::get_elder(
        &state.db,
        elder_id,
        auth.session.caregiver_id,
        auth.is_admin(),
    ).await?;
    
    let location = LocationService::get_home_location(&state.db, elder_id).await?;
    
    match location {
        Some(loc) => Ok(Json(Some(LocationResponse::from(loc)))),
        None => Ok(Json(None)),
    }
}

// Response types

#[derive(Serialize)]
pub struct LocationResponse {
    pub id: String,
    pub elder_id: String,
    pub name: String,
    pub address: String,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
    pub extra_instructions: Option<String>,
    pub is_home: bool,
    pub location_type: String,
    pub tags: Vec<String>,
    pub created_at: String,
    pub updated_at: String,
}

impl From<crate::domain::Location> for LocationResponse {
    fn from(l: crate::domain::Location) -> Self {
        Self {
            id: l.id.to_string(),
            elder_id: l.elder_id.to_string(),
            name: l.name,
            address: l.address,
            latitude: l.latitude,
            longitude: l.longitude,
            extra_instructions: l.extra_instructions,
            is_home: l.is_home,
            location_type: l.location_type.to_string(),
            tags: l.tags,
            created_at: l.created_at.to_rfc3339(),
            updated_at: l.updated_at.to_rfc3339(),
        }
    }
}

