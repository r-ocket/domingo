//! Stripe webhook handlers

use axum::{
    body::Bytes,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
};

use crate::domain::{CreateSubscriptionRequest, UpdateSubscriptionRequest};
use crate::repositories::postgres::{CaregiverRepository, SubscriptionRepository};
use crate::AppState;

/// Handle Stripe webhooks
#[tracing::instrument(skip(state, headers, body))]
pub async fn handle_webhook(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> impl IntoResponse {
    tracing::debug!("Processing Stripe webhook");
    
    // Get signature from headers
    let signature = match headers.get("stripe-signature") {
        Some(sig) => sig.to_str().unwrap_or_default(),
        None => {
            tracing::error!("Missing Stripe signature");
            return StatusCode::BAD_REQUEST;
        }
    };
    
    let payload = match std::str::from_utf8(&body) {
        Ok(p) => p,
        Err(_) => {
            tracing::error!("Invalid payload encoding");
            return StatusCode::BAD_REQUEST;
        }
    };
    
    // Verify and parse webhook
    let event = match state.stripe.verify_webhook(payload, signature) {
        Ok(e) => e,
        Err(e) => {
            tracing::error!("Webhook verification failed: {}", e);
            return StatusCode::BAD_REQUEST;
        }
    };
    
    tracing::info!("Stripe webhook: {}", event.event_type);
    
    match event.event_type.as_str() {
        "checkout.session.completed" => {
            if let Some(session) = event.checkout_session() {
                if let Err(e) = handle_checkout_completed(&state, session).await {
                    tracing::error!("Failed to handle checkout: {}", e);
                }
            }
        }
        "customer.subscription.updated" => {
            if let Some(subscription) = event.subscription() {
                if let Err(e) = handle_subscription_updated(&state, subscription).await {
                    tracing::error!("Failed to handle subscription update: {}", e);
                }
            }
        }
        "customer.subscription.deleted" => {
            if let Some(subscription) = event.subscription() {
                if let Err(e) = handle_subscription_deleted(&state, subscription).await {
                    tracing::error!("Failed to handle subscription deletion: {}", e);
                }
            }
        }
        "invoice.payment_failed" => {
            if let Some(subscription) = event.subscription() {
                if let Err(e) = handle_payment_failed(&state, subscription).await {
                    tracing::error!("Failed to handle payment failure: {}", e);
                }
            }
        }
        _ => {
            tracing::debug!("Unhandled webhook event: {}", event.event_type);
        }
    }
    
    StatusCode::OK
}

async fn handle_checkout_completed(
    state: &AppState,
    session: crate::clients::CheckoutSession,
) -> Result<(), Box<dyn std::error::Error>> {
    let customer_id = session.customer.ok_or("No customer ID")?;
    let subscription_id = session.subscription.ok_or("No subscription ID")?;
    
    // Find caregiver by Stripe customer ID
    // This is a bit tricky - we need to search by stripe_customer_id
    // For now, let's fetch the subscription details from Stripe
    let subscription = state.stripe.get_subscription(&subscription_id).await?;
    
    // Find caregiver
    let caregivers = CaregiverRepository::list(&state.db, &crate::domain::Pagination { page: 1, per_page: 1000 }).await?;
    let caregiver = caregivers.items.into_iter()
        .find(|c| c.stripe_customer_id.as_ref() == Some(&customer_id))
        .ok_or("Caregiver not found for customer")?;
    
    // Create subscription record
    let plan_name = subscription.items.data.first()
        .and_then(|i| i.price.nickname.clone())
        .unwrap_or_else(|| "Monthly Plan".to_string());
    
    let current_period_start = subscription.period_start();
    let current_period_end = subscription.period_end();
    
    let req = CreateSubscriptionRequest {
        caregiver_id: caregiver.id,
        stripe_subscription_id: subscription.id.clone(),
        stripe_customer_id: customer_id,
        plan_name,
        current_period_start,
        current_period_end,
    };
    
    SubscriptionRepository::create(&state.db, &req).await?;
    
    tracing::info!("Created subscription for caregiver {}", caregiver.id);
    
    Ok(())
}

async fn handle_subscription_updated(
    state: &AppState,
    subscription: crate::clients::Subscription,
) -> Result<(), Box<dyn std::error::Error>> {
    let req = UpdateSubscriptionRequest {
        status: Some(subscription.to_domain_status()),
        current_period_start: Some(subscription.period_start()),
        current_period_end: Some(subscription.period_end()),
        cancel_at_period_end: Some(subscription.cancel_at_period_end),
    };
    
    SubscriptionRepository::update_by_stripe_id(&state.db, &subscription.id, &req).await?;
    
    tracing::info!("Updated subscription {}", subscription.id);
    
    Ok(())
}

async fn handle_subscription_deleted(
    state: &AppState,
    subscription: crate::clients::Subscription,
) -> Result<(), Box<dyn std::error::Error>> {
    let req = UpdateSubscriptionRequest {
        status: Some(crate::domain::SubscriptionStatus::Canceled),
        current_period_start: None,
        current_period_end: None,
        cancel_at_period_end: None,
    };
    
    SubscriptionRepository::update_by_stripe_id(&state.db, &subscription.id, &req).await?;
    
    tracing::info!("Canceled subscription {}", subscription.id);
    
    Ok(())
}

async fn handle_payment_failed(
    state: &AppState,
    subscription: crate::clients::Subscription,
) -> Result<(), Box<dyn std::error::Error>> {
    let req = UpdateSubscriptionRequest {
        status: Some(crate::domain::SubscriptionStatus::PastDue),
        current_period_start: None,
        current_period_end: None,
        cancel_at_period_end: None,
    };
    
    SubscriptionRepository::update_by_stripe_id(&state.db, &subscription.id, &req).await?;
    
    tracing::warn!("Payment failed for subscription {}", subscription.id);
    
    // TODO: Send notification to caregiver
    
    Ok(())
}

