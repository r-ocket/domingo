//! Configuration module - loads settings from environment variables

use std::env;

/// Application configuration loaded from environment variables
#[derive(Debug, Clone)]
pub struct Config {
    // Server
    pub host: String,
    pub port: u16,
    pub base_url: String,
    
    // Database
    pub database_url: String,
    
    // Twilio
    pub twilio_account_sid: String,
    pub twilio_auth_token: String,
    pub twilio_phone_number: String,
    
    // OpenAI
    pub openai_api_key: String,
    
    // Gemini API (Gemini Live)
    pub gemini_api_key: String,

    // ElevenLabs
    pub elevenlabs_api_key: String,
    /// ElevenLabs Conversational AI Agent ID (create in ElevenLabs dashboard)
    /// The agent should be configured with Spanish language, Claude Sonnet 4.5, and Juan voice
    pub elevenlabs_agent_id: Option<String>,
    /// Secret token for ElevenLabs MCP requests (configure same value in ElevenLabs dashboard)
    pub mcp_secret_token: Option<String>,
    
    // Uber
    pub uber_client_id: String,
    pub uber_client_secret: String,
    
    // Stripe
    pub stripe_secret_key: String,
    pub stripe_webhook_secret: String,
    pub stripe_price_id: String,
    
    // Google Maps
    pub google_maps_api_key: String,
    
    // Session
    pub session_secret: String,
    
    // Seed admin (optional - creates admin on startup if no users exist)
    pub seed_admin_email: Option<String>,
    pub seed_admin_password: Option<String>,
}

impl Config {
    /// Load configuration from environment variables
    pub fn from_env() -> anyhow::Result<Self> {
        Ok(Self {
            // Server
            host: env::var("HOST").unwrap_or_else(|_| "127.0.0.1".to_string()),
            port: env::var("PORT")
                .unwrap_or_else(|_| "3000".to_string())
                .parse()?,
            base_url: env::var("BASE_URL").unwrap_or_else(|_| "http://localhost:3000".to_string()),
            
            // Database
            database_url: env::var("DATABASE_URL")
                .unwrap_or_else(|_| "postgres://postgres:postgres@localhost/asistente_domingo".to_string()),
            
            // Twilio
            twilio_account_sid: env::var("TWILIO_ACCOUNT_SID").unwrap_or_default(),
            twilio_auth_token: env::var("TWILIO_AUTH_TOKEN").unwrap_or_default(),
            twilio_phone_number: env::var("TWILIO_PHONE_NUMBER").unwrap_or_default(),
            
            // OpenAI
            openai_api_key: env::var("OPENAI_API_KEY").unwrap_or_default(),

            // Gemini API (prefer GEMINI_API_KEY, fallback GOOGLE_API_KEY)
            gemini_api_key: env::var("GEMINI_API_KEY")
                .or_else(|_| env::var("GOOGLE_API_KEY"))
                .unwrap_or_default(),
            
            // ElevenLabs
            elevenlabs_api_key: env::var("ELEVENLABS_API_KEY").unwrap_or_default(),
            elevenlabs_agent_id: env::var("ELEVENLABS_AGENT_ID").ok(),
            mcp_secret_token: env::var("MCP_SECRET_TOKEN").ok(),
            
            // Uber
            uber_client_id: env::var("UBER_CLIENT_ID").unwrap_or_default(),
            uber_client_secret: env::var("UBER_CLIENT_SECRET").unwrap_or_default(),
            
            // Stripe
            stripe_secret_key: env::var("STRIPE_SECRET_KEY").unwrap_or_default(),
            stripe_webhook_secret: env::var("STRIPE_WEBHOOK_SECRET").unwrap_or_default(),
            stripe_price_id: env::var("STRIPE_PRICE_ID").unwrap_or_default(),
            
            // Google Maps
            google_maps_api_key: env::var("GOOGLE_MAPS_API_KEY").unwrap_or_default(),
            
            // Session
            session_secret: env::var("SESSION_SECRET")
                .unwrap_or_else(|_| "development_secret_change_in_production".to_string()),
            
            // Seed admin (creates admin on startup if DB is empty)
            seed_admin_email: env::var("SEED_ADMIN_EMAIL").ok(),
            seed_admin_password: env::var("SEED_ADMIN_PASSWORD").ok(),
        })
    }
}

