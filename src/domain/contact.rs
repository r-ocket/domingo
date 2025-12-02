//! Contact domain model

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Contact for an elder (family members, doctors, etc.)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Contact {
    pub id: Uuid,
    pub elder_id: Uuid,
    pub name: String,
    pub relationship: String,
    pub phone: String,
    pub notes: Option<String>,
    pub is_emergency: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Request to create a new contact
#[derive(Debug, Clone, Deserialize)]
pub struct CreateContactRequest {
    pub name: String,
    pub relationship: String,
    pub phone: String,
    pub notes: Option<String>,
    #[serde(default)]
    pub is_emergency: bool,
}

/// Request to update contact
#[derive(Debug, Clone, Deserialize)]
pub struct UpdateContactRequest {
    pub name: Option<String>,
    pub relationship: Option<String>,
    pub phone: Option<String>,
    pub notes: Option<String>,
    pub is_emergency: Option<bool>,
}

impl Contact {
    /// Check if contact matches a search query (name or relationship)
    #[allow(dead_code)]
    pub fn matches_query(&self, query: &str) -> bool {
        let query_lower = query.to_lowercase();
        self.name.to_lowercase().contains(&query_lower)
            || self.relationship.to_lowercase().contains(&query_lower)
    }
}

