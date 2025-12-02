//! Medication domain model

use chrono::{DateTime, NaiveTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Medication entry for an elder
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Medication {
    pub id: Uuid,
    pub elder_id: Uuid,
    pub name: String,
    pub dosage: String,
    pub instructions: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Request to create a new medication
#[derive(Debug, Clone, Deserialize)]
pub struct CreateMedicationRequest {
    pub name: String,
    pub dosage: String,
    pub instructions: Option<String>,
    /// Times of day for this medication (e.g., ["08:00", "20:00"])
    pub schedule_times: Vec<String>,
}

/// Request to update medication
#[derive(Debug, Clone, Deserialize)]
pub struct UpdateMedicationRequest {
    pub name: Option<String>,
    pub dosage: Option<String>,
    pub instructions: Option<String>,
    pub schedule_times: Option<Vec<String>>,
}

/// Medication schedule entry - when to take a medication
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MedicationSchedule {
    pub id: Uuid,
    pub medication_id: Uuid,
    pub time_of_day: NaiveTime,
    pub days_pattern: String, // "daily", "weekdays", "weekends", or cron-like pattern
    pub created_at: DateTime<Utc>,
}

/// Request to create a medication schedule
#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
pub struct CreateScheduleRequest {
    pub time_of_day: String, // "HH:MM" format
    #[serde(default = "default_days_pattern")]
    pub days_pattern: String,
}

fn default_days_pattern() -> String {
    "daily".to_string()
}

/// Medication with its schedules
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MedicationWithSchedule {
    pub medication: Medication,
    pub schedules: Vec<MedicationSchedule>,
}

/// Upcoming medication reminder info
#[derive(Debug, Clone, Serialize)]
pub struct UpcomingMedication {
    pub medication_id: Uuid,
    pub medication_name: String,
    pub dosage: String,
    pub instructions: Option<String>,
    pub scheduled_time: NaiveTime,
    pub next_due: DateTime<Utc>,
}

