//! Caregiver domain model

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// User role in the system
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum UserRole {
    Caregiver,
    Admin,
}

impl std::fmt::Display for UserRole {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            UserRole::Caregiver => write!(f, "caregiver"),
            UserRole::Admin => write!(f, "admin"),
        }
    }
}

impl std::str::FromStr for UserRole {
    type Err = String;
    
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "caregiver" => Ok(UserRole::Caregiver),
            "admin" => Ok(UserRole::Admin),
            _ => Err(format!("Invalid role: {}", s)),
        }
    }
}

/// Caregiver account - the paying user who sets up the service for an elder
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Caregiver {
    pub id: Uuid,
    pub name: String,
    pub email: String,
    #[serde(skip_serializing)]
    pub password_hash: String,
    pub role: UserRole,
    pub phone: Option<String>,
    pub notes: Option<String>,
    pub stripe_customer_id: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Request to create a new caregiver account
#[derive(Debug, Clone, Deserialize)]
pub struct CreateCaregiverRequest {
    pub name: String,
    pub email: String,
    pub password: String,
}

/// Request to update caregiver profile
#[derive(Debug, Clone, Deserialize)]
pub struct UpdateCaregiverRequest {
    pub name: Option<String>,
    pub email: Option<String>,
    pub phone: Option<String>,
    pub notes: Option<String>,
}

/// Relationship between a caregiver and an elder with context notes
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaregiverElderRelationship {
    pub id: Uuid,
    pub caregiver_id: Uuid,
    pub elder_id: Uuid,
    pub relationship: String,
    pub notes: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Request to create/update caregiver-elder relationship
#[derive(Debug, Clone, Deserialize)]
pub struct UpdateRelationshipRequest {
    pub relationship: Option<String>,
    pub notes: Option<String>,
}

/// Login credentials
#[derive(Debug, Clone, Deserialize)]
pub struct LoginRequest {
    pub email: String,
    pub password: String,
}

/// Session data stored in cookie
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub caregiver_id: Uuid,
    pub role: UserRole,
    pub created_at: DateTime<Utc>,
}

impl Caregiver {
    #[allow(dead_code)]
    pub fn is_admin(&self) -> bool {
        self.role == UserRole::Admin
    }
}

