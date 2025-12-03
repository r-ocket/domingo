//! Medication domain model

use chrono::{DateTime, Datelike, NaiveTime, Utc, Weekday};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Day of week bitmask constants
pub const DAY_MONDAY: i32 = 1;
pub const DAY_TUESDAY: i32 = 2;
pub const DAY_WEDNESDAY: i32 = 4;
pub const DAY_THURSDAY: i32 = 8;
pub const DAY_FRIDAY: i32 = 16;
pub const DAY_SATURDAY: i32 = 32;
pub const DAY_SUNDAY: i32 = 64;
pub const DAYS_ALL: i32 = 127;      // All days
pub const DAYS_WEEKDAYS: i32 = 31;  // Mon-Fri
pub const DAYS_WEEKENDS: i32 = 96;  // Sat-Sun

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
    /// Days of week bitmask (default: 127 = all days)
    #[serde(default = "default_days_of_week")]
    pub days_of_week: i32,
}

fn default_days_of_week() -> i32 {
    DAYS_ALL
}

/// Request to update medication
#[derive(Debug, Clone, Deserialize)]
pub struct UpdateMedicationRequest {
    pub name: Option<String>,
    pub dosage: Option<String>,
    pub instructions: Option<String>,
    pub schedule_times: Option<Vec<String>>,
    pub days_of_week: Option<i32>,
}

/// Medication schedule entry - when to take a medication
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MedicationSchedule {
    pub id: Uuid,
    pub medication_id: Uuid,
    pub time_of_day: NaiveTime,
    pub days_of_week: i32,  // Bitmask: Mon=1, Tue=2, Wed=4, Thu=8, Fri=16, Sat=32, Sun=64
    #[serde(skip_serializing_if = "Option::is_none")]
    pub days_pattern: Option<String>, // Legacy field for backwards compatibility
    pub created_at: DateTime<Utc>,
}

impl MedicationSchedule {
    /// Check if this schedule applies to a given weekday
    pub fn applies_to_day(&self, weekday: Weekday) -> bool {
        let day_bit = match weekday {
            Weekday::Mon => DAY_MONDAY,
            Weekday::Tue => DAY_TUESDAY,
            Weekday::Wed => DAY_WEDNESDAY,
            Weekday::Thu => DAY_THURSDAY,
            Weekday::Fri => DAY_FRIDAY,
            Weekday::Sat => DAY_SATURDAY,
            Weekday::Sun => DAY_SUNDAY,
        };
        (self.days_of_week & day_bit) != 0
    }
    
    /// Check if this schedule applies today
    pub fn applies_today(&self) -> bool {
        self.applies_to_day(Utc::now().weekday())
    }
    
    /// Get human-readable days description (Spanish)
    pub fn days_description(&self) -> String {
        match self.days_of_week {
            127 => "Todos los días".to_string(),
            31 => "Lunes a Viernes".to_string(),
            96 => "Fines de semana".to_string(),
            _ => {
                let days: Vec<&str> = [
                    (DAY_MONDAY, "Lun"),
                    (DAY_TUESDAY, "Mar"),
                    (DAY_WEDNESDAY, "Mié"),
                    (DAY_THURSDAY, "Jue"),
                    (DAY_FRIDAY, "Vie"),
                    (DAY_SATURDAY, "Sáb"),
                    (DAY_SUNDAY, "Dom"),
                ].iter()
                    .filter(|(bit, _)| (self.days_of_week & bit) != 0)
                    .map(|(_, name)| *name)
                    .collect();
                days.join(", ")
            }
        }
    }
}

/// Request to create a medication schedule
#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
pub struct CreateScheduleRequest {
    pub time_of_day: String, // "HH:MM" format
    #[serde(default = "default_days_of_week")]
    pub days_of_week: i32,
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

