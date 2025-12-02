//! Billing handlers

use axum::{
    extract::State,
    response::IntoResponse,
    Json,
};
use serde::Serialize;

use crate::handlers::auth::ApiError;
use crate::middleware::AuthUser;
use crate::repositories::postgres::{CaregiverRepository, SubscriptionRepository};
use crate::AppState;

/// Create a Stripe checkout session for subscription
pub async fn create_checkout(
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<impl IntoResponse, ApiError> {
    let caregiver = CaregiverRepository::find_by_id(&state.db, auth.session.caregiver_id).await?;
    
    // Create or get Stripe customer
    let customer_id = match caregiver.stripe_customer_id {
        Some(id) => id,
        None => {
            let customer = state.stripe.create_customer(&caregiver.email, &caregiver.name).await
                .map_err(|e| crate::domain::DomainError::ExternalService(e.to_string()))?;
            
            // Save customer ID
            CaregiverRepository::update_stripe_customer_id(&state.db, caregiver.id, &customer.id).await?;
            customer.id
        }
    };
    
    let success_url = format!("{}/billing?success=true", state.config.base_url);
    let cancel_url = format!("{}/billing?canceled=true", state.config.base_url);
    
    let session = state.stripe.create_checkout_session(
        &customer_id,
        &state.config.stripe_price_id,
        &success_url,
        &cancel_url,
    ).await
        .map_err(|e| crate::domain::DomainError::ExternalService(e.to_string()))?;
    
    Ok(Json(CheckoutResponse {
        url: session.url.unwrap_or_default(),
    }))
}

/// Get current billing status
pub async fn get_billing_status(
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<impl IntoResponse, ApiError> {
    let subscription = SubscriptionRepository::find_by_caregiver(&state.db, auth.session.caregiver_id).await?;
    
    let is_active = subscription.as_ref()
        .map(|s| s.status == crate::domain::SubscriptionStatus::Active)
        .unwrap_or(false);
    
    let days_until_renewal = subscription.as_ref()
        .map(|s| {
            let now = chrono::Utc::now();
            (s.current_period_end - now).num_days()
        });
    
    Ok(Json(BillingStatusResponse {
        has_subscription: subscription.is_some(),
        is_active,
        plan_name: subscription.as_ref().map(|s| s.plan_name.clone()),
        current_period_end: subscription.as_ref().map(|s| s.current_period_end.to_rfc3339()),
        days_until_renewal,
        cancel_at_period_end: subscription.as_ref().map(|s| s.cancel_at_period_end).unwrap_or(false),
    }))
}

/// Create a Stripe billing portal session
pub async fn create_portal_session(
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<impl IntoResponse, ApiError> {
    let caregiver = CaregiverRepository::find_by_id(&state.db, auth.session.caregiver_id).await?;
    
    let customer_id = caregiver.stripe_customer_id
        .ok_or_else(|| crate::domain::DomainError::NotFound("No billing account found".to_string()))?;
    
    let return_url = format!("{}/billing", state.config.base_url);
    
    let session = state.stripe.create_portal_session(&customer_id, &return_url).await
        .map_err(|e| crate::domain::DomainError::ExternalService(e.to_string()))?;
    
    Ok(Json(PortalResponse {
        url: session.url,
    }))
}

// Response types

#[derive(Serialize)]
pub struct CheckoutResponse {
    pub url: String,
}

#[derive(Serialize)]
pub struct BillingStatusResponse {
    pub has_subscription: bool,
    pub is_active: bool,
    pub plan_name: Option<String>,
    pub current_period_end: Option<String>,
    pub days_until_renewal: Option<i64>,
    pub cancel_at_period_end: bool,
}

#[derive(Serialize)]
pub struct PortalResponse {
    pub url: String,
}

