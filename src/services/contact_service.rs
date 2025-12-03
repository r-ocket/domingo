//! Contact management service

use uuid::Uuid;

use crate::domain::{
    Contact, CreateContactRequest, DomainError, DomainResult,
    Pagination, Paginated, UpdateContactRequest,
};
use crate::repositories::postgres::{ContactRepository, ElderRepository, PostgresPool};

/// Contact management service
pub struct ContactService;

impl ContactService {
    /// Create a new contact for an elder
    pub async fn create_contact(
        pool: &PostgresPool,
        elder_id: Uuid,
        caregiver_id: Uuid,
        is_admin: bool,
        req: &CreateContactRequest,
    ) -> DomainResult<Contact> {
        // Check authorization
        Self::check_elder_access(pool, elder_id, caregiver_id, is_admin).await?;
        
        // Validate
        if req.name.is_empty() {
            return Err(DomainError::Validation("Name is required".to_string()));
        }
        if req.phone.is_empty() {
            return Err(DomainError::Validation("Phone is required".to_string()));
        }
        
        ContactRepository::create(pool, elder_id, req).await
    }
    
    /// Get a contact by ID
    pub async fn get_contact(
        pool: &PostgresPool,
        contact_id: Uuid,
        caregiver_id: Uuid,
        is_admin: bool,
    ) -> DomainResult<Contact> {
        let contact = ContactRepository::find_by_id(pool, contact_id).await?;
        
        // Check authorization
        Self::check_elder_access(pool, contact.elder_id, caregiver_id, is_admin).await?;
        
        Ok(contact)
    }
    
    /// Search contacts by name or relationship
    pub async fn search_contacts(
        pool: &PostgresPool,
        elder_id: Uuid,
        query: &str,
    ) -> DomainResult<Vec<Contact>> {
        ContactRepository::find_by_name(pool, elder_id, query).await
    }
    
    /// Get emergency contacts
    pub async fn get_emergency_contacts(
        pool: &PostgresPool,
        elder_id: Uuid,
    ) -> DomainResult<Vec<Contact>> {
        ContactRepository::find_emergency(pool, elder_id).await
    }
    
    /// Get all contacts for an elder (no auth check - used for voice context)
    pub async fn get_all_contacts(
        pool: &PostgresPool,
        elder_id: Uuid,
    ) -> DomainResult<Vec<Contact>> {
        ContactRepository::find_all_by_elder(pool, elder_id).await
    }
    
    /// List contacts for an elder
    pub async fn list_contacts(
        pool: &PostgresPool,
        elder_id: Uuid,
        caregiver_id: Uuid,
        is_admin: bool,
        pagination: &Pagination,
    ) -> DomainResult<Paginated<Contact>> {
        // Check authorization
        Self::check_elder_access(pool, elder_id, caregiver_id, is_admin).await?;
        
        ContactRepository::list_by_elder(pool, elder_id, pagination).await
    }
    
    /// Update a contact
    pub async fn update_contact(
        pool: &PostgresPool,
        contact_id: Uuid,
        caregiver_id: Uuid,
        is_admin: bool,
        req: &UpdateContactRequest,
    ) -> DomainResult<Contact> {
        let contact = ContactRepository::find_by_id(pool, contact_id).await?;
        
        // Check authorization
        Self::check_elder_access(pool, contact.elder_id, caregiver_id, is_admin).await?;
        
        ContactRepository::update(pool, contact_id, req).await
    }
    
    /// Delete a contact
    pub async fn delete_contact(
        pool: &PostgresPool,
        contact_id: Uuid,
        caregiver_id: Uuid,
        is_admin: bool,
    ) -> DomainResult<()> {
        let contact = ContactRepository::find_by_id(pool, contact_id).await?;
        
        // Check authorization
        Self::check_elder_access(pool, contact.elder_id, caregiver_id, is_admin).await?;
        
        ContactRepository::delete(pool, contact_id).await
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
                "Not authorized to access this elder's contacts".to_string(),
            ));
        }
        
        Ok(())
    }
}

