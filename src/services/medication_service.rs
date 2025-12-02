//! Medication management service

use chrono::{Local, NaiveTime, Timelike, Utc};
use uuid::Uuid;

use crate::domain::{
    CreateMedicationRequest, DomainError, DomainResult, MedicationWithSchedule,
    Pagination, Paginated, UpdateMedicationRequest, UpcomingMedication,
};
use crate::repositories::postgres::{ElderRepository, MedicationRepository, PostgresPool};

/// Medication management service
pub struct MedicationService;

impl MedicationService {
    /// Create a new medication with schedule
    pub async fn create_medication(
        pool: &PostgresPool,
        elder_id: Uuid,
        caregiver_id: Uuid,
        is_admin: bool,
        req: &CreateMedicationRequest,
    ) -> DomainResult<MedicationWithSchedule> {
        // Check authorization
        Self::check_elder_access(pool, elder_id, caregiver_id, is_admin).await?;
        
        // Validate
        if req.name.is_empty() {
            return Err(DomainError::Validation("Medication name is required".to_string()));
        }
        if req.dosage.is_empty() {
            return Err(DomainError::Validation("Dosage is required".to_string()));
        }
        if req.schedule_times.is_empty() {
            return Err(DomainError::Validation("At least one schedule time is required".to_string()));
        }
        
        // Validate time formats
        for time in &req.schedule_times {
            if NaiveTime::parse_from_str(time, "%H:%M").is_err() {
                return Err(DomainError::Validation(format!("Invalid time format: {}. Use HH:MM", time)));
            }
        }
        
        MedicationRepository::create(pool, elder_id, req).await
    }
    
    /// Get a medication by ID
    pub async fn get_medication(
        pool: &PostgresPool,
        medication_id: Uuid,
        caregiver_id: Uuid,
        is_admin: bool,
    ) -> DomainResult<MedicationWithSchedule> {
        let med = MedicationRepository::find_with_schedules(pool, medication_id).await?;
        
        // Check authorization
        Self::check_elder_access(pool, med.medication.elder_id, caregiver_id, is_admin).await?;
        
        Ok(med)
    }
    
    /// Get all medications for an elder (for AI tool)
    pub async fn get_all_medications(
        pool: &PostgresPool,
        elder_id: Uuid,
    ) -> DomainResult<Vec<MedicationWithSchedule>> {
        MedicationRepository::list_all_by_elder(pool, elder_id).await
    }
    
    /// Get upcoming medications for today
    pub async fn get_upcoming_medications(
        pool: &PostgresPool,
        elder_id: Uuid,
    ) -> DomainResult<Vec<UpcomingMedication>> {
        let now = Local::now();
        let current_time = NaiveTime::from_hms_opt(now.hour(), now.minute(), 0).unwrap();
        
        let all_schedules = MedicationRepository::get_all_schedules_by_elder(pool, elder_id).await?;
        
        let mut upcoming: Vec<UpcomingMedication> = all_schedules
            .iter()
            .filter(|(_, schedule)| schedule.time_of_day >= current_time)
            .map(|(med, schedule)| {
                let today = now.date_naive();
                let next_due = today.and_time(schedule.time_of_day);
                let next_due_utc = chrono::TimeZone::from_local_datetime(&Utc, &next_due)
                    .single()
                    .unwrap_or_else(Utc::now);
                
                UpcomingMedication {
                    medication_id: med.id,
                    medication_name: med.name.clone(),
                    dosage: med.dosage.clone(),
                    instructions: med.instructions.clone(),
                    scheduled_time: schedule.time_of_day,
                    next_due: next_due_utc,
                }
            })
            .collect();
        
        // Sort by scheduled time
        upcoming.sort_by(|a, b| a.scheduled_time.cmp(&b.scheduled_time));
        
        Ok(upcoming)
    }
    
    /// List medications for an elder
    pub async fn list_medications(
        pool: &PostgresPool,
        elder_id: Uuid,
        caregiver_id: Uuid,
        is_admin: bool,
        pagination: &Pagination,
    ) -> DomainResult<Paginated<MedicationWithSchedule>> {
        // Check authorization
        Self::check_elder_access(pool, elder_id, caregiver_id, is_admin).await?;
        
        MedicationRepository::list_by_elder(pool, elder_id, pagination).await
    }
    
    /// Update a medication
    pub async fn update_medication(
        pool: &PostgresPool,
        medication_id: Uuid,
        caregiver_id: Uuid,
        is_admin: bool,
        req: &UpdateMedicationRequest,
    ) -> DomainResult<MedicationWithSchedule> {
        let med = MedicationRepository::find_by_id(pool, medication_id).await?;
        
        // Check authorization
        Self::check_elder_access(pool, med.elder_id, caregiver_id, is_admin).await?;
        
        // Validate time formats if provided
        if let Some(times) = &req.schedule_times {
            for time in times {
                if NaiveTime::parse_from_str(time, "%H:%M").is_err() {
                    return Err(DomainError::Validation(format!("Invalid time format: {}. Use HH:MM", time)));
                }
            }
        }
        
        MedicationRepository::update(pool, medication_id, req).await
    }
    
    /// Delete a medication
    pub async fn delete_medication(
        pool: &PostgresPool,
        medication_id: Uuid,
        caregiver_id: Uuid,
        is_admin: bool,
    ) -> DomainResult<()> {
        let med = MedicationRepository::find_by_id(pool, medication_id).await?;
        
        // Check authorization
        Self::check_elder_access(pool, med.elder_id, caregiver_id, is_admin).await?;
        
        MedicationRepository::delete(pool, medication_id).await
    }
    
    /// Check if caregiver has access to elder
    async fn check_elder_access(
        pool: &PostgresPool,
        elder_id: Uuid,
        caregiver_id: Uuid,
        is_admin: bool,
    ) -> DomainResult<()> {
        if is_admin {
            return Ok(());
        }
        
        let elder = ElderRepository::find_by_id(pool, elder_id).await?;
        if elder.caregiver_id != caregiver_id {
            return Err(DomainError::Unauthorized(
                "Not authorized to access this elder's medications".to_string(),
            ));
        }
        
        Ok(())
    }
}

