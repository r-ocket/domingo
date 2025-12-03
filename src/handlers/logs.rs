//! Log viewing handlers

use axum::{
    extract::{Path, Query, State},
    response::IntoResponse,
    Json,
};
use serde::Serialize;
use uuid::Uuid;

use crate::domain::Pagination;
use crate::handlers::auth::ApiError;
use crate::handlers::contacts::PaginationQuery;
use crate::middleware::AuthUser;
use crate::repositories::postgres::{CallSessionRepository, ReminderLogRepository, RideRequestRepository};
use crate::services::{ElderService, RideService};
use crate::AppState;

/// List call logs for an elder
pub async fn list_call_logs(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(elder_id): Path<Uuid>,
    Query(pagination): Query<PaginationQuery>,
) -> Result<impl IntoResponse, ApiError> {
    // Verify access
    let _ = ElderService::get_elder(
        &state.db,
        elder_id,
        auth.session.caregiver_id,
        auth.is_admin(),
    ).await?;
    
    let pagination = Pagination {
        page: pagination.page.unwrap_or(1),
        per_page: pagination.per_page.unwrap_or(20),
    };
    
    let logs = CallSessionRepository::list_by_elder(&state.db, elder_id, &pagination).await?;
    
    Ok(Json(crate::handlers::contacts::PaginatedResponse {
        items: logs.items.into_iter().map(CallLogResponse::from).collect(),
        total: logs.total,
        page: logs.page,
        per_page: logs.per_page,
        total_pages: logs.total_pages,
    }))
}

/// Get a specific call log by ID
pub async fn get_call_log(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((elder_id, call_id)): Path<(Uuid, Uuid)>,
) -> Result<impl IntoResponse, ApiError> {
    // Verify access to elder
    let _ = ElderService::get_elder(
        &state.db,
        elder_id,
        auth.session.caregiver_id,
        auth.is_admin(),
    ).await?;
    
    let call = CallSessionRepository::find_by_id(&state.db, call_id).await?;
    
    // Verify the call belongs to this elder
    if call.elder_id != elder_id {
        return Err(crate::domain::DomainError::NotFound("Call not found".to_string()).into());
    }
    
    Ok(Json(CallLogResponse::from(call)))
}

/// List ride logs for an elder
pub async fn list_ride_logs(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(elder_id): Path<Uuid>,
    Query(pagination): Query<PaginationQuery>,
) -> Result<impl IntoResponse, ApiError> {
    // Verify access
    let _ = ElderService::get_elder(
        &state.db,
        elder_id,
        auth.session.caregiver_id,
        auth.is_admin(),
    ).await?;
    
    let pagination = Pagination {
        page: pagination.page.unwrap_or(1),
        per_page: pagination.per_page.unwrap_or(20),
    };
    
    let logs = RideRequestRepository::list_by_elder(&state.db, elder_id, &pagination).await?;
    
    Ok(Json(crate::handlers::contacts::PaginatedResponse {
        items: logs.items.into_iter().map(RideLogResponse::from).collect(),
        total: logs.total,
        page: logs.page,
        per_page: logs.per_page,
        total_pages: logs.total_pages,
    }))
}

/// List reminder logs for an elder
pub async fn list_reminder_logs(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(elder_id): Path<Uuid>,
    Query(pagination): Query<PaginationQuery>,
) -> Result<impl IntoResponse, ApiError> {
    // Verify access
    let _ = ElderService::get_elder(
        &state.db,
        elder_id,
        auth.session.caregiver_id,
        auth.is_admin(),
    ).await?;
    
    let pagination = Pagination {
        page: pagination.page.unwrap_or(1),
        per_page: pagination.per_page.unwrap_or(20),
    };
    
    let logs = ReminderLogRepository::list_by_elder(&state.db, elder_id, &pagination).await?;
    
    Ok(Json(crate::handlers::contacts::PaginatedResponse {
        items: logs.items.into_iter().map(ReminderLogResponse::from).collect(),
        total: logs.total,
        page: logs.page,
        per_page: logs.per_page,
        total_pages: logs.total_pages,
    }))
}

/// Get active ride for an elder
pub async fn get_active_ride(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(elder_id): Path<Uuid>,
) -> Result<impl IntoResponse, ApiError> {
    // Verify access
    let _ = ElderService::get_elder(
        &state.db,
        elder_id,
        auth.session.caregiver_id,
        auth.is_admin(),
    ).await?;
    
    let ride = RideService::get_active_ride(&state.db, elder_id).await?;
    
    Ok(Json(ride.map(RideLogResponse::from)))
}

// Response types

#[derive(Serialize)]
pub struct CallLogResponse {
    pub id: String,
    pub twilio_call_sid: String,
    pub from_number: String,
    pub status: String,
    pub started_at: String,
    pub ended_at: Option<String>,
    pub duration_seconds: Option<i32>,
    pub summary_text: Option<String>,
    pub tools_used: Vec<String>,
    pub transferred_to: Option<String>,
}

impl From<crate::domain::CallSession> for CallLogResponse {
    fn from(c: crate::domain::CallSession) -> Self {
        Self {
            id: c.id.to_string(),
            twilio_call_sid: c.twilio_call_sid,
            from_number: c.from_number,
            status: c.status.to_string(),
            started_at: c.started_at.to_rfc3339(),
            ended_at: c.ended_at.map(|t| t.to_rfc3339()),
            duration_seconds: c.duration_seconds,
            summary_text: c.summary_text,
            tools_used: c.tools_used,
            transferred_to: c.transferred_to,
        }
    }
}

#[derive(Serialize)]
pub struct RideLogResponse {
    pub id: String,
    pub pickup_address: String,
    pub dropoff_address: String,
    pub status: String,
    pub driver_name: Option<String>,
    pub vehicle_description: Option<String>,
    pub eta_minutes: Option<i32>,
    pub fare_actual: Option<String>,
    pub requested_at: String,
    pub completed_at: Option<String>,
}

impl From<crate::domain::RideRequest> for RideLogResponse {
    fn from(r: crate::domain::RideRequest) -> Self {
        let vehicle_description = match (&r.vehicle_make, &r.vehicle_model, &r.vehicle_license) {
            (Some(make), Some(model), Some(license)) => Some(format!("{} {} ({})", make, model, license)),
            (Some(make), Some(model), None) => Some(format!("{} {}", make, model)),
            _ => None,
        };
        
        Self {
            id: r.id.to_string(),
            pickup_address: r.pickup_address,
            dropoff_address: r.dropoff_address,
            status: r.status.to_string(),
            driver_name: r.driver_name,
            vehicle_description,
            eta_minutes: r.eta_minutes,
            fare_actual: r.fare_actual,
            requested_at: r.requested_at.to_rfc3339(),
            completed_at: r.completed_at.map(|t| t.to_rfc3339()),
        }
    }
}

#[derive(Serialize)]
pub struct ReminderLogResponse {
    pub id: String,
    pub medication_name: String,
    pub timestamp: String,
    pub delivery_method: String,
    pub status: String,
}

impl From<crate::domain::ReminderLog> for ReminderLogResponse {
    fn from(r: crate::domain::ReminderLog) -> Self {
        Self {
            id: r.id.to_string(),
            medication_name: r.medication_name,
            timestamp: r.timestamp.to_rfc3339(),
            delivery_method: r.delivery_method.to_string(),
            status: r.status.to_string(),
        }
    }
}

