//! Location domain model

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

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
}

impl Location {
    /// Check if location matches a search query
    #[allow(dead_code)]
    pub fn matches_query(&self, query: &str) -> bool {
        let query_lower = query.to_lowercase();
        self.name.to_lowercase().contains(&query_lower)
    }
}

