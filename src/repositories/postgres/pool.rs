//! Database connection pool

use deadpool_postgres::{Config, Pool, Runtime};
use tokio_postgres::NoTls;

/// PostgreSQL connection pool wrapper
#[derive(Clone)]
pub struct PostgresPool {
    pool: Pool,
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
        
        Ok(Self { pool })
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

