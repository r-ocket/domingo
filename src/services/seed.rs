//! Database seeding for initial admin user

use crate::config::Config;
use crate::domain::{CreateCaregiverRequest, DomainResult, UserRole};
use crate::repositories::postgres::{CaregiverRepository, PostgresPool};
use crate::services::AuthService;
use crate::domain::Pagination;

/// Seed the database with an initial admin user if no users exist
pub async fn seed_admin_if_empty(pool: &PostgresPool, config: &Config) -> DomainResult<()> {
    // Check if any caregivers exist
    let pagination = Pagination { page: 1, per_page: 1 };
    let caregivers = CaregiverRepository::list(pool, &pagination).await?;
    
    if caregivers.total > 0 {
        tracing::info!("Database already has {} users, skipping seed", caregivers.total);
        return Ok(());
    }
    
    // Check if seed credentials are provided
    let (email, password) = match (&config.seed_admin_email, &config.seed_admin_password) {
        (Some(e), Some(p)) => (e.clone(), p.clone()),
        _ => {
            // Use defaults for development
            tracing::warn!("No SEED_ADMIN_EMAIL/PASSWORD set, using defaults: admin@walle.local / admin123");
            ("admin@walle.local".to_string(), "admin123".to_string())
        }
    };
    
    // Create admin user
    let req = CreateCaregiverRequest {
        name: "Administrador".to_string(),
        email: email.clone(),
        password,
    };
    
    let caregiver = AuthService::register(pool, &req).await?;
    
    // Upgrade to admin role
    CaregiverRepository::update_role(pool, caregiver.id, UserRole::Admin).await?;
    
    tracing::info!("✅ Created seed admin user: {}", email);
    tracing::info!("   Login at /login with the configured credentials");
    
    Ok(())
}

