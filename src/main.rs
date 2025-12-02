//! Voice AI Assistant for Elderly - Main Entry Point

use std::net::SocketAddr;
use std::sync::Arc;

use axum::Router;
use tokio::net::TcpListener;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

mod config;
mod domain;
mod services;
mod repositories;
mod clients;
mod handlers;
mod middleware;

use services::ReminderScheduler;

pub use config::Config;
use repositories::postgres::PostgresPool;

/// Application state shared across handlers
#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    pub db: PostgresPool,
    pub twilio: Arc<clients::TwilioClient>,
    pub openai: Arc<clients::OpenAIClient>,
    pub uber: Arc<clients::UberClient>,
    pub stripe: Arc<clients::StripeClient>,
    pub templates: Arc<tera::Tera>,
}

impl AppState {
    pub async fn new(config: Config) -> anyhow::Result<Self> {
        // Initialize database pool
        let db = PostgresPool::new(&config.database_url).await?;
        
        // Initialize external clients
        let twilio = Arc::new(clients::TwilioClient::new(
            &config.twilio_account_sid,
            &config.twilio_auth_token,
            &config.twilio_phone_number,
        ));
        
        let openai = Arc::new(clients::OpenAIClient::new(&config.openai_api_key));
        
        let uber = Arc::new(clients::UberClient::new(
            &config.uber_client_id,
            &config.uber_client_secret,
        ));
        
        let stripe = Arc::new(clients::StripeClient::new(
            &config.stripe_secret_key,
            &config.stripe_webhook_secret,
        ));
        
        // Initialize templates
        let templates = Arc::new(
            tera::Tera::new("src/templates/**/*")
                .expect("Failed to load templates")
        );
        
        Ok(Self {
            config: Arc::new(config),
            db,
            twilio,
            openai,
            uber,
            stripe,
            templates,
        })
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize tracing
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "walle=debug,tower_http=debug".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();
    
    // Load configuration
    dotenvy::dotenv().ok();
    let config = Config::from_env()?;
    let addr = SocketAddr::new(config.host.parse()?, config.port);
    
    tracing::info!("Starting server on {}", addr);
    
    // Initialize application state
    let state = AppState::new(config.clone()).await?;
    
    // Start medication reminder scheduler as background task
    let scheduler = Arc::new(ReminderScheduler::new(
        state.db.clone(),
        state.twilio.clone(),
        config.base_url.clone(),
    ));
    tokio::spawn(async move {
        scheduler.start().await;
    });
    tracing::info!("Medication reminder scheduler started");
    
    // Build router
    let app = Router::new()
        .merge(handlers::api_routes())
        .merge(handlers::page_routes())
        .nest_service("/static", tower_http::services::ServeDir::new("static"))
        .layer(TraceLayer::new_for_http())
        .layer(CorsLayer::permissive())
        .with_state(state);
    
    // Start server
    let listener = TcpListener::bind(addr).await?;
    tracing::info!("Listening on {}", addr);
    
    axum::serve(listener, app).await?;
    
    Ok(())
}

