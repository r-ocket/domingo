//! Call session repository implementation

use chrono::Utc;
use uuid::Uuid;

use crate::domain::{
    CallSession, CallStatus, CreateCallSessionRequest, DomainError,
    DomainResult, Pagination, Paginated, UpdateCallSessionRequest,
};
use super::PostgresPool;

/// Repository for call session operations
pub struct CallSessionRepository;

impl CallSessionRepository {
    /// Create a new call session
    pub async fn create(
        pool: &PostgresPool,
        req: &CreateCallSessionRequest,
    ) -> DomainResult<CallSession> {
        let client = pool.get().await?;
        let id = Uuid::new_v4();
        let now = Utc::now();
        let empty_tools: Vec<String> = Vec::new();
        
        let row = client
            .query_one(
                r#"
                INSERT INTO call_sessions (id, elder_id, twilio_call_sid, from_number, status, started_at, tools_used, created_at, updated_at)
                VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
                RETURNING id, elder_id, twilio_call_sid, from_number, status, started_at, ended_at, duration_seconds,
                          summary_text, transcript, tools_used, transferred_to, metadata, created_at, updated_at
                "#,
                &[&id, &req.elder_id, &req.twilio_call_sid, &req.from_number, &"active", &now, &empty_tools, &now, &now],
            )
            .await?;
        
        Ok(row_to_call_session(&row))
    }
    
    /// Find call session by ID
    pub async fn find_by_id(pool: &PostgresPool, id: Uuid) -> DomainResult<CallSession> {
        let client = pool.get().await?;
        
        let row = client
            .query_opt(
                r#"
                SELECT id, elder_id, twilio_call_sid, from_number, status, started_at, ended_at, duration_seconds,
                       summary_text, transcript, tools_used, transferred_to, metadata, created_at, updated_at
                FROM call_sessions WHERE id = $1
                "#,
                &[&id],
            )
            .await?
            .ok_or_else(|| DomainError::NotFound("Call session not found".to_string()))?;
        
        Ok(row_to_call_session(&row))
    }
    
    /// Find call session by Twilio call SID
    pub async fn find_by_twilio_sid(pool: &PostgresPool, call_sid: &str) -> DomainResult<CallSession> {
        let client = pool.get().await?;
        
        let row = client
            .query_opt(
                r#"
                SELECT id, elder_id, twilio_call_sid, from_number, status, started_at, ended_at, duration_seconds,
                       summary_text, transcript, tools_used, transferred_to, metadata, created_at, updated_at
                FROM call_sessions WHERE twilio_call_sid = $1
                "#,
                &[&call_sid],
            )
            .await?
            .ok_or_else(|| DomainError::NotFound("Call session not found".to_string()))?;
        
        Ok(row_to_call_session(&row))
    }
    
    /// Update call session
    pub async fn update(
        pool: &PostgresPool,
        id: Uuid,
        req: &UpdateCallSessionRequest,
    ) -> DomainResult<CallSession> {
        let current = Self::find_by_id(pool, id).await?;
        let client = pool.get().await?;
        let now = Utc::now();
        
        let status = req.status.unwrap_or(current.status).to_string();
        let ended_at = req.ended_at.or(current.ended_at);
        let duration_seconds = req.duration_seconds.or(current.duration_seconds);
        let summary_text = req.summary_text.clone().or(current.summary_text);
        let transcript = req.transcript.clone().or(current.transcript);
        let tools_used = req.tools_used.clone().unwrap_or(current.tools_used);
        let transferred_to = req.transferred_to.clone().or(current.transferred_to);
        let metadata = req.metadata.clone().or(current.metadata);
        
        let row = client
            .query_one(
                r#"
                UPDATE call_sessions 
                SET status = $2, ended_at = $3, duration_seconds = $4, summary_text = $5,
                    transcript = $6, tools_used = $7, transferred_to = $8, metadata = $9, updated_at = $10
                WHERE id = $1
                RETURNING id, elder_id, twilio_call_sid, from_number, status, started_at, ended_at, duration_seconds,
                          summary_text, transcript, tools_used, transferred_to, metadata, created_at, updated_at
                "#,
                &[&id, &status, &ended_at, &duration_seconds, &summary_text, &transcript, &tools_used, &transferred_to, &metadata, &now],
            )
            .await?;
        
        Ok(row_to_call_session(&row))
    }
    
    /// Add a tool to the tools_used list
    pub async fn add_tool_used(
        pool: &PostgresPool,
        id: Uuid,
        tool_name: &str,
    ) -> DomainResult<()> {
        let client = pool.get().await?;
        let now = Utc::now();
        
        client
            .execute(
                r#"
                UPDATE call_sessions 
                SET tools_used = array_append(tools_used, $2), updated_at = $3
                WHERE id = $1
                "#,
                &[&id, &tool_name, &now],
            )
            .await?;
        
        Ok(())
    }
    
    /// List call sessions for an elder
    pub async fn list_by_elder(
        pool: &PostgresPool,
        elder_id: Uuid,
        pagination: &Pagination,
    ) -> DomainResult<Paginated<CallSession>> {
        let client = pool.get().await?;
        
        let total: i64 = client
            .query_one(
                "SELECT COUNT(*) FROM call_sessions WHERE elder_id = $1",
                &[&elder_id],
            )
            .await?
            .get(0);
        
        let rows = client
            .query(
                r#"
                SELECT id, elder_id, twilio_call_sid, from_number, status, started_at, ended_at, duration_seconds,
                       summary_text, transcript, tools_used, transferred_to, metadata, created_at, updated_at
                FROM call_sessions
                WHERE elder_id = $1
                ORDER BY started_at DESC
                LIMIT $2 OFFSET $3
                "#,
                &[&elder_id, &pagination.limit(), &pagination.offset()],
            )
            .await?;
        
        let sessions: Vec<CallSession> = rows.iter().map(row_to_call_session).collect();
        
        Ok(Paginated::new(sessions, total, pagination))
    }
    
    /// List all call sessions (admin)
    pub async fn list_all(
        pool: &PostgresPool,
        pagination: &Pagination,
    ) -> DomainResult<Paginated<CallSession>> {
        let client = pool.get().await?;
        
        let total: i64 = client
            .query_one("SELECT COUNT(*) FROM call_sessions", &[])
            .await?
            .get(0);
        
        let rows = client
            .query(
                r#"
                SELECT id, elder_id, twilio_call_sid, from_number, status, started_at, ended_at, duration_seconds,
                       summary_text, transcript, tools_used, transferred_to, metadata, created_at, updated_at
                FROM call_sessions
                ORDER BY started_at DESC
                LIMIT $1 OFFSET $2
                "#,
                &[&pagination.limit(), &pagination.offset()],
            )
            .await?;
        
        let sessions: Vec<CallSession> = rows.iter().map(row_to_call_session).collect();
        
        Ok(Paginated::new(sessions, total, pagination))
    }
    
    /// Get the last call for an elder
    pub async fn get_last_for_elder(pool: &PostgresPool, elder_id: Uuid) -> DomainResult<Option<CallSession>> {
        let client = pool.get().await?;
        
        let row = client
            .query_opt(
                r#"
                SELECT id, elder_id, twilio_call_sid, from_number, status, started_at, ended_at, duration_seconds,
                       summary_text, transcript, tools_used, transferred_to, metadata, created_at, updated_at
                FROM call_sessions 
                WHERE elder_id = $1
                ORDER BY started_at DESC
                LIMIT 1
                "#,
                &[&elder_id],
            )
            .await?;
        
        Ok(row.map(|r| row_to_call_session(&r)))
    }
    
    /// Count calls today (admin stats)
    pub async fn count_today(pool: &PostgresPool) -> DomainResult<i64> {
        let client = pool.get().await?;
        
        let count: i64 = client
            .query_one(
                "SELECT COUNT(*) FROM call_sessions WHERE started_at::date = CURRENT_DATE",
                &[],
            )
            .await?
            .get(0);
        
        Ok(count)
    }
}

fn row_to_call_session(row: &tokio_postgres::Row) -> CallSession {
    CallSession {
        id: row.get("id"),
        elder_id: row.get("elder_id"),
        twilio_call_sid: row.get("twilio_call_sid"),
        from_number: row.get("from_number"),
        status: row.get::<_, String>("status")
            .parse()
            .unwrap_or(CallStatus::Active),
        started_at: row.get("started_at"),
        ended_at: row.get("ended_at"),
        duration_seconds: row.get("duration_seconds"),
        summary_text: row.get("summary_text"),
        transcript: row.get("transcript"),
        tools_used: row.get("tools_used"),
        transferred_to: row.get("transferred_to"),
        metadata: row.get("metadata"),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
    }
}

