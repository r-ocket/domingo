//! Call session management service

use chrono::Utc;
use uuid::Uuid;

use crate::domain::{
    CallSession, CallStatus, CreateCallSessionRequest, DomainResult,
    UpdateCallSessionRequest,
};
use crate::repositories::postgres::{CallSessionRepository, PostgresPool};

/// Call session management service
pub struct CallService;

impl CallService {
    /// Start a new call session
    pub async fn start_session(
        pool: &PostgresPool,
        elder_id: Uuid,
        twilio_call_sid: &str,
        from_number: &str,
    ) -> DomainResult<CallSession> {
        let req = CreateCallSessionRequest {
            elder_id,
            twilio_call_sid: twilio_call_sid.to_string(),
            from_number: from_number.to_string(),
        };
        
        CallSessionRepository::create(pool, &req).await
    }
    
    /// End a call session
    pub async fn end_session(
        pool: &PostgresPool,
        session_id: Uuid,
        status: CallStatus,
        summary: Option<String>,
        transcript: Option<String>,
    ) -> DomainResult<CallSession> {
        let session = CallSessionRepository::find_by_id(pool, session_id).await?;
        
        let duration = (Utc::now() - session.started_at).num_seconds() as i32;
        
        let update_req = UpdateCallSessionRequest {
            status: Some(status),
            ended_at: Some(Utc::now()),
            duration_seconds: Some(duration),
            summary_text: summary,
            transcript,
            tools_used: None,
            transferred_to: None,
            metadata: None,
        };
        
        CallSessionRepository::update(pool, session_id, &update_req).await
    }
    
    /// Mark call as transferred
    pub async fn mark_transferred(
        pool: &PostgresPool,
        session_id: Uuid,
        transferred_to: &str,
    ) -> DomainResult<CallSession> {
        let session = CallSessionRepository::find_by_id(pool, session_id).await?;
        
        let duration = (Utc::now() - session.started_at).num_seconds() as i32;
        
        let update_req = UpdateCallSessionRequest {
            status: Some(CallStatus::Transferred),
            ended_at: Some(Utc::now()),
            duration_seconds: Some(duration),
            summary_text: None,
            transcript: None,
            tools_used: None,
            transferred_to: Some(transferred_to.to_string()),
            metadata: None,
        };
        
        CallSessionRepository::update(pool, session_id, &update_req).await
    }
    
    /// Record a tool usage
    pub async fn record_tool_usage(
        pool: &PostgresPool,
        session_id: Uuid,
        tool_name: &str,
    ) -> DomainResult<()> {
        CallSessionRepository::add_tool_used(pool, session_id, tool_name).await
    }
    
    /// Get session by Twilio call SID
    pub async fn get_session_by_sid(
        pool: &PostgresPool,
        call_sid: &str,
    ) -> DomainResult<CallSession> {
        CallSessionRepository::find_by_twilio_sid(pool, call_sid).await
    }
    
    /// Get calls today count (admin stats)
    pub async fn count_calls_today(pool: &PostgresPool) -> DomainResult<i64> {
        CallSessionRepository::count_today(pool).await
    }
}

