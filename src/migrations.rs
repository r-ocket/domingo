//! Database migrations using refinery
//!
//! Migrations are embedded at compile time from the `migrations/` directory.
//! Files must be named `V{version}__{name}.sql` (e.g., V1__initial_schema.sql)

use refinery::embed_migrations;
use tokio_postgres::Client;

// Embed all migrations from the migrations directory at compile time
embed_migrations!("migrations");

/// Run all pending migrations
pub async fn run(client: &mut Client) -> anyhow::Result<()> {
    tracing::info!("Running database migrations...");
    
    let report = migrations::runner()
        .run_async(client)
        .await?;
    
    if report.applied_migrations().is_empty() {
        tracing::info!("No new migrations to apply");
    } else {
        for migration in report.applied_migrations() {
            tracing::info!(
                "Applied migration: V{} - {}",
                migration.version(),
                migration.name()
            );
        }
    }
    
    Ok(())
}

