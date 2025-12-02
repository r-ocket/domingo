//! Reminder domain model

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// How a reminder was delivered
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DeliveryMethod {
    Call,
    Sms,
}

impl std::fmt::Display for DeliveryMethod {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DeliveryMethod::Call => write!(f, "call"),
            DeliveryMethod::Sms => write!(f, "sms"),
        }
    }
}

impl std::str::FromStr for DeliveryMethod {
    type Err = String;
    
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "call" => Ok(DeliveryMethod::Call),
            "sms" => Ok(DeliveryMethod::Sms),
            _ => Err(format!("Invalid delivery method: {}", s)),
        }
    }
}

/// Status of a reminder delivery
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReminderStatus {
    Pending,
    Delivered,
    Answered,
    Confirmed,
    NoAnswer,
    Failed,
}

impl std::fmt::Display for ReminderStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ReminderStatus::Pending => write!(f, "pending"),
            ReminderStatus::Delivered => write!(f, "delivered"),
            ReminderStatus::Answered => write!(f, "answered"),
            ReminderStatus::Confirmed => write!(f, "confirmed"),
            ReminderStatus::NoAnswer => write!(f, "no_answer"),
            ReminderStatus::Failed => write!(f, "failed"),
        }
    }
}

impl std::str::FromStr for ReminderStatus {
    type Err = String;
    
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "pending" => Ok(ReminderStatus::Pending),
            "delivered" => Ok(ReminderStatus::Delivered),
            "answered" => Ok(ReminderStatus::Answered),
            "confirmed" => Ok(ReminderStatus::Confirmed),
            "no_answer" => Ok(ReminderStatus::NoAnswer),
            "failed" => Ok(ReminderStatus::Failed),
            _ => Err(format!("Invalid reminder status: {}", s)),
        }
    }
}

/// Log entry for a medication reminder
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReminderLog {
    pub id: Uuid,
    pub schedule_id: Uuid,
    pub elder_id: Uuid,
    pub medication_name: String,
    pub timestamp: DateTime<Utc>,
    pub delivery_method: DeliveryMethod,
    pub status: ReminderStatus,
    pub twilio_call_sid: Option<String>,
    pub metadata: Option<serde_json::Value>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Request to create a reminder log entry
#[derive(Debug, Clone)]
pub struct CreateReminderLogRequest {
    pub schedule_id: Uuid,
    pub elder_id: Uuid,
    pub medication_name: String,
    pub delivery_method: DeliveryMethod,
}

/// Request to update reminder log status
#[derive(Debug, Clone)]
pub struct UpdateReminderLogRequest {
    pub status: ReminderStatus,
    pub twilio_call_sid: Option<String>,
    pub metadata: Option<serde_json::Value>,
}

