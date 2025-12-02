//! Database connection pool

use deadpool_postgres::{Config, Pool, Runtime};
use tokio_postgres::NoTls;

/// PostgreSQL connection pool wrapper
#[derive(Clone)]
pub struct PostgresPool {
    pool: Pool,
    database_url: String,
}

impl PostgresPool {
    /// Create a new connection pool from database URL
    pub async fn new(database_url: &str) -> anyhow::Result<Self> {
        let mut cfg = Config::new();
        
        // Parse the database URL
        let url = url::Url::parse(database_url)?;
        
        cfg.host = url.host_str().map(String::from);
        cfg.port = url.port();
        cfg.user = if url.username().is_empty() {
            None
        } else {
            Some(url.username().to_string())
        };
        cfg.password = url.password().map(String::from);
        cfg.dbname = Some(url.path().trim_start_matches('/').to_string());
        
        let pool = cfg.create_pool(Some(Runtime::Tokio1), NoTls)?;
        
        // Test the connection
        let client = pool.get().await?;
        client.simple_query("SELECT 1").await?;
        
        tracing::info!("Database connection pool initialized");
        
        Ok(Self { 
            pool,
            database_url: database_url.to_string(),
        })
    }
    
    /// Run database migrations
    /// 
    /// This uses a separate connection to run migrations because refinery
    /// requires ownership of the client.
    pub async fn run_migrations(&self) -> anyhow::Result<()> {
        // Create a dedicated connection for migrations
        let (mut client, connection) = tokio_postgres::connect(&self.database_url, NoTls).await?;
        
        // Spawn the connection handler
        tokio::spawn(async move {
            if let Err(e) = connection.await {
                tracing::error!("Migration connection error: {}", e);
            }
        });
        
        crate::migrations::run(&mut client).await
    }
    
    /// Get a connection from the pool
    pub async fn get(&self) -> Result<deadpool_postgres::Object, deadpool_postgres::PoolError> {
        self.pool.get().await
    }
    
    /// Get the underlying pool
    pub fn inner(&self) -> &Pool {
        &self.pool
    }
}

