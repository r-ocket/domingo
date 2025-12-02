//! Subscription repository implementation

use chrono::Utc;
use uuid::Uuid;

use crate::domain::{
    CreateSubscriptionRequest, DomainError, DomainResult, Subscription,
    SubscriptionStatus, UpdateSubscriptionRequest,
};
use super::PostgresPool;

/// Repository for subscription operations
pub struct SubscriptionRepository;

impl SubscriptionRepository {
    /// Create a new subscription
    pub async fn create(
        pool: &PostgresPool,
        req: &CreateSubscriptionRequest,
    ) -> DomainResult<Subscription> {
        let client = pool.get().await?;
        let id = Uuid::new_v4();
        let now = Utc::now();
        
        let row = client
            .query_one(
                r#"
                INSERT INTO subscriptions (id, caregiver_id, stripe_subscription_id, stripe_customer_id, status, plan_name, 
                                          current_period_start, current_period_end, cancel_at_period_end, created_at, updated_at)
                VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
                RETURNING id, caregiver_id, stripe_subscription_id, stripe_customer_id, status, plan_name,
                          current_period_start, current_period_end, cancel_at_period_end, created_at, updated_at
                "#,
                &[&id, &req.caregiver_id, &req.stripe_subscription_id, &req.stripe_customer_id, &"active",
                  &req.plan_name, &req.current_period_start, &req.current_period_end, &false, &now, &now],
            )
            .await?;
        
        Ok(row_to_subscription(&row))
    }
    
    /// Find subscription by ID
    pub async fn find_by_id(pool: &PostgresPool, id: Uuid) -> DomainResult<Subscription> {
        let client = pool.get().await?;
        
        let row = client
            .query_opt(
                r#"
                SELECT id, caregiver_id, stripe_subscription_id, stripe_customer_id, status, plan_name,
                       current_period_start, current_period_end, cancel_at_period_end, created_at, updated_at
                FROM subscriptions WHERE id = $1
                "#,
                &[&id],
            )
            .await?
            .ok_or_else(|| DomainError::NotFound("Subscription not found".to_string()))?;
        
        Ok(row_to_subscription(&row))
    }
    
    /// Find subscription by Stripe subscription ID
    pub async fn find_by_stripe_id(pool: &PostgresPool, stripe_subscription_id: &str) -> DomainResult<Subscription> {
        let client = pool.get().await?;
        
        let row = client
            .query_opt(
                r#"
                SELECT id, caregiver_id, stripe_subscription_id, stripe_customer_id, status, plan_name,
                       current_period_start, current_period_end, cancel_at_period_end, created_at, updated_at
                FROM subscriptions WHERE stripe_subscription_id = $1
                "#,
                &[&stripe_subscription_id],
            )
            .await?
            .ok_or_else(|| DomainError::NotFound("Subscription not found".to_string()))?;
        
        Ok(row_to_subscription(&row))
    }
    
    /// Find subscription by caregiver ID
    pub async fn find_by_caregiver(pool: &PostgresPool, caregiver_id: Uuid) -> DomainResult<Option<Subscription>> {
        let client = pool.get().await?;
        
        let row = client
            .query_opt(
                r#"
                SELECT id, caregiver_id, stripe_subscription_id, stripe_customer_id, status, plan_name,
                       current_period_start, current_period_end, cancel_at_period_end, created_at, updated_at
                FROM subscriptions WHERE caregiver_id = $1
                ORDER BY created_at DESC
                LIMIT 1
                "#,
                &[&caregiver_id],
            )
            .await?;
        
        Ok(row.map(|r| row_to_subscription(&r)))
    }
    
    /// Update subscription
    pub async fn update(
        pool: &PostgresPool,
        id: Uuid,
        req: &UpdateSubscriptionRequest,
    ) -> DomainResult<Subscription> {
        let current = Self::find_by_id(pool, id).await?;
        let client = pool.get().await?;
        let now = Utc::now();
        
        let status = req.status.unwrap_or(current.status).to_string();
        let current_period_start = req.current_period_start.unwrap_or(current.current_period_start);
        let current_period_end = req.current_period_end.unwrap_or(current.current_period_end);
        let cancel_at_period_end = req.cancel_at_period_end.unwrap_or(current.cancel_at_period_end);
        
        let row = client
            .query_one(
                r#"
                UPDATE subscriptions 
                SET status = $2, current_period_start = $3, current_period_end = $4, 
                    cancel_at_period_end = $5, updated_at = $6
                WHERE id = $1
                RETURNING id, caregiver_id, stripe_subscription_id, stripe_customer_id, status, plan_name,
                          current_period_start, current_period_end, cancel_at_period_end, created_at, updated_at
                "#,
                &[&id, &status, &current_period_start, &current_period_end, &cancel_at_period_end, &now],
            )
            .await?;
        
        Ok(row_to_subscription(&row))
    }
    
    /// Update subscription by Stripe subscription ID
    pub async fn update_by_stripe_id(
        pool: &PostgresPool,
        stripe_subscription_id: &str,
        req: &UpdateSubscriptionRequest,
    ) -> DomainResult<Subscription> {
        let current = Self::find_by_stripe_id(pool, stripe_subscription_id).await?;
        Self::update(pool, current.id, req).await
    }
    
    /// Count active subscriptions (admin stats)
    pub async fn count_active(pool: &PostgresPool) -> DomainResult<i64> {
        let client = pool.get().await?;
        
        let count: i64 = client
            .query_one(
                "SELECT COUNT(*) FROM subscriptions WHERE status = 'active'",
                &[],
            )
            .await?
            .get(0);
        
        Ok(count)
    }
}

fn row_to_subscription(row: &tokio_postgres::Row) -> Subscription {
    Subscription {
        id: row.get("id"),
        caregiver_id: row.get("caregiver_id"),
        stripe_subscription_id: row.get("stripe_subscription_id"),
        stripe_customer_id: row.get("stripe_customer_id"),
        status: row.get::<_, String>("status")
            .parse()
            .unwrap_or(SubscriptionStatus::Active),
        plan_name: row.get("plan_name"),
        current_period_start: row.get("current_period_start"),
        current_period_end: row.get("current_period_end"),
        cancel_at_period_end: row.get("cancel_at_period_end"),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
    }
}

