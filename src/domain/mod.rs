//! Domain models - pure data structures representing core entities

mod caregiver;
mod elder;
mod contact;
mod location;
mod medication;
mod reminder;
mod ride;
mod call_session;
mod subscription;

pub use caregiver::*;
pub use elder::*;
pub use contact::*;
pub use location::*;
pub use medication::*;
pub use reminder::*;
pub use ride::*;
pub use call_session::*;
pub use subscription::*;

use serde::{Deserialize, Serialize};

/// Result type for domain operations
pub type DomainResult<T> = Result<T, DomainError>;

/// Domain-level errors
#[derive(Debug, thiserror::Error)]
pub enum DomainError {
    #[error("Entity not found: {0}")]
    NotFound(String),
    
    #[error("Validation error: {0}")]
    Validation(String),
    
    #[error("Unauthorized: {0}")]
    Unauthorized(String),
    
    #[error("Conflict: {0}")]
    Conflict(String),
    
    #[error("External service error: {0}")]
    ExternalService(String),
    
    #[error("Database error: {0}")]
    Database(String),
}

/// Pagination parameters
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Pagination {
    pub page: u32,
    pub per_page: u32,
}

impl Default for Pagination {
    fn default() -> Self {
        Self {
            page: 1,
            per_page: 20,
        }
    }
}

impl Pagination {
    pub fn offset(&self) -> i64 {
        ((self.page.saturating_sub(1)) * self.per_page) as i64
    }
    
    pub fn limit(&self) -> i64 {
        self.per_page as i64
    }
}

/// Paginated response wrapper
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Paginated<T> {
    pub items: Vec<T>,
    pub total: i64,
    pub page: u32,
    pub per_page: u32,
    pub total_pages: u32,
}

impl<T> Paginated<T> {
    pub fn new(items: Vec<T>, total: i64, pagination: &Pagination) -> Self {
        let total_pages = ((total as f64) / (pagination.per_page as f64)).ceil() as u32;
        Self {
            items,
            total,
            page: pagination.page,
            per_page: pagination.per_page,
            total_pages,
        }
    }
}

