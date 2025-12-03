//! Authentication middleware

use axum::{
    extract::FromRequestParts,
    http::{request::Parts, StatusCode},
    response::{IntoResponse, Response},
};
use axum_extra::extract::CookieJar;

use crate::domain::{Session, UserRole};
use crate::services::decode_session;
use crate::AppState;

/// Session cookie name
pub const SESSION_COOKIE: &str = "domingo_session";

/// Authenticated user - extracted from session cookie
#[derive(Debug, Clone)]
pub struct AuthUser {
    pub session: Session,
}

impl AuthUser {
    pub fn is_admin(&self) -> bool {
        self.session.role == UserRole::Admin
    }
}

/// Extractor that requires authentication
impl<S> FromRequestParts<S> for AuthUser
where
    S: Send + Sync,
    AppState: FromRef<S>,
{
    type Rejection = Response;
    
    fn from_request_parts<'life0, 'life1, 'async_trait>(
        parts: &'life0 mut Parts,
        state: &'life1 S,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Self, Self::Rejection>> + Send + 'async_trait>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        Self: 'async_trait,
    {
        Box::pin(async move {
            let app_state = AppState::from_ref(state);
            let jar = CookieJar::from_headers(&parts.headers);
            
            let session_cookie = jar.get(SESSION_COOKIE)
                .ok_or_else(|| {
                    (StatusCode::UNAUTHORIZED, "Not authenticated").into_response()
                })?;
            
            let session = decode_session(session_cookie.value(), &app_state.config.session_secret)
                .ok_or_else(|| {
                    (StatusCode::UNAUTHORIZED, "Invalid session").into_response()
                })?;
            
            Ok(AuthUser { session })
        })
    }
}

/// Optional authenticated user
#[derive(Debug, Clone)]
pub struct OptionalAuthUser {
    pub session: Option<Session>,
}

impl<S> FromRequestParts<S> for OptionalAuthUser
where
    S: Send + Sync,
    AppState: FromRef<S>,
{
    type Rejection = Response;
    
    fn from_request_parts<'life0, 'life1, 'async_trait>(
        parts: &'life0 mut Parts,
        state: &'life1 S,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Self, Self::Rejection>> + Send + 'async_trait>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        Self: 'async_trait,
    {
        Box::pin(async move {
            let app_state = AppState::from_ref(state);
            let jar = CookieJar::from_headers(&parts.headers);
            
            let session = jar.get(SESSION_COOKIE)
                .and_then(|c| decode_session(c.value(), &app_state.config.session_secret));
            
            Ok(OptionalAuthUser { session })
        })
    }
}

/// Admin-only authenticated user
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct AdminUser {
    pub session: Session,
}

impl<S> FromRequestParts<S> for AdminUser
where
    S: Send + Sync,
    AppState: FromRef<S>,
{
    type Rejection = Response;
    
    fn from_request_parts<'life0, 'life1, 'async_trait>(
        parts: &'life0 mut Parts,
        state: &'life1 S,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Self, Self::Rejection>> + Send + 'async_trait>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        Self: 'async_trait,
    {
        Box::pin(async move {
            let auth_user = AuthUser::from_request_parts(parts, state).await?;
            
            if !auth_user.is_admin() {
                return Err((StatusCode::FORBIDDEN, "Admin access required").into_response());
            }
            
            Ok(AdminUser { session: auth_user.session })
        })
    }
}

/// Helper trait to extract AppState from S
pub trait FromRef<S> {
    fn from_ref(state: &S) -> Self;
}

impl FromRef<AppState> for AppState {
    fn from_ref(state: &AppState) -> Self {
        state.clone()
    }
}

