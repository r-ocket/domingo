//! Location domain model

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Type of location
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LocationType {
    /// Places to GO to (doctor, pharmacy, home)
    Destination,
    /// Places the elder might BE when calling (friend's house, church)
    CommonSpot,
}

impl Default for LocationType {
    fn default() -> Self {
        LocationType::Destination
    }
}

impl std::fmt::Display for LocationType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LocationType::Destination => write!(f, "destination"),
            LocationType::CommonSpot => write!(f, "common_spot"),
        }
    }
}

impl std::str::FromStr for LocationType {
    type Err = String;
    
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "destination" => Ok(LocationType::Destination),
            "common_spot" => Ok(LocationType::CommonSpot),
            _ => Err(format!("Invalid location type: {}", s)),
        }
    }
}

/// Saved location for an elder (home, doctor's office, etc.)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Location {
    pub id: Uuid,
    pub elder_id: Uuid,
    pub name: String,
    pub address: String,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
    pub extra_instructions: Option<String>,
    pub is_home: bool,
    pub location_type: LocationType,
    pub tags: Vec<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Request to create a new location
#[derive(Debug, Clone, Deserialize)]
pub struct CreateLocationRequest {
    pub name: String,
    pub address: String,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
    pub extra_instructions: Option<String>,
    #[serde(default)]
    pub is_home: bool,
    #[serde(default)]
    pub location_type: LocationType,
    #[serde(default)]
    pub tags: Vec<String>,
}

/// Request to update location
#[derive(Debug, Clone, Deserialize)]
pub struct UpdateLocationRequest {
    pub name: Option<String>,
    pub address: Option<String>,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
    pub extra_instructions: Option<String>,
    pub is_home: Option<bool>,
    pub location_type: Option<LocationType>,
    pub tags: Option<Vec<String>>,
}

impl Location {
    /// Check if location matches a search query
    #[allow(dead_code)]
    pub fn matches_query(&self, query: &str) -> bool {
        let query_lower = query.to_lowercase();
        self.name.to_lowercase().contains(&query_lower)
    }
}

