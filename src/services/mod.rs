//! Service layer - business logic and use cases

mod auth;
mod elder_service;
mod contact_service;
mod location_service;
mod medication_service;
mod ride_service;
mod call_service;
mod reminder_scheduler;
mod seed;

pub use auth::*;
pub use elder_service::*;
pub use contact_service::*;
pub use location_service::*;
pub use medication_service::*;
pub use ride_service::*;
pub use call_service::*;
pub use reminder_scheduler::ReminderScheduler;
pub use reminder_scheduler::update_reminder_status;
pub use seed::seed_admin_if_empty;

