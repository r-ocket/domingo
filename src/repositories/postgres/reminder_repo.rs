//! Reminder log repository implementation

use chrono::Utc;
use uuid::Uuid;

use crate::domain::{
    CreateReminderLogRequest, DeliveryMethod, DomainResult,
    Pagination, Paginated, ReminderLog, ReminderStatus, UpdateReminderLogRequest,
};
use super::PostgresPool;

/// Repository for reminder log operations
pub struct ReminderLogRepository;

impl ReminderLogRepository {
    /// Create a new reminder log entry
    pub async fn create(
        pool: &PostgresPool,
        req: &CreateReminderLogRequest,
    ) -> DomainResult<ReminderLog> {
        let client = pool.get().await?;
        let id = Uuid::new_v4();
        let now = Utc::now();
        
        let row = client
            .query_one(
                r#"
                INSERT INTO reminder_logs (id, schedule_id, elder_id, medication_name, timestamp, delivery_method, status, created_at, updated_at)
                VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
                RETURNING id, schedule_id, elder_id, medication_name, timestamp, delivery_method, status, twilio_call_sid, metadata, created_at, updated_at
                "#,
                &[&id, &req.schedule_id, &req.elder_id, &req.medication_name, &now, &req.delivery_method.to_string(), &"pending", &now, &now],
            )
            .await?;
        
        Ok(row_to_reminder_log(&row))
    }
    
    /// Update reminder log status
    pub async fn update(
        pool: &PostgresPool,
        id: Uuid,
        req: &UpdateReminderLogRequest,
    ) -> DomainResult<ReminderLog> {
        let client = pool.get().await?;
        let now = Utc::now();
        
        let row = client
            .query_one(
                r#"
                UPDATE reminder_logs 
                SET status = $2, twilio_call_sid = COALESCE($3, twilio_call_sid), metadata = COALESCE($4, metadata), updated_at = $5
                WHERE id = $1
                RETURNING id, schedule_id, elder_id, medication_name, timestamp, delivery_method, status, twilio_call_sid, metadata, created_at, updated_at
                "#,
                &[&id, &req.status.to_string(), &req.twilio_call_sid, &req.metadata, &now],
            )
            .await?;
        
        Ok(row_to_reminder_log(&row))
    }
    
    /// List reminder logs for an elder
    pub async fn list_by_elder(
        pool: &PostgresPool,
        elder_id: Uuid,
        pagination: &Pagination,
    ) -> DomainResult<Paginated<ReminderLog>> {
        let client = pool.get().await?;
        
        let total: i64 = client
            .query_one(
                "SELECT COUNT(*) FROM reminder_logs WHERE elder_id = $1",
                &[&elder_id],
            )
            .await?
            .get(0);
        
        let rows = client
            .query(
                r#"
                SELECT id, schedule_id, elder_id, medication_name, timestamp, delivery_method, status, twilio_call_sid, metadata, created_at, updated_at
                FROM reminder_logs
                WHERE elder_id = $1
                ORDER BY timestamp DESC
                LIMIT $2 OFFSET $3
                "#,
                &[&elder_id, &pagination.limit(), &pagination.offset()],
            )
            .await?;
        
        let logs: Vec<ReminderLog> = rows.iter().map(row_to_reminder_log).collect();
        
        Ok(Paginated::new(logs, total, pagination))
    }
    
    /// Check if a reminder was already sent recently for a schedule
    pub async fn was_recently_sent(
        pool: &PostgresPool,
        schedule_id: Uuid,
        minutes_threshold: i32,
    ) -> DomainResult<bool> {
        let client = pool.get().await?;
        
        let count: i64 = client
            .query_one(
                r#"
                SELECT COUNT(*) FROM reminder_logs 
                WHERE schedule_id = $1 
                AND timestamp > NOW() - INTERVAL '1 minute' * $2
                "#,
                &[&schedule_id, &minutes_threshold],
            )
            .await?
            .get(0);
        
        Ok(count > 0)
    }
}

fn row_to_reminder_log(row: &tokio_postgres::Row) -> ReminderLog {
    ReminderLog {
        id: row.get("id"),
        schedule_id: row.get("schedule_id"),
        elder_id: row.get("elder_id"),
        medication_name: row.get("medication_name"),
        timestamp: row.get("timestamp"),
        delivery_method: row.get::<_, String>("delivery_method")
            .parse()
            .unwrap_or(DeliveryMethod::Call),
        status: row.get::<_, String>("status")
            .parse()
            .unwrap_or(ReminderStatus::Pending),
        twilio_call_sid: row.get("twilio_call_sid"),
        metadata: row.get("metadata"),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
    }
}

