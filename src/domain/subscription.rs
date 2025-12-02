//! Subscription domain model

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Status of a subscription
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubscriptionStatus {
    Active,
    PastDue,
    Canceled,
    Unpaid,
    Trialing,
}

impl std::fmt::Display for SubscriptionStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SubscriptionStatus::Active => write!(f, "active"),
            SubscriptionStatus::PastDue => write!(f, "past_due"),
            SubscriptionStatus::Canceled => write!(f, "canceled"),
            SubscriptionStatus::Unpaid => write!(f, "unpaid"),
            SubscriptionStatus::Trialing => write!(f, "trialing"),
        }
    }
}

impl std::str::FromStr for SubscriptionStatus {
    type Err = String;
    
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "active" => Ok(SubscriptionStatus::Active),
            "past_due" => Ok(SubscriptionStatus::PastDue),
            "canceled" => Ok(SubscriptionStatus::Canceled),
            "unpaid" => Ok(SubscriptionStatus::Unpaid),
            "trialing" => Ok(SubscriptionStatus::Trialing),
            _ => Err(format!("Invalid subscription status: {}", s)),
        }
    }
}

/// Subscription record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Subscription {
    pub id: Uuid,
    pub caregiver_id: Uuid,
    pub stripe_subscription_id: String,
    pub stripe_customer_id: String,
    pub status: SubscriptionStatus,
    pub plan_name: String,
    pub current_period_start: DateTime<Utc>,
    pub current_period_end: DateTime<Utc>,
    pub cancel_at_period_end: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Request to create subscription record
#[derive(Debug, Clone)]
pub struct CreateSubscriptionRequest {
    pub caregiver_id: Uuid,
    pub stripe_subscription_id: String,
    pub stripe_customer_id: String,
    pub plan_name: String,
    pub current_period_start: DateTime<Utc>,
    pub current_period_end: DateTime<Utc>,
}

/// Request to update subscription
#[derive(Debug, Clone)]
pub struct UpdateSubscriptionRequest {
    pub status: Option<SubscriptionStatus>,
    pub current_period_start: Option<DateTime<Utc>>,
    pub current_period_end: Option<DateTime<Utc>>,
    pub cancel_at_period_end: Option<bool>,
}

/// Billing info for display
#[derive(Debug, Clone, Serialize)]
#[allow(dead_code)]
pub struct BillingInfo {
    pub subscription: Option<Subscription>,
    pub is_active: bool,
    pub days_until_renewal: Option<i64>,
}

