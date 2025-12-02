//! Authentication handlers

use axum::{
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use axum_extra::extract::cookie::{Cookie, CookieJar};
use serde::Serialize;

use crate::domain::{Caregiver, CreateCaregiverRequest, LoginRequest};
use crate::middleware::{AuthUser, SESSION_COOKIE};
use crate::services::{encode_session, AuthService};
use crate::AppState;

/// Register a new caregiver account
#[tracing::instrument(skip(state, req), fields(email = %req.email))]
pub async fn register(
    State(state): State<AppState>,
    Json(req): Json<CreateCaregiverRequest>,
) -> Result<impl IntoResponse, ApiError> {
    tracing::info!("Registering new caregiver");
    let caregiver = AuthService::register(&state.db, &req).await?;
    tracing::info!(caregiver_id = %caregiver.id, "Caregiver registered successfully");
    
    Ok((StatusCode::CREATED, Json(CaregiverResponse::from(caregiver))))
}

/// Log in to an existing account
#[tracing::instrument(skip(state, jar, req), fields(email = %req.email))]
pub async fn login(
    State(state): State<AppState>,
    jar: CookieJar,
    Json(req): Json<LoginRequest>,
) -> Result<impl IntoResponse, ApiError> {
    tracing::info!("Login attempt");
    let session = AuthService::login(&state.db, &req).await?;
    tracing::info!(caregiver_id = %session.caregiver_id, role = %session.role, "Login successful");
    
    // Encode session and set cookie
    let session_value = encode_session(&session, &state.config.session_secret);
    let cookie = Cookie::build((SESSION_COOKIE, session_value))
        .path("/")
        .http_only(true)
        .secure(false) // Set to true in production with HTTPS
        .same_site(axum_extra::extract::cookie::SameSite::Lax)
        .build();
    
    Ok((jar.add(cookie), Json(LoginResponse { success: true })))
}

/// Log out
pub async fn logout(jar: CookieJar) -> impl IntoResponse {
    let cookie = Cookie::build((SESSION_COOKIE, ""))
        .path("/")
        .http_only(true)
        .build();
    
    (jar.remove(cookie), Json(LogoutResponse { success: true }))
}

/// Get current user profile
#[tracing::instrument(skip(state, auth), fields(caregiver_id = %auth.session.caregiver_id))]
pub async fn get_profile(
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<impl IntoResponse, ApiError> {
    tracing::debug!("Fetching profile");
    let caregiver = AuthService::get_caregiver(&state.db, auth.session.caregiver_id).await?;
    
    // Get elder if exists
    let elder = crate::services::ElderService::get_elder_for_caregiver(
        &state.db,
        auth.session.caregiver_id,
    ).await?;
    
    Ok(Json(ProfileResponse {
        caregiver: CaregiverResponse::from(caregiver),
        elder: elder.map(ElderResponse::from),
    }))
}

// Response types

#[derive(Serialize)]
pub struct CaregiverResponse {
    pub id: String,
    pub name: String,
    pub email: String,
    pub role: String,
    pub created_at: String,
}

impl From<Caregiver> for CaregiverResponse {
    fn from(c: Caregiver) -> Self {
        Self {
            id: c.id.to_string(),
            name: c.name,
            email: c.email,
            role: c.role.to_string(),
            created_at: c.created_at.to_rfc3339(),
        }
    }
}

#[derive(Serialize)]
pub struct ElderResponse {
    pub id: String,
    pub name: String,
    pub phone_number: String,
    pub timezone: String,
    pub status: String,
}

impl From<crate::domain::Elder> for ElderResponse {
    fn from(e: crate::domain::Elder) -> Self {
        Self {
            id: e.id.to_string(),
            name: e.name,
            phone_number: e.phone_number,
            timezone: e.timezone,
            status: e.status.to_string(),
        }
    }
}

#[derive(Serialize)]
pub struct LoginResponse {
    pub success: bool,
}

#[derive(Serialize)]
pub struct LogoutResponse {
    pub success: bool,
}

#[derive(Serialize)]
pub struct ProfileResponse {
    pub caregiver: CaregiverResponse,
    pub elder: Option<ElderResponse>,
}

// Error handling

#[derive(Debug)]
pub struct ApiError(crate::domain::DomainError);

impl From<crate::domain::DomainError> for ApiError {
    fn from(err: crate::domain::DomainError) -> Self {
        ApiError(err)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> axum::response::Response {
        let (status, message) = match &self.0 {
            crate::domain::DomainError::NotFound(msg) => (StatusCode::NOT_FOUND, msg.clone()),
            crate::domain::DomainError::Validation(msg) => (StatusCode::BAD_REQUEST, msg.clone()),
            crate::domain::DomainError::Unauthorized(msg) => (StatusCode::UNAUTHORIZED, msg.clone()),
            crate::domain::DomainError::Conflict(msg) => (StatusCode::CONFLICT, msg.clone()),
            crate::domain::DomainError::ExternalService(msg) => (StatusCode::BAD_GATEWAY, msg.clone()),
            crate::domain::DomainError::Database(msg) => (StatusCode::INTERNAL_SERVER_ERROR, msg.clone()),
        };
        
        (status, Json(ErrorResponse { error: message })).into_response()
    }
}

#[derive(Serialize)]
pub struct ErrorResponse {
    pub error: String,
}

