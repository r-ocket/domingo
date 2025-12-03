//! HTTP and WebSocket handlers

mod auth;
mod elder;
mod contacts;
mod locations;
mod medications;
mod logs;
mod twilio_webhooks;
mod stripe_webhooks;
mod uber_webhooks;
mod voice_stream;
mod admin;
mod pages;
mod billing;
mod debug;

use axum::{
    routing::{get, post, put, delete},
    Router,
};

use crate::AppState;

/// Build API routes
pub fn api_routes() -> Router<AppState> {
    Router::new()
        // Auth
        .route("/api/auth/register", post(auth::register))
        .route("/api/auth/login", post(auth::login))
        .route("/api/auth/logout", post(auth::logout))
        .route("/api/profile", get(auth::get_profile))
        
        // Elder
        .route("/api/elder", post(elder::create_elder))
        .route("/api/elder/:elder_id", get(elder::get_elder))
        .route("/api/elder/:elder_id", put(elder::update_elder))
        
        // Caregiver elders (multi-elder support)
        .route("/api/caregiver/elders", get(elder::list_caregiver_elders))
        .route("/api/caregiver/select-elder", post(elder::select_elder))
        
        // Contacts
        .route("/api/elder/:elder_id/contacts", get(contacts::list_contacts))
        .route("/api/elder/:elder_id/contacts", post(contacts::create_contact))
        .route("/api/elder/:elder_id/contacts/emergency", get(contacts::get_emergency_contacts))
        .route("/api/elder/:elder_id/contacts/:contact_id", get(contacts::get_contact))
        .route("/api/elder/:elder_id/contacts/:contact_id", put(contacts::update_contact))
        .route("/api/elder/:elder_id/contacts/:contact_id", delete(contacts::delete_contact))
        
        // Locations
        .route("/api/elder/:elder_id/locations", get(locations::list_locations))
        .route("/api/elder/:elder_id/locations", post(locations::create_location))
        .route("/api/elder/:elder_id/locations/home", get(locations::get_home_location))
        .route("/api/elder/:elder_id/locations/:location_id", get(locations::get_location))
        .route("/api/elder/:elder_id/locations/:location_id", put(locations::update_location))
        .route("/api/elder/:elder_id/locations/:location_id", delete(locations::delete_location))
        
        // Medications
        .route("/api/elder/:elder_id/medications", get(medications::list_medications))
        .route("/api/elder/:elder_id/medications", post(medications::create_medication))
        .route("/api/elder/:elder_id/medications/:medication_id", get(medications::get_medication))
        .route("/api/elder/:elder_id/medications/:medication_id", put(medications::update_medication))
        .route("/api/elder/:elder_id/medications/:medication_id", delete(medications::delete_medication))
        
        // Logs
        .route("/api/elder/:elder_id/call-logs", get(logs::list_call_logs))
        .route("/api/elder/:elder_id/call-logs/:call_id", get(logs::get_call_log))
        .route("/api/elder/:elder_id/ride-logs", get(logs::list_ride_logs))
        .route("/api/elder/:elder_id/rides/active", get(logs::get_active_ride))
        .route("/api/elder/:elder_id/reminder-logs", get(logs::list_reminder_logs))
        
        // Billing
        .route("/api/billing/subscribe", post(billing::create_checkout))
        .route("/api/billing/status", get(billing::get_billing_status))
        .route("/api/billing/portal", post(billing::create_portal_session))
        
        // Twilio webhooks
        .route("/api/twilio/voice", post(twilio_webhooks::incoming_call))
        .route("/api/twilio/media-stream/:call_sid", get(voice_stream::media_stream))
        .route("/api/twilio/status", post(twilio_webhooks::call_status))
        .route("/api/twilio/reminder-twiml", post(twilio_webhooks::reminder_twiml))
        .route("/api/twilio/reminder-confirm", post(twilio_webhooks::reminder_confirm))
        .route("/api/twilio/outbound-voice", post(twilio_webhooks::outbound_voice))
        
        // Stripe webhooks
        .route("/api/stripe/webhook", post(stripe_webhooks::handle_webhook))
        
        // Uber webhooks
        .route("/api/uber/webhook", post(uber_webhooks::handle_webhook))
        
        // Admin routes
        .route("/api/admin/caregivers", get(admin::list_caregivers))
        .route("/api/admin/caregivers/:id", get(admin::get_caregiver))
        .route("/api/admin/caregivers/:id", put(admin::update_caregiver))
        .route("/api/admin/elders", get(admin::list_elders))
        .route("/api/admin/elders/:id", get(admin::get_elder))
        .route("/api/admin/logs/calls", get(admin::list_all_call_logs))
        .route("/api/admin/logs/rides", get(admin::list_all_ride_logs))
        .route("/api/admin/stats", get(admin::get_stats))
        
        // Live call monitoring
        .route("/api/admin/calls", get(admin::list_active_calls))
        .route("/api/admin/calls/live", get(admin::calls_sse))
        .route("/api/admin/calls/:call_sid", get(admin::get_call_state))
        .route("/api/admin/calls/:call_sid/stream", get(admin::call_stream_sse))
        
        // Debug routes (no auth - development only)
        .route("/debug/caregiver", post(debug::create_caregiver))
        .route("/debug/elder", post(debug::create_elder))
        .route("/debug/call-elder", post(debug::initiate_call))
        .route("/debug/health", get(debug::health))
}

/// Build page routes (HTML)
pub fn page_routes() -> Router<AppState> {
    Router::new()
        // Public pages
        .route("/", get(pages::home))
        .route("/login", get(pages::login_page))
        .route("/register", get(pages::register_page))
        
        // Caregiver pages
        .route("/dashboard", get(pages::dashboard))
        .route("/elder/profile", get(pages::elder_profile))
        .route("/elder/contacts", get(pages::contacts_page))
        .route("/elder/locations", get(pages::locations_page))
        .route("/elder/medications", get(pages::medications_page))
        .route("/elder/logs/calls", get(pages::call_logs_page))
        .route("/elder/logs/rides", get(pages::ride_logs_page))
        .route("/elder/logs/reminders", get(pages::reminder_logs_page))
        .route("/billing", get(pages::billing_page))
        
        // Admin pages
        .route("/admin", get(pages::admin_dashboard))
        .route("/admin/caregivers", get(pages::admin_caregivers))
        .route("/admin/elders", get(pages::admin_elders))
        .route("/admin/logs", get(pages::admin_logs))
        .route("/admin/calls", get(pages::admin_calls))
        .route("/admin/debug", get(pages::admin_debug))
}

