//! HTML page handlers

use axum::{
    extract::State,
    response::{Html, IntoResponse, Redirect},
};
use axum_extra::extract::CookieJar;
use tera::Context;
use uuid::Uuid;

use crate::domain::Elder;
use crate::handlers::elder::SELECTED_ELDER_COOKIE;
use crate::middleware::{OptionalAuthUser, SESSION_COOKIE};
use crate::repositories::postgres::ElderRepository;
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
    context.insert("title", "Domingo - Voice AI Assistant for Elderly");
    
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
    context.insert("title", "Login - Domingo");
    
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
    context.insert("title", "Register - Domingo");
    
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
    context.insert("title", "Dashboard - Domingo");
    context.insert("current_path", "/dashboard");
    context.insert("is_admin", &(session.role == crate::domain::UserRole::Admin));
    
    // Get all elders and selected elder
    if let Some((selected_elder, elders)) = get_selected_elder(&state, &jar, session.caregiver_id).await {
        context.insert("elders", &elders);
        context.insert("selected_elder", &selected_elder);
        
        // Get elder summary
        if let Ok(summary) = ElderService::get_elder_summary(&state.db, selected_elder.id, session.caregiver_id, false).await {
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

/// Elder profile page query params
#[derive(Debug, serde::Deserialize)]
pub struct ProfileQuery {
    new: Option<String>,
}

/// Elder profile page
pub async fn elder_profile(
    State(state): State<AppState>,
    jar: CookieJar,
    axum::extract::Query(query): axum::extract::Query<ProfileQuery>,
) -> impl IntoResponse {
    let session = match get_session(&state, &jar) {
        Some(s) => s,
        None => return Redirect::to("/login").into_response(),
    };
    
    let mut context = Context::new();
    context.insert("title", "Elder Profile - Domingo");
    context.insert("current_path", "/elder/profile");
    context.insert("is_admin", &(session.role == crate::domain::UserRole::Admin));
    
    // Check if we should show the new form
    let show_new_form = query.new.as_deref() == Some("true");
    context.insert("show_new_form", &show_new_form);
    
    // Get all elders and selected elder
    if let Some((selected_elder, elders)) = get_selected_elder(&state, &jar, session.caregiver_id).await {
        context.insert("elders", &elders);
        context.insert("selected_elder", &selected_elder);
        context.insert("elder", &selected_elder);
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
    context.insert("title", "Contactos - Domingo");
    context.insert("current_path", "/elder/contacts");
    context.insert("is_admin", &(session.role == crate::domain::UserRole::Admin));
    
    // Get all elders and selected elder
    if let Some((selected_elder, elders)) = get_selected_elder(&state, &jar, session.caregiver_id).await {
        context.insert("elders", &elders);
        context.insert("selected_elder", &selected_elder);
        context.insert("elder_id", &selected_elder.id.to_string());
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
    context.insert("title", "Ubicaciones - Domingo");
    context.insert("current_path", "/elder/locations");
    context.insert("is_admin", &(session.role == crate::domain::UserRole::Admin));
    context.insert("google_maps_api_key", &state.config.google_maps_api_key);
    
    // Get all elders and selected elder
    if let Some((selected_elder, elders)) = get_selected_elder(&state, &jar, session.caregiver_id).await {
        context.insert("elders", &elders);
        context.insert("selected_elder", &selected_elder);
        context.insert("elder_id", &selected_elder.id.to_string());
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
    context.insert("title", "Medicamentos - Domingo");
    context.insert("current_path", "/elder/medications");
    context.insert("is_admin", &(session.role == crate::domain::UserRole::Admin));
    
    // Get all elders and selected elder
    if let Some((selected_elder, elders)) = get_selected_elder(&state, &jar, session.caregiver_id).await {
        context.insert("elders", &elders);
        context.insert("selected_elder", &selected_elder);
        context.insert("elder_id", &selected_elder.id.to_string());
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
    context.insert("title", "Llamadas - Domingo");
    context.insert("current_path", "/elder/logs/calls");
    context.insert("is_admin", &(session.role == crate::domain::UserRole::Admin));
    
    // Get all elders and selected elder
    if let Some((selected_elder, elders)) = get_selected_elder(&state, &jar, session.caregiver_id).await {
        context.insert("elders", &elders);
        context.insert("selected_elder", &selected_elder);
        context.insert("elder_id", &selected_elder.id.to_string());
    }
    
    match state.templates.render("elder/logs_calls.html", &context) {
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
    context.insert("title", "Viajes - Domingo");
    context.insert("current_path", "/elder/logs/rides");
    context.insert("is_admin", &(session.role == crate::domain::UserRole::Admin));
    
    // Get all elders and selected elder
    if let Some((selected_elder, elders)) = get_selected_elder(&state, &jar, session.caregiver_id).await {
        context.insert("elders", &elders);
        context.insert("selected_elder", &selected_elder);
        context.insert("elder_id", &selected_elder.id.to_string());
    }
    
    match state.templates.render("elder/logs_rides.html", &context) {
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
    context.insert("title", "Recordatorios - Domingo");
    context.insert("current_path", "/elder/logs/reminders");
    context.insert("is_admin", &(session.role == crate::domain::UserRole::Admin));
    
    // Get all elders and selected elder
    if let Some((selected_elder, elders)) = get_selected_elder(&state, &jar, session.caregiver_id).await {
        context.insert("elders", &elders);
        context.insert("selected_elder", &selected_elder);
        context.insert("elder_id", &selected_elder.id.to_string());
    }
    
    match state.templates.render("elder/logs_reminders.html", &context) {
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
    context.insert("title", "Facturación - Domingo");
    context.insert("current_path", "/billing");
    context.insert("is_admin", &(session.role == crate::domain::UserRole::Admin));
    
    // Get all elders for billing overview
    if let Some((selected_elder, elders)) = get_selected_elder(&state, &jar, session.caregiver_id).await {
        context.insert("elders", &elders);
        context.insert("selected_elder", &selected_elder);
    }
    
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
    let session = match get_session(&state, &jar) {
        Some(s) if s.role == crate::domain::UserRole::Admin => s,
        _ => return Redirect::to("/login").into_response(),
    };
    
    let mut context = Context::new();
    context.insert("title", "Panel Admin - Domingo");
    context.insert("current_path", "/admin");
    context.insert("is_admin", &true);
    
    // Admin also gets elder switcher if they have elders
    if let Some((selected_elder, elders)) = get_selected_elder(&state, &jar, session.caregiver_id).await {
        context.insert("elders", &elders);
        context.insert("selected_elder", &selected_elder);
    }
    
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
    let session = match get_session(&state, &jar) {
        Some(s) if s.role == crate::domain::UserRole::Admin => s,
        _ => return Redirect::to("/login").into_response(),
    };
    
    let mut context = Context::new();
    context.insert("title", "Cuidadores - Admin - Domingo");
    context.insert("current_path", "/admin/caregivers");
    context.insert("is_admin", &true);
    
    // Admin also gets elder switcher if they have elders
    if let Some((selected_elder, elders)) = get_selected_elder(&state, &jar, session.caregiver_id).await {
        context.insert("elders", &elders);
        context.insert("selected_elder", &selected_elder);
    }
    
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
    let session = match get_session(&state, &jar) {
        Some(s) if s.role == crate::domain::UserRole::Admin => s,
        _ => return Redirect::to("/login").into_response(),
    };
    
    let mut context = Context::new();
    context.insert("title", "Adultos Mayores - Admin - Domingo");
    context.insert("current_path", "/admin/elders");
    context.insert("is_admin", &true);
    
    // Admin also gets elder switcher if they have elders
    if let Some((selected_elder, elders)) = get_selected_elder(&state, &jar, session.caregiver_id).await {
        context.insert("elders", &elders);
        context.insert("selected_elder", &selected_elder);
    }
    
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
    let session = match get_session(&state, &jar) {
        Some(s) if s.role == crate::domain::UserRole::Admin => s,
        _ => return Redirect::to("/login").into_response(),
    };
    
    let mut context = Context::new();
    context.insert("title", "Registros - Admin - Domingo");
    context.insert("current_path", "/admin/logs");
    context.insert("is_admin", &true);
    
    // Admin also gets elder switcher if they have elders
    if let Some((selected_elder, elders)) = get_selected_elder(&state, &jar, session.caregiver_id).await {
        context.insert("elders", &elders);
        context.insert("selected_elder", &selected_elder);
    }
    
    match state.templates.render("admin/logs.html", &context) {
        Ok(html) => Html(html).into_response(),
        Err(e) => {
            tracing::error!("Template error: {}", e);
            Html("<h1>Error loading page</h1>").into_response()
        }
    }
}

/// Admin live calls page (call center)
pub async fn admin_calls(
    State(state): State<AppState>,
    jar: CookieJar,
) -> impl IntoResponse {
    let session = match get_session(&state, &jar) {
        Some(s) if s.role == crate::domain::UserRole::Admin => s,
        _ => return Redirect::to("/login").into_response(),
    };
    
    let mut context = Context::new();
    context.insert("title", "Centro de Llamadas - Admin - Domingo");
    context.insert("current_path", "/admin/calls");
    context.insert("is_admin", &true);
    
    // Admin also gets elder switcher if they have elders
    if let Some((selected_elder, elders)) = get_selected_elder(&state, &jar, session.caregiver_id).await {
        context.insert("elders", &elders);
        context.insert("selected_elder", &selected_elder);
    }
    
    match state.templates.render("admin/calls.html", &context) {
        Ok(html) => Html(html).into_response(),
        Err(e) => {
            tracing::error!("Template error: {}", e);
            Html("<h1>Error loading page</h1>").into_response()
        }
    }
}

/// Admin debug tools page
pub async fn admin_debug(
    State(state): State<AppState>,
    jar: CookieJar,
) -> impl IntoResponse {
    let session = match get_session(&state, &jar) {
        Some(s) if s.role == crate::domain::UserRole::Admin => s,
        _ => return Redirect::to("/login").into_response(),
    };
    
    let mut context = Context::new();
    context.insert("title", "Herramientas Debug - Admin - Domingo");
    context.insert("current_path", "/admin/debug");
    context.insert("is_admin", &true);
    
    // Admin also gets elder switcher if they have elders
    if let Some((selected_elder, elders)) = get_selected_elder(&state, &jar, session.caregiver_id).await {
        context.insert("elders", &elders);
        context.insert("selected_elder", &selected_elder);
    }
    
    match state.templates.render("admin/debug.html", &context) {
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

/// Get selected elder from cookie, with fallback to first elder
/// Settings profile page
pub async fn settings_profile_page(
    State(state): State<AppState>,
    jar: CookieJar,
) -> impl IntoResponse {
    let session = match get_session(&state, &jar) {
        Some(s) => s,
        None => return Redirect::to("/login").into_response(),
    };
    
    let mut context = Context::new();
    context.insert("title", "Mi Perfil - Domingo");
    context.insert("current_path", "/settings/profile");
    context.insert("is_admin", &(session.role == crate::domain::UserRole::Admin));
    
    // Get all elders for sidebar
    if let Some((selected_elder, elders)) = get_selected_elder(&state, &jar, session.caregiver_id).await {
        context.insert("elders", &elders);
        context.insert("selected_elder", &selected_elder);
    }
    
    match state.templates.render("settings/profile.html", &context) {
        Ok(html) => Html(html).into_response(),
        Err(e) => {
            tracing::error!("Template error: {}", e);
            Html("<h1>Error loading page</h1>").into_response()
        }
    }
}

async fn get_selected_elder(
    state: &AppState,
    jar: &CookieJar,
    caregiver_id: Uuid,
) -> Option<(Elder, Vec<Elder>)> {
    // Get all elders for this caregiver
    let elders = ElderRepository::list_by_caregiver(&state.db, caregiver_id).await.ok()?;
    
    if elders.is_empty() {
        return None;
    }
    
    // Try to get selected elder from cookie
    let selected_id = jar.get(SELECTED_ELDER_COOKIE)
        .and_then(|c| Uuid::parse_str(c.value()).ok());
    
    // Find selected elder or default to first
    let selected_elder = if let Some(id) = selected_id {
        elders.iter().find(|e| e.id == id).cloned()
    } else {
        None
    }.unwrap_or_else(|| elders.first().unwrap().clone());
    
    Some((selected_elder, elders))
}
