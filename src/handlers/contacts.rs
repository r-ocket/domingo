//! Contact management handlers

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::domain::{CreateContactRequest, Pagination, UpdateContactRequest};
use crate::handlers::auth::ApiError;
use crate::middleware::AuthUser;
use crate::services::ContactService;
use crate::AppState;

/// List contacts for an elder
pub async fn list_contacts(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(elder_id): Path<Uuid>,
    Query(pagination): Query<PaginationQuery>,
) -> Result<impl IntoResponse, ApiError> {
    let pagination = Pagination {
        page: pagination.page.unwrap_or(1),
        per_page: pagination.per_page.unwrap_or(20),
    };
    
    let contacts = ContactService::list_contacts(
        &state.db,
        elder_id,
        auth.session.caregiver_id,
        auth.is_admin(),
        &pagination,
    ).await?;
    
    Ok(Json(PaginatedResponse {
        items: contacts.items.into_iter().map(ContactResponse::from).collect(),
        total: contacts.total,
        page: contacts.page,
        per_page: contacts.per_page,
        total_pages: contacts.total_pages,
    }))
}

/// Create a new contact
pub async fn create_contact(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(elder_id): Path<Uuid>,
    Json(req): Json<CreateContactRequest>,
) -> Result<impl IntoResponse, ApiError> {
    let contact = ContactService::create_contact(
        &state.db,
        elder_id,
        auth.session.caregiver_id,
        auth.is_admin(),
        &req,
    ).await?;
    
    Ok((StatusCode::CREATED, Json(ContactResponse::from(contact))))
}

/// Get a contact by ID
pub async fn get_contact(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((_elder_id, contact_id)): Path<(Uuid, Uuid)>,
) -> Result<impl IntoResponse, ApiError> {
    let contact = ContactService::get_contact(
        &state.db,
        contact_id,
        auth.session.caregiver_id,
        auth.is_admin(),
    ).await?;
    
    Ok(Json(ContactResponse::from(contact)))
}

/// Update a contact
pub async fn update_contact(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((_elder_id, contact_id)): Path<(Uuid, Uuid)>,
    Json(req): Json<UpdateContactRequest>,
) -> Result<impl IntoResponse, ApiError> {
    let contact = ContactService::update_contact(
        &state.db,
        contact_id,
        auth.session.caregiver_id,
        auth.is_admin(),
        &req,
    ).await?;
    
    Ok(Json(ContactResponse::from(contact)))
}

/// Delete a contact
pub async fn delete_contact(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((_elder_id, contact_id)): Path<(Uuid, Uuid)>,
) -> Result<impl IntoResponse, ApiError> {
    ContactService::delete_contact(
        &state.db,
        contact_id,
        auth.session.caregiver_id,
        auth.is_admin(),
    ).await?;
    
    Ok(StatusCode::NO_CONTENT)
}

/// Get emergency contacts for an elder
pub async fn get_emergency_contacts(
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
    
    let contacts = ContactService::get_emergency_contacts(&state.db, elder_id).await?;
    
    Ok(Json(contacts.into_iter().map(ContactResponse::from).collect::<Vec<_>>()))
}

// Query and response types

#[derive(Deserialize)]
pub struct PaginationQuery {
    pub page: Option<u32>,
    pub per_page: Option<u32>,
}

#[derive(Serialize)]
pub struct PaginatedResponse<T> {
    pub items: Vec<T>,
    pub total: i64,
    pub page: u32,
    pub per_page: u32,
    pub total_pages: u32,
}

#[derive(Serialize)]
pub struct ContactResponse {
    pub id: String,
    pub elder_id: String,
    pub name: String,
    pub relationship: String,
    pub phone: String,
    pub notes: Option<String>,
    pub is_emergency: bool,
    pub created_at: String,
    pub updated_at: String,
}

impl From<crate::domain::Contact> for ContactResponse {
    fn from(c: crate::domain::Contact) -> Self {
        Self {
            id: c.id.to_string(),
            elder_id: c.elder_id.to_string(),
            name: c.name,
            relationship: c.relationship,
            phone: c.phone,
            notes: c.notes,
            is_emergency: c.is_emergency,
            created_at: c.created_at.to_rfc3339(),
            updated_at: c.updated_at.to_rfc3339(),
        }
    }
}

