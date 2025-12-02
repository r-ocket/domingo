//! Uber webhook handlers

use axum::{
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    Json,
};

use crate::clients::UberWebhookEvent;
use crate::domain::RideStatus;
use crate::services::RideService;
use crate::AppState;

/// Handle Uber webhooks
pub async fn handle_webhook(
    State(state): State<AppState>,
    Json(event): Json<UberWebhookEvent>,
) -> impl IntoResponse {
    tracing::info!("Uber webhook: {} for ride {}", event.event_type, event.meta.resource_id);
    
    let status = match event.event_type.as_str() {
        "requests.status_changed" => {
            match event.meta.status.as_deref() {
                Some("processing") => Some(RideStatus::Requested),
                Some("accepted") => Some(RideStatus::DriverAssigned),
                Some("arriving") => Some(RideStatus::Arriving),
                Some("in_progress") => Some(RideStatus::InProgress),
                Some("completed") => Some(RideStatus::Completed),
                Some("driver_canceled") | Some("rider_canceled") => Some(RideStatus::Canceled),
                _ => None,
            }
        }
        _ => None,
    };
    
    if let Some(status) = status {
        let metadata = serde_json::json!({
            "event_type": event.event_type,
            "event_time": event.event_time,
        });
        
        if let Err(e) = RideService::update_ride_from_webhook(
            &state.db,
            &event.meta.resource_id,
            status,
            Some(metadata),
        ).await {
            tracing::error!("Failed to update ride from webhook: {}", e);
        }
    }
    
    StatusCode::OK
}

