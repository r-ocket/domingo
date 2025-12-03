//! Medication management handlers

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use serde::Serialize;
use uuid::Uuid;

use crate::domain::{CreateMedicationRequest, Pagination, UpdateMedicationRequest};
use crate::handlers::auth::ApiError;
use crate::handlers::contacts::PaginationQuery;
use crate::middleware::AuthUser;
use crate::services::MedicationService;
use crate::AppState;

/// List medications for an elder
pub async fn list_medications(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(elder_id): Path<Uuid>,
    Query(pagination): Query<PaginationQuery>,
) -> Result<impl IntoResponse, ApiError> {
    let pagination = Pagination {
        page: pagination.page.unwrap_or(1),
        per_page: pagination.per_page.unwrap_or(20),
    };
    
    let medications = MedicationService::list_medications(
        &state.db,
        elder_id,
        auth.session.caregiver_id,
        auth.is_admin(),
        &pagination,
    ).await?;
    
    Ok(Json(crate::handlers::contacts::PaginatedResponse {
        items: medications.items.into_iter().map(MedicationResponse::from).collect(),
        total: medications.total,
        page: medications.page,
        per_page: medications.per_page,
        total_pages: medications.total_pages,
    }))
}

/// Create a new medication
pub async fn create_medication(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(elder_id): Path<Uuid>,
    Json(req): Json<CreateMedicationRequest>,
) -> Result<impl IntoResponse, ApiError> {
    let medication = MedicationService::create_medication(
        &state.db,
        elder_id,
        auth.session.caregiver_id,
        auth.is_admin(),
        &req,
    ).await?;
    
    Ok((StatusCode::CREATED, Json(MedicationResponse::from(medication))))
}

/// Get a medication by ID
pub async fn get_medication(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((_elder_id, medication_id)): Path<(Uuid, Uuid)>,
) -> Result<impl IntoResponse, ApiError> {
    let medication = MedicationService::get_medication(
        &state.db,
        medication_id,
        auth.session.caregiver_id,
        auth.is_admin(),
    ).await?;
    
    Ok(Json(MedicationResponse::from(medication)))
}

/// Update a medication
pub async fn update_medication(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((_elder_id, medication_id)): Path<(Uuid, Uuid)>,
    Json(req): Json<UpdateMedicationRequest>,
) -> Result<impl IntoResponse, ApiError> {
    let medication = MedicationService::update_medication(
        &state.db,
        medication_id,
        auth.session.caregiver_id,
        auth.is_admin(),
        &req,
    ).await?;
    
    Ok(Json(MedicationResponse::from(medication)))
}

/// Delete a medication
pub async fn delete_medication(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((_elder_id, medication_id)): Path<(Uuid, Uuid)>,
) -> Result<impl IntoResponse, ApiError> {
    MedicationService::delete_medication(
        &state.db,
        medication_id,
        auth.session.caregiver_id,
        auth.is_admin(),
    ).await?;
    
    Ok(StatusCode::NO_CONTENT)
}

// Response types

#[derive(Serialize)]
pub struct MedicationResponse {
    pub id: String,
    pub elder_id: String,
    pub name: String,
    pub dosage: String,
    pub instructions: Option<String>,
    pub schedules: Vec<ScheduleResponse>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Serialize)]
pub struct ScheduleResponse {
    pub id: String,
    pub time_of_day: String,
    pub days_of_week: i32,
    pub days_description: String,
}

impl From<crate::domain::MedicationWithSchedule> for MedicationResponse {
    fn from(m: crate::domain::MedicationWithSchedule) -> Self {
        Self {
            id: m.medication.id.to_string(),
            elder_id: m.medication.elder_id.to_string(),
            name: m.medication.name,
            dosage: m.medication.dosage,
            instructions: m.medication.instructions,
            schedules: m.schedules.into_iter().map(|s| {
                let days_description = s.days_description();
                ScheduleResponse {
                    id: s.id.to_string(),
                    time_of_day: s.time_of_day.format("%H:%M").to_string(),
                    days_of_week: s.days_of_week,
                    days_description,
                }
            }).collect(),
            created_at: m.medication.created_at.to_rfc3339(),
            updated_at: m.medication.updated_at.to_rfc3339(),
        }
    }
}

