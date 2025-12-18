//! Admin handlers

use axum::{
    body::Body,
    extract::{Path, Query, State},
    http::header,
    response::{IntoResponse, Response},
    Json,
};
use futures_util::StreamExt;
use serde::Serialize;
use tokio_stream::wrappers::BroadcastStream;
use uuid::Uuid;

use crate::domain::Pagination;
use crate::handlers::auth::ApiError;
use crate::handlers::contacts::PaginationQuery;
use crate::middleware::AdminUser;
use crate::repositories::postgres::{
    CaregiverRepository, CallSessionRepository, ElderRepository,
    SubscriptionRepository,
};
use crate::services::{CallService, ElderService, RideService};
use crate::AppState;

/// List all caregivers (admin only)
#[tracing::instrument(skip(state, _admin), fields(page = pagination.page, per_page = pagination.per_page))]
pub async fn list_caregivers(
    State(state): State<AppState>,
    _admin: AdminUser,
    Query(pagination): Query<PaginationQuery>,
) -> Result<impl IntoResponse, ApiError> {
    tracing::debug!("Listing caregivers");
    let pagination = Pagination {
        page: pagination.page.unwrap_or(1),
        per_page: pagination.per_page.unwrap_or(20),
    };
    
    let caregivers = CaregiverRepository::list(&state.db, &pagination).await?;
    tracing::info!(total = caregivers.total, "Retrieved caregivers list");
    
    Ok(Json(crate::handlers::contacts::PaginatedResponse {
        items: caregivers.items.into_iter().map(|c| crate::handlers::auth::CaregiverResponse::from(c)).collect(),
        total: caregivers.total,
        page: caregivers.page,
        per_page: caregivers.per_page,
        total_pages: caregivers.total_pages,
    }))
}

/// Get caregiver details (admin only)
#[tracing::instrument(skip(state, _admin), fields(caregiver_id = %id))]
pub async fn get_caregiver(
    State(state): State<AppState>,
    _admin: AdminUser,
    Path(id): Path<Uuid>,
) -> Result<impl IntoResponse, ApiError> {
    tracing::debug!("Fetching caregiver details");
    let caregiver = CaregiverRepository::find_by_id(&state.db, id).await?;
    let elder = ElderService::get_elder_for_caregiver(&state.db, id).await?;
    let subscription = SubscriptionRepository::find_by_caregiver(&state.db, id).await?;
    
    Ok(Json(CaregiverDetailResponse {
        caregiver: crate::handlers::auth::CaregiverResponse::from(caregiver),
        elder: elder.map(|e| crate::handlers::elder::ElderResponse::from(e)),
        subscription: subscription.map(|s| SubscriptionResponse::from(s)),
    }))
}

/// Update caregiver (admin only)
#[tracing::instrument(skip(state, _admin), fields(caregiver_id = %id, new_role = ?req.role))]
pub async fn update_caregiver(
    State(state): State<AppState>,
    _admin: AdminUser,
    Path(id): Path<Uuid>,
    Json(req): Json<AdminUpdateCaregiverRequest>,
) -> Result<impl IntoResponse, ApiError> {
    tracing::info!("Updating caregiver");
    if let Some(role) = req.role {
        let role = role.parse()
            .map_err(|_| crate::domain::DomainError::Validation("Invalid role".to_string()))?;
        CaregiverRepository::update_role(&state.db, id, role).await?;
        tracing::info!("Caregiver role updated");
    }
    
    let caregiver = CaregiverRepository::find_by_id(&state.db, id).await?;
    Ok(Json(crate::handlers::auth::CaregiverResponse::from(caregiver)))
}

/// List all elders (admin only)
pub async fn list_elders(
    State(state): State<AppState>,
    _admin: AdminUser,
    Query(pagination): Query<PaginationQuery>,
) -> Result<impl IntoResponse, ApiError> {
    let pagination = Pagination {
        page: pagination.page.unwrap_or(1),
        per_page: pagination.per_page.unwrap_or(20),
    };
    
    let elders = ElderService::list_elders(&state.db, &pagination).await?;
    
    Ok(Json(crate::handlers::contacts::PaginatedResponse {
        items: elders.items.into_iter().map(|e| crate::handlers::elder::ElderResponse::from(e)).collect(),
        total: elders.total,
        page: elders.page,
        per_page: elders.per_page,
        total_pages: elders.total_pages,
    }))
}

/// Get elder details (admin only)
pub async fn get_elder(
    State(state): State<AppState>,
    _admin: AdminUser,
    Path(id): Path<Uuid>,
) -> Result<impl IntoResponse, ApiError> {
    let elder = ElderRepository::find_by_id(&state.db, id).await?;
    Ok(Json(crate::handlers::elder::ElderResponse::from(elder)))
}

/// List all call logs (admin only)
pub async fn list_all_call_logs(
    State(state): State<AppState>,
    _admin: AdminUser,
    Query(pagination): Query<PaginationQuery>,
) -> Result<impl IntoResponse, ApiError> {
    let pagination = Pagination {
        page: pagination.page.unwrap_or(1),
        per_page: pagination.per_page.unwrap_or(20),
    };
    
    let logs = CallSessionRepository::list_all(&state.db, &pagination).await?;
    
    Ok(Json(crate::handlers::contacts::PaginatedResponse {
        items: logs.items.into_iter().map(|c| crate::handlers::logs::CallLogResponse::from(c)).collect(),
        total: logs.total,
        page: logs.page,
        per_page: logs.per_page,
        total_pages: logs.total_pages,
    }))
}

/// List all ride logs (admin only)
pub async fn list_all_ride_logs(
    State(state): State<AppState>,
    _admin: AdminUser,
    Query(pagination): Query<PaginationQuery>,
) -> Result<impl IntoResponse, ApiError> {
    let pagination = Pagination {
        page: pagination.page.unwrap_or(1),
        per_page: pagination.per_page.unwrap_or(20),
    };
    
    let logs = RideService::list_all_rides(&state.db, &pagination).await?;
    
    Ok(Json(crate::handlers::contacts::PaginatedResponse {
        items: logs.items.into_iter().map(|r| crate::handlers::logs::RideLogResponse::from(r)).collect(),
        total: logs.total,
        page: logs.page,
        per_page: logs.per_page,
        total_pages: logs.total_pages,
    }))
}

/// Get admin statistics
#[tracing::instrument(skip(state, _admin))]
pub async fn get_stats(
    State(state): State<AppState>,
    _admin: AdminUser,
) -> Result<impl IntoResponse, ApiError> {
    tracing::debug!("Fetching admin statistics");
    let active_elders = ElderService::count_active_elders(&state.db).await?;
    let active_subscriptions = SubscriptionRepository::count_active(&state.db).await?;
    let calls_today = CallService::count_calls_today(&state.db).await?;
    let rides_today = RideService::count_rides_today(&state.db).await?;
    
    tracing::info!(
        active_elders = active_elders,
        active_subscriptions = active_subscriptions,
        calls_today = calls_today,
        rides_today = rides_today,
        "Admin stats retrieved"
    );
    
    Ok(Json(AdminStatsResponse {
        active_elders,
        active_subscriptions,
        calls_today,
        rides_today,
    }))
}

/// List active calls (non-SSE endpoint for initial state)
pub async fn list_active_calls(
    State(state): State<AppState>,
    _admin: AdminUser,
) -> impl IntoResponse {
    let calls = state.call_state.list_all();
    Json(calls)
}

/// Get a specific call's state
pub async fn get_call_state(
    State(state): State<AppState>,
    _admin: AdminUser,
    Path(call_sid): Path<String>,
) -> impl IntoResponse {
    match state.call_state.get_call(&call_sid) {
        Some(call) => Json(serde_json::json!(call)).into_response(),
        None => (axum::http::StatusCode::NOT_FOUND, Json(serde_json::json!({ "error": "Call not found" }))).into_response(),
    }
}

/// SSE stream for all call events (global feed)
pub async fn calls_sse(
    State(state): State<AppState>,
    _admin: AdminUser,
) -> Response {
    tracing::debug!("SSE client connected to global call feed");
    
    let rx = state.call_state.subscribe_global();
    let stream = BroadcastStream::new(rx);
    
    let body = Body::from_stream(stream.filter_map(|result| async move {
        match result {
            Ok(event) => {
                let data = serde_json::to_string(&event).ok()?;
                Some(Ok::<_, std::convert::Infallible>(format!("data: {}\n\n", data)))
            }
            Err(_) => None,
        }
    }));
    
    Response::builder()
        .header(header::CONTENT_TYPE, "text/event-stream")
        .header(header::CACHE_CONTROL, "no-cache")
        .header(header::CONNECTION, "keep-alive")
        .body(body)
        .unwrap()
}

/// SSE stream for a specific call
pub async fn call_stream_sse(
    State(state): State<AppState>,
    _admin: AdminUser,
    Path(call_sid): Path<String>,
) -> Response {
    tracing::info!(call_sid = %call_sid, "SSE client connected to call stream");
    
    match state.call_state.subscribe_call(&call_sid) {
        Some(rx) => {
            let stream = BroadcastStream::new(rx);
            
            let body = Body::from_stream(stream.filter_map(|result| async move {
                match result {
                    Ok(event) => {
                        let data = serde_json::to_string(&event).ok()?;
                        Some(Ok::<_, std::convert::Infallible>(format!("data: {}\n\n", data)))
                    }
                    Err(_) => None,
                }
            }));
            
            Response::builder()
                .header(header::CONTENT_TYPE, "text/event-stream")
                .header(header::CACHE_CONTROL, "no-cache")
                .header(header::CONNECTION, "keep-alive")
                .body(body)
                .unwrap()
        }
        None => {
            Response::builder()
                .status(axum::http::StatusCode::NOT_FOUND)
                .body(Body::from("Call not found"))
                .unwrap()
        }
    }
}

// Request/response types

#[derive(Debug, serde::Deserialize)]
pub struct AdminUpdateCaregiverRequest {
    pub role: Option<String>,
}

#[derive(Serialize)]
pub struct CaregiverDetailResponse {
    pub caregiver: crate::handlers::auth::CaregiverResponse,
    pub elder: Option<crate::handlers::elder::ElderResponse>,
    pub subscription: Option<SubscriptionResponse>,
}

#[derive(Serialize)]
pub struct SubscriptionResponse {
    pub id: String,
    pub status: String,
    pub plan_name: String,
    pub current_period_end: String,
    pub cancel_at_period_end: bool,
}

impl From<crate::domain::Subscription> for SubscriptionResponse {
    fn from(s: crate::domain::Subscription) -> Self {
        Self {
            id: s.id.to_string(),
            status: s.status.to_string(),
            plan_name: s.plan_name,
            current_period_end: s.current_period_end.to_rfc3339(),
            cancel_at_period_end: s.cancel_at_period_end,
        }
    }
}

#[derive(Serialize)]
pub struct AdminStatsResponse {
    pub active_elders: i64,
    pub active_subscriptions: i64,
    pub calls_today: i64,
    pub rides_today: i64,
}

