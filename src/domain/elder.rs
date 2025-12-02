//! Elder domain model

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Status of an elder's account
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ElderStatus {
    Active,
    Inactive,
}

impl std::fmt::Display for ElderStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ElderStatus::Active => write!(f, "active"),
            ElderStatus::Inactive => write!(f, "inactive"),
        }
    }
}

impl std::str::FromStr for ElderStatus {
    type Err = String;
    
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "active" => Ok(ElderStatus::Active),
            "inactive" => Ok(ElderStatus::Inactive),
            _ => Err(format!("Invalid status: {}", s)),
        }
    }
}

/// Elder profile - the elderly person who uses the voice assistant
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Elder {
    pub id: Uuid,
    pub caregiver_id: Uuid,
    pub name: String,
    pub phone_number: String,
    pub timezone: String,
    pub language: String,
    pub status: ElderStatus,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Request to create a new elder profile
#[derive(Debug, Clone, Deserialize)]
pub struct CreateElderRequest {
    pub name: String,
    pub phone_number: String,
    #[serde(default = "default_timezone")]
    pub timezone: String,
    #[serde(default = "default_language")]
    pub language: String,
}

fn default_timezone() -> String {
    "America/New_York".to_string()
}

fn default_language() -> String {
    "en".to_string()
}

/// Request to update elder profile
#[derive(Debug, Clone, Deserialize)]
pub struct UpdateElderRequest {
    pub name: Option<String>,
    pub phone_number: Option<String>,
    pub timezone: Option<String>,
    pub language: Option<String>,
    pub status: Option<ElderStatus>,
}

/// Elder summary for dashboard display
#[derive(Debug, Clone, Serialize)]
pub struct ElderSummary {
    pub elder: Elder,
    pub contacts_count: i64,
    pub locations_count: i64,
    pub medications_count: i64,
    pub last_call_at: Option<DateTime<Utc>>,
    pub next_reminder_at: Option<DateTime<Utc>>,
}

