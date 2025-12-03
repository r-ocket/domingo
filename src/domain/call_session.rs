//! Call session domain model

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Voice provider/backend used for the call
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum VoiceProvider {
    #[default]
    OpenaiRealtime,
    Elevenlabs,
}

impl std::fmt::Display for VoiceProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VoiceProvider::OpenaiRealtime => write!(f, "openai_realtime"),
            VoiceProvider::Elevenlabs => write!(f, "elevenlabs"),
        }
    }
}

impl std::str::FromStr for VoiceProvider {
    type Err = String;
    
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "openai_realtime" | "openai" | "gpt-realtime" => Ok(VoiceProvider::OpenaiRealtime),
            "elevenlabs" | "eleven" | "11labs" => Ok(VoiceProvider::Elevenlabs),
            _ => Err(format!("Invalid voice provider: {}", s)),
        }
    }
}

/// Status of a call session
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CallStatus {
    Active,
    Completed,
    Transferred,
    Failed,
}

impl std::fmt::Display for CallStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CallStatus::Active => write!(f, "active"),
            CallStatus::Completed => write!(f, "completed"),
            CallStatus::Transferred => write!(f, "transferred"),
            CallStatus::Failed => write!(f, "failed"),
        }
    }
}

impl std::str::FromStr for CallStatus {
    type Err = String;
    
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "active" => Ok(CallStatus::Active),
            "completed" => Ok(CallStatus::Completed),
            "transferred" => Ok(CallStatus::Transferred),
            "failed" => Ok(CallStatus::Failed),
            _ => Err(format!("Invalid call status: {}", s)),
        }
    }
}

/// Call session with the AI assistant
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallSession {
    pub id: Uuid,
    pub elder_id: Uuid,
    pub twilio_call_sid: String,
    pub from_number: String,
    pub status: CallStatus,
    pub voice_provider: VoiceProvider,
    pub started_at: DateTime<Utc>,
    pub ended_at: Option<DateTime<Utc>>,
    pub duration_seconds: Option<i32>,
    pub summary_text: Option<String>,
    pub transcript: Option<String>,
    pub tools_used: Vec<String>,
    pub transferred_to: Option<String>,
    pub metadata: Option<serde_json::Value>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Request to create a call session
#[derive(Debug, Clone)]
pub struct CreateCallSessionRequest {
    pub elder_id: Uuid,
    pub twilio_call_sid: String,
    pub from_number: String,
    pub voice_provider: VoiceProvider,
}

/// Request to update call session
#[derive(Debug, Clone)]
pub struct UpdateCallSessionRequest {
    pub status: Option<CallStatus>,
    pub ended_at: Option<DateTime<Utc>>,
    pub duration_seconds: Option<i32>,
    pub summary_text: Option<String>,
    pub transcript: Option<String>,
    pub tools_used: Option<Vec<String>>,
    pub transferred_to: Option<String>,
    pub metadata: Option<serde_json::Value>,
}

/// Call log entry for display
#[derive(Debug, Clone, Serialize)]
#[allow(dead_code)]
pub struct CallLogEntry {
    pub id: Uuid,
    pub elder_name: String,
    pub from_number: String,
    pub status: CallStatus,
    pub voice_provider: VoiceProvider,
    pub started_at: DateTime<Utc>,
    pub duration_seconds: Option<i32>,
    pub summary_text: Option<String>,
    pub tools_used: Vec<String>,
}

/// Tool call during a session
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub name: String,
    pub arguments: serde_json::Value,
    pub result: Option<serde_json::Value>,
    pub timestamp: DateTime<Utc>,
}

