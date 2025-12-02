//! Medication reminder scheduler

use std::sync::Arc;
use chrono::{Local, NaiveTime, Timelike};
use tokio::time::{interval, Duration};
use uuid::Uuid;

use crate::clients::TwilioClient;
use crate::domain::{
    CreateReminderLogRequest, DeliveryMethod, DomainResult, ReminderStatus,
    UpdateReminderLogRequest,
};
use crate::repositories::postgres::{
    ElderRepository, MedicationRepository, PostgresPool, ReminderLogRepository,
};

/// Background scheduler for medication reminders
pub struct ReminderScheduler {
    pool: PostgresPool,
    twilio: Arc<TwilioClient>,
    base_url: String,
}

impl ReminderScheduler {
    /// Create a new reminder scheduler
    pub fn new(pool: PostgresPool, twilio: Arc<TwilioClient>, base_url: String) -> Self {
        Self { pool, twilio, base_url }
    }
    
    /// Start the scheduler background task
    pub async fn start(self: Arc<Self>) {
        let mut interval = interval(Duration::from_secs(60)); // Check every minute
        
        loop {
            interval.tick().await;
            
            if let Err(e) = self.check_and_send_reminders().await {
                tracing::error!("Error checking reminders: {}", e);
            }
        }
    }
    
    /// Check for due reminders and send them
    async fn check_and_send_reminders(&self) -> DomainResult<()> {
        let now = Local::now();
        let current_time = NaiveTime::from_hms_opt(now.hour(), now.minute(), 0).unwrap();
        
        // Get all elders
        let pagination = crate::domain::Pagination { page: 1, per_page: 1000 };
        let elders = ElderRepository::list(&self.pool, &pagination).await?;
        
        for elder in elders.items {
            if elder.status != crate::domain::ElderStatus::Active {
                continue;
            }
            
            // Get all medication schedules for this elder
            let schedules = MedicationRepository::get_all_schedules_by_elder(&self.pool, elder.id).await?;
            
            for (medication, schedule) in schedules {
                // Check if this schedule matches the current time (within 1 minute tolerance)
                let schedule_time = schedule.time_of_day;
                if schedule_time.hour() != current_time.hour() 
                    || schedule_time.minute() != current_time.minute() {
                    continue;
                }
                
                // Check if we already sent a reminder recently
                if ReminderLogRepository::was_recently_sent(&self.pool, schedule.id, 5).await? {
                    continue;
                }
                
                // Create reminder log
                let log_req = CreateReminderLogRequest {
                    schedule_id: schedule.id,
                    elder_id: elder.id,
                    medication_name: medication.name.clone(),
                    delivery_method: DeliveryMethod::Call,
                };
                
                let log = ReminderLogRepository::create(&self.pool, &log_req).await?;
                
                // Send reminder call
                match self.send_reminder_call(&elder.phone_number, &medication.name, &medication.dosage, log.id).await {
                    Ok(call_sid) => {
                        let update_req = UpdateReminderLogRequest {
                            status: ReminderStatus::Delivered,
                            twilio_call_sid: Some(call_sid),
                            metadata: None,
                        };
                        ReminderLogRepository::update(&self.pool, log.id, &update_req).await?;
                        tracing::info!(
                            "Sent medication reminder to {} for {}",
                            elder.name, medication.name
                        );
                    }
                    Err(e) => {
                        let update_req = UpdateReminderLogRequest {
                            status: ReminderStatus::Failed,
                            twilio_call_sid: None,
                            metadata: Some(serde_json::json!({ "error": e.to_string() })),
                        };
                        ReminderLogRepository::update(&self.pool, log.id, &update_req).await?;
                        tracing::error!(
                            "Failed to send reminder to {} for {}: {}",
                            elder.name, medication.name, e
                        );
                    }
                }
            }
        }
        
        Ok(())
    }
    
    /// Send a reminder call via Twilio
    async fn send_reminder_call(
        &self,
        phone_number: &str,
        medication_name: &str,
        dosage: &str,
        reminder_id: Uuid,
    ) -> Result<String, crate::clients::TwilioError> {
        let twiml_url = format!(
            "{}/api/twilio/reminder-twiml?reminder_id={}&med={}&dosage={}",
            self.base_url,
            reminder_id,
            urlencoding::encode(medication_name),
            urlencoding::encode(dosage)
        );
        
        let response = self.twilio.make_call(phone_number, &twiml_url).await?;
        Ok(response.sid)
    }
}

/// Update reminder status after call completion
pub async fn update_reminder_status(
    pool: &PostgresPool,
    reminder_id: Uuid,
    confirmed: bool,
) -> DomainResult<()> {
    let status = if confirmed {
        ReminderStatus::Confirmed
    } else {
        ReminderStatus::NoAnswer
    };
    
    let update_req = UpdateReminderLogRequest {
        status,
        twilio_call_sid: None,
        metadata: None,
    };
    
    ReminderLogRepository::update(pool, reminder_id, &update_req).await?;
    
    Ok(())
}

