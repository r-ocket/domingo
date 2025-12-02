//! Location management service

use uuid::Uuid;

use crate::domain::{
    CreateLocationRequest, DomainError, DomainResult, Location,
    Pagination, Paginated, UpdateLocationRequest,
};
use crate::repositories::postgres::{ElderRepository, LocationRepository, PostgresPool};

/// Location management service
pub struct LocationService;

impl LocationService {
    /// Create a new location for an elder
    pub async fn create_location(
        pool: &PostgresPool,
        elder_id: Uuid,
        caregiver_id: Uuid,
        is_admin: bool,
        req: &CreateLocationRequest,
    ) -> DomainResult<Location> {
        // Check authorization
        Self::check_elder_access(pool, elder_id, caregiver_id, is_admin).await?;
        
        // Validate
        if req.name.is_empty() {
            return Err(DomainError::Validation("Name is required".to_string()));
        }
        if req.address.is_empty() {
            return Err(DomainError::Validation("Address is required".to_string()));
        }
        
        LocationRepository::create(pool, elder_id, req).await
    }
    
    /// Get a location by ID
    pub async fn get_location(
        pool: &PostgresPool,
        location_id: Uuid,
        caregiver_id: Uuid,
        is_admin: bool,
    ) -> DomainResult<Location> {
        let location = LocationRepository::find_by_id(pool, location_id).await?;
        
        // Check authorization
        Self::check_elder_access(pool, location.elder_id, caregiver_id, is_admin).await?;
        
        Ok(location)
    }
    
    /// Get all locations for an elder (for AI tool)
    pub async fn get_all_locations(
        pool: &PostgresPool,
        elder_id: Uuid,
    ) -> DomainResult<Vec<Location>> {
        LocationRepository::list_all_by_elder(pool, elder_id).await
    }
    
    /// Get home location for an elder
    pub async fn get_home_location(
        pool: &PostgresPool,
        elder_id: Uuid,
    ) -> DomainResult<Option<Location>> {
        LocationRepository::find_home(pool, elder_id).await
    }
    
    /// List locations for an elder
    pub async fn list_locations(
        pool: &PostgresPool,
        elder_id: Uuid,
        caregiver_id: Uuid,
        is_admin: bool,
        pagination: &Pagination,
    ) -> DomainResult<Paginated<Location>> {
        // Check authorization
        Self::check_elder_access(pool, elder_id, caregiver_id, is_admin).await?;
        
        LocationRepository::list_by_elder(pool, elder_id, pagination).await
    }
    
    /// Update a location
    pub async fn update_location(
        pool: &PostgresPool,
        location_id: Uuid,
        caregiver_id: Uuid,
        is_admin: bool,
        req: &UpdateLocationRequest,
    ) -> DomainResult<Location> {
        let location = LocationRepository::find_by_id(pool, location_id).await?;
        
        // Check authorization
        Self::check_elder_access(pool, location.elder_id, caregiver_id, is_admin).await?;
        
        LocationRepository::update(pool, location_id, req).await
    }
    
    /// Delete a location
    pub async fn delete_location(
        pool: &PostgresPool,
        location_id: Uuid,
        caregiver_id: Uuid,
        is_admin: bool,
    ) -> DomainResult<()> {
        let location = LocationRepository::find_by_id(pool, location_id).await?;
        
        // Check authorization
        Self::check_elder_access(pool, location.elder_id, caregiver_id, is_admin).await?;
        
        LocationRepository::delete(pool, location_id).await
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
                "Not authorized to access this elder's locations".to_string(),
            ));
        }
        
        Ok(())
    }
}

