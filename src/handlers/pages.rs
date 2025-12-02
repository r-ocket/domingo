//! HTML page handlers

use axum::{
    extract::State,
    response::{Html, IntoResponse, Redirect},
};
use axum_extra::extract::CookieJar;
use tera::Context;

use crate::middleware::{OptionalAuthUser, SESSION_COOKIE};
use crate::services::{decode_session, ElderService};
use crate::AppState;

/// Home page
pub async fn home(
    State(state): State<AppState>,
    auth: OptionalAuthUser,
) -> impl IntoResponse {
    if auth.session.is_some() {
        return Redirect::to("/dashboard").into_response();
    }
    
    let mut context = Context::new();
    context.insert("title", "Walle - Voice AI Assistant for Elderly");
    
    match state.templates.render("home.html", &context) {
        Ok(html) => Html(html).into_response(),
        Err(e) => {
            tracing::error!("Template error: {}", e);
            Html("<h1>Error loading page</h1>").into_response()
        }
    }
}

/// Login page
pub async fn login_page(
    State(state): State<AppState>,
    auth: OptionalAuthUser,
) -> impl IntoResponse {
    if auth.session.is_some() {
        return Redirect::to("/dashboard").into_response();
    }
    
    let mut context = Context::new();
    context.insert("title", "Login - Walle");
    
    match state.templates.render("login.html", &context) {
        Ok(html) => Html(html).into_response(),
        Err(e) => {
            tracing::error!("Template error: {}", e);
            Html("<h1>Error loading page</h1>").into_response()
        }
    }
}

/// Register page
pub async fn register_page(
    State(state): State<AppState>,
    auth: OptionalAuthUser,
) -> impl IntoResponse {
    if auth.session.is_some() {
        return Redirect::to("/dashboard").into_response();
    }
    
    let mut context = Context::new();
    context.insert("title", "Register - Walle");
    
    match state.templates.render("register.html", &context) {
        Ok(html) => Html(html).into_response(),
        Err(e) => {
            tracing::error!("Template error: {}", e);
            Html("<h1>Error loading page</h1>").into_response()
        }
    }
}

/// Dashboard page
pub async fn dashboard(
    State(state): State<AppState>,
    jar: CookieJar,
) -> impl IntoResponse {
    let session = match get_session(&state, &jar) {
        Some(s) => s,
        None => return Redirect::to("/login").into_response(),
    };
    
    let mut context = Context::new();
    context.insert("title", "Dashboard - Walle");
    context.insert("is_admin", &(session.role == crate::domain::UserRole::Admin));
    
    // Get elder summary if exists
    if let Ok(Some(elder)) = ElderService::get_elder_for_caregiver(&state.db, session.caregiver_id).await {
        if let Ok(summary) = ElderService::get_elder_summary(&state.db, elder.id, session.caregiver_id, false).await {
            context.insert("elder", &summary.elder);
            context.insert("contacts_count", &summary.contacts_count);
            context.insert("locations_count", &summary.locations_count);
            context.insert("medications_count", &summary.medications_count);
            context.insert("last_call_at", &summary.last_call_at.map(|t| t.to_rfc3339()));
        }
    }
    
    match state.templates.render("dashboard.html", &context) {
        Ok(html) => Html(html).into_response(),
        Err(e) => {
            tracing::error!("Template error: {}", e);
            Html("<h1>Error loading page</h1>").into_response()
        }
    }
}

/// Elder profile page
pub async fn elder_profile(
    State(state): State<AppState>,
    jar: CookieJar,
) -> impl IntoResponse {
    let session = match get_session(&state, &jar) {
        Some(s) => s,
        None => return Redirect::to("/login").into_response(),
    };
    
    let mut context = Context::new();
    context.insert("title", "Elder Profile - Walle");
    context.insert("is_admin", &(session.role == crate::domain::UserRole::Admin));
    
    if let Ok(Some(elder)) = ElderService::get_elder_for_caregiver(&state.db, session.caregiver_id).await {
        context.insert("elder", &elder);
    }
    
    match state.templates.render("elder/profile.html", &context) {
        Ok(html) => Html(html).into_response(),
        Err(e) => {
            tracing::error!("Template error: {}", e);
            Html("<h1>Error loading page</h1>").into_response()
        }
    }
}

/// Contacts page
pub async fn contacts_page(
    State(state): State<AppState>,
    jar: CookieJar,
) -> impl IntoResponse {
    let session = match get_session(&state, &jar) {
        Some(s) => s,
        None => return Redirect::to("/login").into_response(),
    };
    
    let mut context = Context::new();
    context.insert("title", "Contacts - Walle");
    context.insert("is_admin", &(session.role == crate::domain::UserRole::Admin));
    
    if let Ok(Some(elder)) = ElderService::get_elder_for_caregiver(&state.db, session.caregiver_id).await {
        context.insert("elder_id", &elder.id.to_string());
    }
    
    match state.templates.render("elder/contacts.html", &context) {
        Ok(html) => Html(html).into_response(),
        Err(e) => {
            tracing::error!("Template error: {}", e);
            Html("<h1>Error loading page</h1>").into_response()
        }
    }
}

/// Locations page
pub async fn locations_page(
    State(state): State<AppState>,
    jar: CookieJar,
) -> impl IntoResponse {
    let session = match get_session(&state, &jar) {
        Some(s) => s,
        None => return Redirect::to("/login").into_response(),
    };
    
    let mut context = Context::new();
    context.insert("title", "Locations - Walle");
    context.insert("is_admin", &(session.role == crate::domain::UserRole::Admin));
    
    if let Ok(Some(elder)) = ElderService::get_elder_for_caregiver(&state.db, session.caregiver_id).await {
        context.insert("elder_id", &elder.id.to_string());
    }
    
    match state.templates.render("elder/locations.html", &context) {
        Ok(html) => Html(html).into_response(),
        Err(e) => {
            tracing::error!("Template error: {}", e);
            Html("<h1>Error loading page</h1>").into_response()
        }
    }
}

/// Medications page
pub async fn medications_page(
    State(state): State<AppState>,
    jar: CookieJar,
) -> impl IntoResponse {
    let session = match get_session(&state, &jar) {
        Some(s) => s,
        None => return Redirect::to("/login").into_response(),
    };
    
    let mut context = Context::new();
    context.insert("title", "Medications - Walle");
    context.insert("is_admin", &(session.role == crate::domain::UserRole::Admin));
    
    if let Ok(Some(elder)) = ElderService::get_elder_for_caregiver(&state.db, session.caregiver_id).await {
        context.insert("elder_id", &elder.id.to_string());
    }
    
    match state.templates.render("elder/medications.html", &context) {
        Ok(html) => Html(html).into_response(),
        Err(e) => {
            tracing::error!("Template error: {}", e);
            Html("<h1>Error loading page</h1>").into_response()
        }
    }
}

/// Call logs page
pub async fn call_logs_page(
    State(state): State<AppState>,
    jar: CookieJar,
) -> impl IntoResponse {
    let session = match get_session(&state, &jar) {
        Some(s) => s,
        None => return Redirect::to("/login").into_response(),
    };
    
    let mut context = Context::new();
    context.insert("title", "Call Logs - Walle");
    context.insert("is_admin", &(session.role == crate::domain::UserRole::Admin));
    
    if let Ok(Some(elder)) = ElderService::get_elder_for_caregiver(&state.db, session.caregiver_id).await {
        context.insert("elder_id", &elder.id.to_string());
    }
    
    match state.templates.render("elder/call_logs.html", &context) {
        Ok(html) => Html(html).into_response(),
        Err(e) => {
            tracing::error!("Template error: {}", e);
            Html("<h1>Error loading page</h1>").into_response()
        }
    }
}

/// Ride logs page
pub async fn ride_logs_page(
    State(state): State<AppState>,
    jar: CookieJar,
) -> impl IntoResponse {
    let session = match get_session(&state, &jar) {
        Some(s) => s,
        None => return Redirect::to("/login").into_response(),
    };
    
    let mut context = Context::new();
    context.insert("title", "Ride Logs - Walle");
    context.insert("is_admin", &(session.role == crate::domain::UserRole::Admin));
    
    if let Ok(Some(elder)) = ElderService::get_elder_for_caregiver(&state.db, session.caregiver_id).await {
        context.insert("elder_id", &elder.id.to_string());
    }
    
    match state.templates.render("elder/ride_logs.html", &context) {
        Ok(html) => Html(html).into_response(),
        Err(e) => {
            tracing::error!("Template error: {}", e);
            Html("<h1>Error loading page</h1>").into_response()
        }
    }
}

/// Reminder logs page
pub async fn reminder_logs_page(
    State(state): State<AppState>,
    jar: CookieJar,
) -> impl IntoResponse {
    let session = match get_session(&state, &jar) {
        Some(s) => s,
        None => return Redirect::to("/login").into_response(),
    };
    
    let mut context = Context::new();
    context.insert("title", "Reminder Logs - Walle");
    context.insert("is_admin", &(session.role == crate::domain::UserRole::Admin));
    
    if let Ok(Some(elder)) = ElderService::get_elder_for_caregiver(&state.db, session.caregiver_id).await {
        context.insert("elder_id", &elder.id.to_string());
    }
    
    match state.templates.render("elder/reminder_logs.html", &context) {
        Ok(html) => Html(html).into_response(),
        Err(e) => {
            tracing::error!("Template error: {}", e);
            Html("<h1>Error loading page</h1>").into_response()
        }
    }
}

/// Billing page
pub async fn billing_page(
    State(state): State<AppState>,
    jar: CookieJar,
) -> impl IntoResponse {
    let session = match get_session(&state, &jar) {
        Some(s) => s,
        None => return Redirect::to("/login").into_response(),
    };
    
    let mut context = Context::new();
    context.insert("title", "Billing - Walle");
    context.insert("is_admin", &(session.role == crate::domain::UserRole::Admin));
    
    match state.templates.render("billing.html", &context) {
        Ok(html) => Html(html).into_response(),
        Err(e) => {
            tracing::error!("Template error: {}", e);
            Html("<h1>Error loading page</h1>").into_response()
        }
    }
}

/// Admin dashboard
pub async fn admin_dashboard(
    State(state): State<AppState>,
    jar: CookieJar,
) -> impl IntoResponse {
    let _session = match get_session(&state, &jar) {
        Some(s) if s.role == crate::domain::UserRole::Admin => s,
        _ => return Redirect::to("/login").into_response(),
    };
    
    let mut context = Context::new();
    context.insert("title", "Admin Dashboard - Walle");
    context.insert("is_admin", &true);
    
    match state.templates.render("admin/dashboard.html", &context) {
        Ok(html) => Html(html).into_response(),
        Err(e) => {
            tracing::error!("Template error: {}", e);
            Html("<h1>Error loading page</h1>").into_response()
        }
    }
}

/// Admin caregivers page
pub async fn admin_caregivers(
    State(state): State<AppState>,
    jar: CookieJar,
) -> impl IntoResponse {
    let _session = match get_session(&state, &jar) {
        Some(s) if s.role == crate::domain::UserRole::Admin => s,
        _ => return Redirect::to("/login").into_response(),
    };
    
    let mut context = Context::new();
    context.insert("title", "Caregivers - Admin - Walle");
    context.insert("is_admin", &true);
    
    match state.templates.render("admin/caregivers.html", &context) {
        Ok(html) => Html(html).into_response(),
        Err(e) => {
            tracing::error!("Template error: {}", e);
            Html("<h1>Error loading page</h1>").into_response()
        }
    }
}

/// Admin elders page
pub async fn admin_elders(
    State(state): State<AppState>,
    jar: CookieJar,
) -> impl IntoResponse {
    let _session = match get_session(&state, &jar) {
        Some(s) if s.role == crate::domain::UserRole::Admin => s,
        _ => return Redirect::to("/login").into_response(),
    };
    
    let mut context = Context::new();
    context.insert("title", "Elders - Admin - Walle");
    context.insert("is_admin", &true);
    
    match state.templates.render("admin/elders.html", &context) {
        Ok(html) => Html(html).into_response(),
        Err(e) => {
            tracing::error!("Template error: {}", e);
            Html("<h1>Error loading page</h1>").into_response()
        }
    }
}

/// Admin logs page
pub async fn admin_logs(
    State(state): State<AppState>,
    jar: CookieJar,
) -> impl IntoResponse {
    let _session = match get_session(&state, &jar) {
        Some(s) if s.role == crate::domain::UserRole::Admin => s,
        _ => return Redirect::to("/login").into_response(),
    };
    
    let mut context = Context::new();
    context.insert("title", "Logs - Admin - Walle");
    context.insert("is_admin", &true);
    
    match state.templates.render("admin/logs.html", &context) {
        Ok(html) => Html(html).into_response(),
        Err(e) => {
            tracing::error!("Template error: {}", e);
            Html("<h1>Error loading page</h1>").into_response()
        }
    }
}

fn get_session(state: &AppState, jar: &CookieJar) -> Option<crate::domain::Session> {
    jar.get(SESSION_COOKIE)
        .and_then(|c| decode_session(c.value(), &state.config.session_secret))
}

