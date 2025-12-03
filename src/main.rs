//! Voice AI Assistant for Elderly - Main Entry Point

use std::net::SocketAddr;
use std::sync::Arc;

use axum::{middleware as axum_middleware, Router};
use tokio::net::TcpListener;
use tower_http::cors::CorsLayer;
use tower_http::trace::{DefaultOnRequest, DefaultOnResponse, TraceLayer};
use tracing::Level;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

mod config;
mod domain;
mod migrations;
mod services;
mod repositories;
mod clients;
mod handlers;
mod middleware;
mod mcp;

use services::{ReminderScheduler, CallStateStore, SharedCallStateStore};

pub use config::Config;
use repositories::postgres::PostgresPool;

/// Application state shared across handlers
#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    pub db: PostgresPool,
    pub twilio: Arc<clients::TwilioClient>,
    pub openai: Arc<clients::OpenAIClient>,
    pub elevenlabs: Arc<clients::ElevenLabsClient>,
    pub uber: Arc<clients::UberClient>,
    pub stripe: Arc<clients::StripeClient>,
    pub templates: Arc<tera::Tera>,
    pub call_state: SharedCallStateStore,
}

impl AppState {
    pub async fn new(config: Config) -> anyhow::Result<Self> {
        // Initialize database pool
        let db = PostgresPool::new(&config.database_url).await?;
        
        // Run database migrations
        db.run_migrations().await?;
        
        // Initialize external clients
        let twilio = Arc::new(clients::TwilioClient::new(
            &config.twilio_account_sid,
            &config.twilio_auth_token,
            &config.twilio_phone_number,
        ));
        
        let openai = Arc::new(clients::OpenAIClient::new(&config.openai_api_key));
        
        let elevenlabs = Arc::new(clients::ElevenLabsClient::new(&config.elevenlabs_api_key));
        
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
        
        // Initialize call state store for live monitoring
        let call_state = Arc::new(CallStateStore::new());
        
        Ok(Self {
            config: Arc::new(config),
            db,
            twilio,
            openai,
            elevenlabs,
            uber,
            stripe,
            templates,
            call_state,
        })
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize tracing
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "asistente_domingo=debug,tower_http=debug".into()),
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
    
    // Seed admin user if database is empty
    if let Err(e) = services::seed_admin_if_empty(&state.db, &config).await {
        tracing::warn!("Failed to seed admin user: {}", e);
    }
    
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
    
    // Build router with middleware stack
    // Layer order: last added = outermost (runs first on request)
    let app = Router::new()
        .merge(handlers::api_routes())
        .merge(handlers::page_routes())
        .nest_service("/static", tower_http::services::ServeDir::new("static"))
        .layer(CorsLayer::permissive())
        // Tracing layer - runs after request ID is set
        .layer(
            TraceLayer::new_for_http()
                .on_request(DefaultOnRequest::new().level(Level::INFO))
                .on_response(
                    DefaultOnResponse::new()
                        .level(Level::INFO)
                        .latency_unit(tower_http::LatencyUnit::Millis)
                )
                .make_span_with(|request: &axum::http::Request<_>| {
                    // Include request ID in span if available
                    let request_id = request
                        .extensions()
                        .get::<middleware::RequestId>()
                        .map(|r| r.to_string())
                        .unwrap_or_else(|| "unknown".to_string());
                    
                    tracing::info_span!(
                        "http_request",
                        request_id = %request_id,
                        method = %request.method(),
                        path = %request.uri().path(),
                        version = ?request.version(),
                    )
                })
        )
        // Request ID middleware - runs first, sets ID in extensions
        .layer(axum_middleware::from_fn(middleware::request_id_middleware))
        .with_state(state);
    
    // Start server
    let listener = TcpListener::bind(addr).await?;
    tracing::info!("Listening on {}", addr);
    
    axum::serve(listener, app).await?;
    
    Ok(())
}

