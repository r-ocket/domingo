//! Elder management service

use uuid::Uuid;

use crate::domain::{
    CreateElderRequest, DomainError, DomainResult, Elder, ElderSummary,
    Pagination, Paginated, UpdateElderRequest,
};
use crate::repositories::postgres::{
    CallSessionRepository, ContactRepository, ElderRepository, LocationRepository,
    MedicationRepository, PostgresPool,
};

/// Elder management service
pub struct ElderService;

impl ElderService {
    /// Create a new elder profile for a caregiver
    pub async fn create_elder(
        pool: &PostgresPool,
        caregiver_id: Uuid,
        req: &CreateElderRequest,
    ) -> DomainResult<Elder> {
        // Validate phone number
        if req.phone_number.is_empty() {
            return Err(DomainError::Validation("Phone number is required".to_string()));
        }
        
        ElderRepository::create(pool, caregiver_id, req).await
    }
    
    /// Get elder by ID (with authorization check)
    pub async fn get_elder(
        pool: &PostgresPool,
        elder_id: Uuid,
        caregiver_id: Uuid,
        is_admin: bool,
    ) -> DomainResult<Elder> {
        let elder = ElderRepository::find_by_id(pool, elder_id).await?;
        
        // Check authorization
        if !is_admin && elder.caregiver_id != caregiver_id {
            return Err(DomainError::Unauthorized(
                "Not authorized to access this elder".to_string(),
            ));
        }
        
        Ok(elder)
    }
    
    /// Get elder for a caregiver
    pub async fn get_elder_for_caregiver(
        pool: &PostgresPool,
        caregiver_id: Uuid,
    ) -> DomainResult<Option<Elder>> {
        ElderRepository::find_by_caregiver(pool, caregiver_id).await
    }
    
    /// Get elder by phone number (for incoming calls)
    pub async fn get_elder_by_phone(pool: &PostgresPool, phone: &str) -> DomainResult<Elder> {
        ElderRepository::find_by_phone(pool, phone).await
    }
    
    /// Find elder by ID (no auth check - for internal/debug use)
    pub async fn find_elder_by_id(pool: &PostgresPool, elder_id: Uuid) -> DomainResult<Elder> {
        ElderRepository::find_by_id(pool, elder_id).await
    }
    
    /// Update elder profile
    pub async fn update_elder(
        pool: &PostgresPool,
        elder_id: Uuid,
        caregiver_id: Uuid,
        is_admin: bool,
        req: &UpdateElderRequest,
    ) -> DomainResult<Elder> {
        // Check authorization
        let elder = ElderRepository::find_by_id(pool, elder_id).await?;
        if !is_admin && elder.caregiver_id != caregiver_id {
            return Err(DomainError::Unauthorized(
                "Not authorized to update this elder".to_string(),
            ));
        }
        
        ElderRepository::update(pool, elder_id, req).await
    }
    
    /// Get elder summary (dashboard data)
    pub async fn get_elder_summary(
        pool: &PostgresPool,
        elder_id: Uuid,
        caregiver_id: Uuid,
        is_admin: bool,
    ) -> DomainResult<ElderSummary> {
        let elder = Self::get_elder(pool, elder_id, caregiver_id, is_admin).await?;
        
        let contacts_count = ContactRepository::count_by_elder(pool, elder_id).await?;
        let locations_count = LocationRepository::count_by_elder(pool, elder_id).await?;
        let medications_count = MedicationRepository::count_by_elder(pool, elder_id).await?;
        
        let last_call = CallSessionRepository::get_last_for_elder(pool, elder_id).await?;
        let last_call_at = last_call.map(|c| c.started_at);
        
        // TODO: Calculate next reminder time based on medication schedules
        let next_reminder_at = None;
        
        Ok(ElderSummary {
            elder,
            contacts_count,
            locations_count,
            medications_count,
            last_call_at,
            next_reminder_at,
        })
    }
    
    /// List all elders (admin only)
    pub async fn list_elders(
        pool: &PostgresPool,
        pagination: &Pagination,
    ) -> DomainResult<Paginated<Elder>> {
        ElderRepository::list(pool, pagination).await
    }
    
    /// Get count of active elders (admin stats)
    pub async fn count_active_elders(pool: &PostgresPool) -> DomainResult<i64> {
        ElderRepository::count_active(pool).await
    }
}

