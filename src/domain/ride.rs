//! Ride request domain model

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Status of a ride request
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RideStatus {
    Requested,
    DriverAssigned,
    Arriving,
    InProgress,
    Completed,
    Canceled,
    Failed,
}

impl std::fmt::Display for RideStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RideStatus::Requested => write!(f, "requested"),
            RideStatus::DriverAssigned => write!(f, "driver_assigned"),
            RideStatus::Arriving => write!(f, "arriving"),
            RideStatus::InProgress => write!(f, "in_progress"),
            RideStatus::Completed => write!(f, "completed"),
            RideStatus::Canceled => write!(f, "canceled"),
            RideStatus::Failed => write!(f, "failed"),
        }
    }
}

impl std::str::FromStr for RideStatus {
    type Err = String;
    
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "requested" => Ok(RideStatus::Requested),
            "driver_assigned" => Ok(RideStatus::DriverAssigned),
            "arriving" => Ok(RideStatus::Arriving),
            "in_progress" => Ok(RideStatus::InProgress),
            "completed" => Ok(RideStatus::Completed),
            "canceled" => Ok(RideStatus::Canceled),
            "failed" => Ok(RideStatus::Failed),
            _ => Err(format!("Invalid ride status: {}", s)),
        }
    }
}

/// Ride request record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RideRequest {
    pub id: Uuid,
    pub elder_id: Uuid,
    pub location_id: Uuid,
    pub pickup_address: String,
    pub dropoff_address: String,
    pub uber_ride_id: Option<String>,
    pub status: RideStatus,
    pub driver_name: Option<String>,
    pub driver_phone: Option<String>,
    pub vehicle_make: Option<String>,
    pub vehicle_model: Option<String>,
    pub vehicle_license: Option<String>,
    pub eta_minutes: Option<i32>,
    pub fare_estimate: Option<String>,
    pub fare_actual: Option<String>,
    pub requested_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
    pub metadata: Option<serde_json::Value>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Request to create a ride request
#[derive(Debug, Clone)]
pub struct CreateRideRequest {
    pub elder_id: Uuid,
    pub location_id: Uuid,
    pub pickup_address: String,
    pub dropoff_address: String,
}

/// Update from Uber webhook
#[derive(Debug, Clone)]
pub struct UpdateRideRequest {
    pub uber_ride_id: Option<String>,
    pub status: Option<RideStatus>,
    pub driver_name: Option<String>,
    pub driver_phone: Option<String>,
    pub vehicle_make: Option<String>,
    pub vehicle_model: Option<String>,
    pub vehicle_license: Option<String>,
    pub eta_minutes: Option<i32>,
    pub fare_estimate: Option<String>,
    pub fare_actual: Option<String>,
    pub completed_at: Option<DateTime<Utc>>,
    pub metadata: Option<serde_json::Value>,
}

/// Ride info to speak to the elder
#[derive(Debug, Clone, Serialize)]
pub struct RideInfo {
    pub status: RideStatus,
    pub pickup_name: String,
    pub destination_name: String,
    pub driver_name: Option<String>,
    pub vehicle_description: Option<String>,
    pub eta_minutes: Option<i32>,
}

