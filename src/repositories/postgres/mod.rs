//! PostgreSQL repository implementations

mod pool;
mod caregiver_repo;
mod elder_repo;
mod contact_repo;
mod location_repo;
mod medication_repo;
mod reminder_repo;
mod ride_repo;
mod call_session_repo;
mod subscription_repo;

pub use pool::*;
pub use caregiver_repo::*;
pub use elder_repo::*;
pub use contact_repo::*;
pub use location_repo::*;
pub use medication_repo::*;
pub use reminder_repo::*;
pub use ride_repo::*;
pub use call_session_repo::*;
pub use subscription_repo::*;

use crate::domain::DomainError;

/// Convert database errors to domain errors
impl From<tokio_postgres::Error> for DomainError {
    fn from(err: tokio_postgres::Error) -> Self {
        DomainError::Database(err.to_string())
    }
}

impl From<deadpool_postgres::PoolError> for DomainError {
    fn from(err: deadpool_postgres::PoolError) -> Self {
        DomainError::Database(err.to_string())
    }
}

