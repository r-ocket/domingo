//! Database connection pool

use deadpool_postgres::{Config, Pool, Runtime};
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{ClientConfig, DigitallySignedStruct, Error, SignatureScheme};
use std::sync::Arc;
use tokio_postgres_rustls::MakeRustlsConnect;

/// PostgreSQL connection pool wrapper
#[derive(Clone)]
pub struct PostgresPool {
    pool: Pool,
    database_url: String,
    tls_connector: MakeRustlsConnect,
}

/// A certificate verifier that accepts any certificate (sslmode=require behavior)
/// This provides encryption without certificate verification.
#[derive(Debug)]
struct AcceptAllVerifier;

impl ServerCertVerifier for AcceptAllVerifier {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, Error> {
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        vec![
            SignatureScheme::RSA_PKCS1_SHA256,
            SignatureScheme::RSA_PKCS1_SHA384,
            SignatureScheme::RSA_PKCS1_SHA512,
            SignatureScheme::ECDSA_NISTP256_SHA256,
            SignatureScheme::ECDSA_NISTP384_SHA384,
            SignatureScheme::ECDSA_NISTP521_SHA512,
            SignatureScheme::RSA_PSS_SHA256,
            SignatureScheme::RSA_PSS_SHA384,
            SignatureScheme::RSA_PSS_SHA512,
            SignatureScheme::ED25519,
        ]
    }
}

/// Create a TLS connector for PostgreSQL connections (sslmode=require)
/// This encrypts the connection but does not verify the server certificate.
fn create_tls_connector() -> MakeRustlsConnect {
    let config = ClientConfig::builder()
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(AcceptAllVerifier))
        .with_no_client_auth();
    
    MakeRustlsConnect::new(config)
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
        
        let tls_connector = create_tls_connector();
        let pool = cfg.create_pool(Some(Runtime::Tokio1), tls_connector.clone())?;
        
        // Test the connection
        let client = pool.get().await?;
        client.simple_query("SELECT 1").await?;
        
        tracing::info!("Database connection pool initialized with SSL");
        
        Ok(Self { 
            pool,
            database_url: database_url.to_string(),
            tls_connector,
        })
    }
    
    /// Run database migrations
    /// 
    /// This uses a separate connection to run migrations because refinery
    /// requires ownership of the client.
    pub async fn run_migrations(&self) -> anyhow::Result<()> {
        // Create a dedicated connection for migrations
        let (mut client, connection) = tokio_postgres::connect(&self.database_url, self.tls_connector.clone()).await?;
        
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

