//! Stripe API client for billing and subscriptions

use chrono::{DateTime, TimeZone, Utc};
use hmac::{Hmac, Mac};
use serde::Deserialize;
use sha2::Sha256;

/// Stripe API client
pub struct StripeClient {
    secret_key: String,
    webhook_secret: String,
    http_client: reqwest::Client,
}

impl StripeClient {
    /// Create a new Stripe client
    pub fn new(secret_key: &str, webhook_secret: &str) -> Self {
        Self {
            secret_key: secret_key.to_string(),
            webhook_secret: webhook_secret.to_string(),
            http_client: reqwest::Client::new(),
        }
    }
    
    /// Create a Stripe customer
    pub async fn create_customer(
        &self,
        email: &str,
        name: &str,
    ) -> Result<Customer, StripeError> {
        let params = [
            ("email", email),
            ("name", name),
        ];
        
        let response = self.http_client
            .post("https://api.stripe.com/v1/customers")
            .basic_auth(&self.secret_key, None::<&str>)
            .form(&params)
            .send()
            .await
            .map_err(|e| StripeError::Request(e.to_string()))?;
        
        if !response.status().is_success() {
            let error_text = response.text().await.unwrap_or_default();
            return Err(StripeError::Api(error_text));
        }
        
        response.json().await.map_err(|e| StripeError::Parse(e.to_string()))
    }
    
    /// Create a checkout session for subscription
    pub async fn create_checkout_session(
        &self,
        customer_id: &str,
        price_id: &str,
        success_url: &str,
        cancel_url: &str,
    ) -> Result<CheckoutSession, StripeError> {
        let params = [
            ("customer", customer_id),
            ("mode", "subscription"),
            ("success_url", success_url),
            ("cancel_url", cancel_url),
            ("line_items[0][price]", price_id),
            ("line_items[0][quantity]", "1"),
        ];
        
        let response = self.http_client
            .post("https://api.stripe.com/v1/checkout/sessions")
            .basic_auth(&self.secret_key, None::<&str>)
            .form(&params)
            .send()
            .await
            .map_err(|e| StripeError::Request(e.to_string()))?;
        
        if !response.status().is_success() {
            let error_text = response.text().await.unwrap_or_default();
            return Err(StripeError::Api(error_text));
        }
        
        response.json().await.map_err(|e| StripeError::Parse(e.to_string()))
    }
    
    /// Create a billing portal session
    pub async fn create_portal_session(
        &self,
        customer_id: &str,
        return_url: &str,
    ) -> Result<PortalSession, StripeError> {
        let params = [
            ("customer", customer_id),
            ("return_url", return_url),
        ];
        
        let response = self.http_client
            .post("https://api.stripe.com/v1/billing_portal/sessions")
            .basic_auth(&self.secret_key, None::<&str>)
            .form(&params)
            .send()
            .await
            .map_err(|e| StripeError::Request(e.to_string()))?;
        
        if !response.status().is_success() {
            let error_text = response.text().await.unwrap_or_default();
            return Err(StripeError::Api(error_text));
        }
        
        response.json().await.map_err(|e| StripeError::Parse(e.to_string()))
    }
    
    /// Get subscription details
    pub async fn get_subscription(&self, subscription_id: &str) -> Result<Subscription, StripeError> {
        let url = format!("https://api.stripe.com/v1/subscriptions/{}", subscription_id);
        
        let response = self.http_client
            .get(&url)
            .basic_auth(&self.secret_key, None::<&str>)
            .send()
            .await
            .map_err(|e| StripeError::Request(e.to_string()))?;
        
        if !response.status().is_success() {
            let error_text = response.text().await.unwrap_or_default();
            return Err(StripeError::Api(error_text));
        }
        
        response.json().await.map_err(|e| StripeError::Parse(e.to_string()))
    }
    
    /// Cancel subscription
    pub async fn cancel_subscription(&self, subscription_id: &str) -> Result<Subscription, StripeError> {
        let url = format!("https://api.stripe.com/v1/subscriptions/{}", subscription_id);
        
        let response = self.http_client
            .delete(&url)
            .basic_auth(&self.secret_key, None::<&str>)
            .send()
            .await
            .map_err(|e| StripeError::Request(e.to_string()))?;
        
        if !response.status().is_success() {
            let error_text = response.text().await.unwrap_or_default();
            return Err(StripeError::Api(error_text));
        }
        
        response.json().await.map_err(|e| StripeError::Parse(e.to_string()))
    }
    
    /// Verify webhook signature and parse event
    pub fn verify_webhook(
        &self,
        payload: &str,
        signature: &str,
    ) -> Result<WebhookEvent, StripeError> {
        // Parse the signature header
        let parts: std::collections::HashMap<_, _> = signature
            .split(',')
            .filter_map(|part| {
                let mut split = part.splitn(2, '=');
                Some((split.next()?, split.next()?))
            })
            .collect();
        
        let timestamp = parts.get("t")
            .ok_or_else(|| StripeError::Webhook("Missing timestamp".to_string()))?;
        let signature = parts.get("v1")
            .ok_or_else(|| StripeError::Webhook("Missing signature".to_string()))?;
        
        // Create the signed payload
        let signed_payload = format!("{}.{}", timestamp, payload);
        
        // Compute expected signature
        let mut mac = Hmac::<Sha256>::new_from_slice(self.webhook_secret.as_bytes())
            .map_err(|_| StripeError::Webhook("Invalid webhook secret".to_string()))?;
        mac.update(signed_payload.as_bytes());
        let expected = hex::encode(mac.finalize().into_bytes());
        
        // Compare signatures
        if expected != *signature {
            return Err(StripeError::Webhook("Invalid signature".to_string()));
        }
        
        // Check timestamp is within tolerance (5 minutes)
        let timestamp_i64: i64 = timestamp.parse()
            .map_err(|_| StripeError::Webhook("Invalid timestamp".to_string()))?;
        let event_time = Utc.timestamp_opt(timestamp_i64, 0)
            .single()
            .ok_or_else(|| StripeError::Webhook("Invalid timestamp".to_string()))?;
        let now = Utc::now();
        let tolerance = chrono::Duration::minutes(5);
        
        if (now - event_time).abs() > tolerance {
            return Err(StripeError::Webhook("Timestamp too old".to_string()));
        }
        
        // Parse the event
        serde_json::from_str(payload)
            .map_err(|e| StripeError::Parse(e.to_string()))
    }
}

/// Stripe API errors
#[derive(Debug, thiserror::Error)]
pub enum StripeError {
    #[error("Request error: {0}")]
    Request(String),
    
    #[error("API error: {0}")]
    Api(String),
    
    #[error("Parse error: {0}")]
    Parse(String),
    
    #[error("Webhook error: {0}")]
    Webhook(String),
}

/// Stripe customer
#[derive(Debug, Deserialize)]
pub struct Customer {
    pub id: String,
    pub email: Option<String>,
    pub name: Option<String>,
}

/// Stripe checkout session
#[derive(Debug, Deserialize)]
pub struct CheckoutSession {
    pub id: String,
    pub url: Option<String>,
    pub customer: Option<String>,
    pub subscription: Option<String>,
}

/// Stripe billing portal session
#[derive(Debug, Deserialize)]
pub struct PortalSession {
    pub id: String,
    pub url: String,
}

/// Stripe subscription
#[derive(Debug, Deserialize)]
pub struct Subscription {
    pub id: String,
    pub customer: String,
    pub status: String,
    pub current_period_start: i64,
    pub current_period_end: i64,
    pub cancel_at_period_end: bool,
    pub items: SubscriptionItems,
}

#[derive(Debug, Deserialize)]
pub struct SubscriptionItems {
    pub data: Vec<SubscriptionItem>,
}

#[derive(Debug, Deserialize)]
pub struct SubscriptionItem {
    pub id: String,
    pub price: Price,
}

#[derive(Debug, Deserialize)]
pub struct Price {
    pub id: String,
    pub product: String,
    pub nickname: Option<String>,
}

impl Subscription {
    /// Convert to domain subscription status
    pub fn to_domain_status(&self) -> crate::domain::SubscriptionStatus {
        match self.status.as_str() {
            "active" => crate::domain::SubscriptionStatus::Active,
            "past_due" => crate::domain::SubscriptionStatus::PastDue,
            "canceled" => crate::domain::SubscriptionStatus::Canceled,
            "unpaid" => crate::domain::SubscriptionStatus::Unpaid,
            "trialing" => crate::domain::SubscriptionStatus::Trialing,
            _ => crate::domain::SubscriptionStatus::Canceled,
        }
    }
    
    /// Get period start as DateTime
    pub fn period_start(&self) -> DateTime<Utc> {
        Utc.timestamp_opt(self.current_period_start, 0)
            .single()
            .unwrap_or_else(Utc::now)
    }
    
    /// Get period end as DateTime
    pub fn period_end(&self) -> DateTime<Utc> {
        Utc.timestamp_opt(self.current_period_end, 0)
            .single()
            .unwrap_or_else(Utc::now)
    }
}

/// Stripe webhook event
#[derive(Debug, Deserialize)]
pub struct WebhookEvent {
    pub id: String,
    #[serde(rename = "type")]
    pub event_type: String,
    pub data: WebhookEventData,
}

#[derive(Debug, Deserialize)]
pub struct WebhookEventData {
    pub object: serde_json::Value,
}

impl WebhookEvent {
    /// Get subscription from event data
    pub fn subscription(&self) -> Option<Subscription> {
        serde_json::from_value(self.data.object.clone()).ok()
    }
    
    /// Get checkout session from event data
    pub fn checkout_session(&self) -> Option<CheckoutSession> {
        serde_json::from_value(self.data.object.clone()).ok()
    }
}

