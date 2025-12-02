//! Request ID middleware for tracing correlation
//!
//! Generates a unique request ID for each incoming request, stores it in
//! request extensions, and adds it to the response headers.

use axum::{
    body::Body,
    http::{Request, Response},
    middleware::Next,
};
use uuid::Uuid;

/// Header name for request ID
pub const REQUEST_ID_HEADER: &str = "x-request-id";

/// Request ID stored in request extensions
#[derive(Clone, Debug)]
pub struct RequestId(pub Uuid);

impl RequestId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
    
    pub fn as_str(&self) -> String {
        self.0.to_string()
    }
}

impl Default for RequestId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for RequestId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Middleware that adds a request ID to each request
///
/// - Checks for existing X-Request-Id header (forwarded from upstream proxy)
/// - Generates new UUID if not present
/// - Stores in request extensions for handler access
/// - Adds to response headers
pub async fn request_id_middleware(
    mut request: Request<Body>,
    next: Next,
) -> Response<Body> {
    // Check for existing request ID from header (e.g., from reverse proxy)
    let request_id = request
        .headers()
        .get(REQUEST_ID_HEADER)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| Uuid::parse_str(s).ok())
        .map(RequestId)
        .unwrap_or_else(RequestId::new);
    
    // Store in extensions for handlers to access
    request.extensions_mut().insert(request_id.clone());
    
    // Create tracing span with request ID
    let span = tracing::info_span!(
        "request",
        request_id = %request_id,
        method = %request.method(),
        path = %request.uri().path(),
    );
    
    // Execute request within span
    let _guard = span.enter();
    
    let mut response = next.run(request).await;
    
    // Add request ID to response headers
    response.headers_mut().insert(
        REQUEST_ID_HEADER,
        request_id.as_str().parse().unwrap(),
    );
    
    response
}

/// Extractor to get request ID in handlers
impl<S> axum::extract::FromRequestParts<S> for RequestId
where
    S: Send + Sync,
{
    type Rejection = std::convert::Infallible;
    
    fn from_request_parts<'life0, 'life1, 'async_trait>(
        parts: &'life0 mut axum::http::request::Parts,
        _state: &'life1 S,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Self, Self::Rejection>> + Send + 'async_trait>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        Self: 'async_trait,
    {
        Box::pin(async move {
            Ok(parts
                .extensions
                .get::<RequestId>()
                .cloned()
                .unwrap_or_else(RequestId::new))
        })
    }
}

