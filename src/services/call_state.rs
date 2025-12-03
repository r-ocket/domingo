//! In-memory store for active call state and live monitoring
//!
//! Tracks active calls with their transcripts, tool calls, and status.
//! Provides SSE broadcasting for real-time updates.

use chrono::{DateTime, Utc};
use dashmap::DashMap;
use serde::Serialize;
use std::sync::Arc;
use tokio::sync::broadcast;
use uuid::Uuid;

/// Maximum number of events to buffer in broadcast channel
const BROADCAST_CAPACITY: usize = 100;

/// A single transcript entry (user or assistant speech)
#[derive(Debug, Clone, Serialize)]
pub struct TranscriptEntry {
    pub speaker: Speaker,
    pub text: String,
    pub timestamp: DateTime<Utc>,
    pub is_partial: bool,
}

/// Who is speaking
#[derive(Debug, Clone, Copy, Serialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Speaker {
    User,
    Assistant,
}

/// A tool/function call made by the AI
#[derive(Debug, Clone, Serialize)]
pub struct ToolCallEntry {
    pub id: String,
    pub name: String,
    pub arguments: String,
    pub result: Option<String>,
    pub timestamp: DateTime<Utc>,
}

/// Call status
#[derive(Debug, Clone, Copy, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum LiveCallStatus {
    Connecting,
    Active,
    Ended,
    Failed,
}

/// State for a single active call
#[derive(Debug, Clone, Serialize)]
pub struct CallState {
    pub call_sid: String,
    pub elder_id: Uuid,
    pub elder_name: String,
    pub elder_phone: String,
    pub status: LiveCallStatus,
    pub started_at: DateTime<Utc>,
    pub ended_at: Option<DateTime<Utc>>,
    pub transcript: Vec<TranscriptEntry>,
    pub tool_calls: Vec<ToolCallEntry>,
}

impl CallState {
    pub fn new(call_sid: String, elder_id: Uuid, elder_name: String, elder_phone: String) -> Self {
        Self {
            call_sid,
            elder_id,
            elder_name,
            elder_phone,
            status: LiveCallStatus::Connecting,
            started_at: Utc::now(),
            ended_at: None,
            transcript: Vec::new(),
            tool_calls: Vec::new(),
        }
    }
}

/// Events broadcast to SSE subscribers
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CallEvent {
    /// A new call has started
    CallStarted {
        call_sid: String,
        elder_id: Uuid,
        elder_name: String,
        elder_phone: String,
    },
    /// Call status changed
    StatusChanged {
        call_sid: String,
        status: LiveCallStatus,
    },
    /// New transcript text (partial or complete)
    Transcript {
        call_sid: String,
        speaker: Speaker,
        text: String,
        is_partial: bool,
    },
    /// Tool call initiated
    ToolCallStarted {
        call_sid: String,
        id: String,
        name: String,
        arguments: String,
    },
    /// Tool call completed with result
    ToolCallCompleted {
        call_sid: String,
        id: String,
        result: String,
    },
    /// Call ended
    CallEnded {
        call_sid: String,
        duration_secs: i64,
    },
}

/// Context for MCP session (used by external AI providers)
#[derive(Debug, Clone)]
pub struct McpSessionContext {
    pub elder_id: Uuid,
    pub session_id: Uuid,
    pub call_sid: String,
}

/// In-memory store for active calls
#[derive(Debug)]
pub struct CallStateStore {
    /// Active calls keyed by call_sid
    calls: DashMap<String, CallState>,
    /// Global broadcast channel for all call events
    global_tx: broadcast::Sender<CallEvent>,
    /// Per-call broadcast channels
    call_txs: DashMap<String, broadcast::Sender<CallEvent>>,
    /// MCP session tokens → context mapping
    /// Token format: random UUID for security
    mcp_sessions: DashMap<String, McpSessionContext>,
    /// Reverse lookup: call_sid → MCP token
    call_to_mcp_token: DashMap<String, String>,
}

impl Default for CallStateStore {
    fn default() -> Self {
        Self::new()
    }
}

impl CallStateStore {
    /// Create a new call state store
    pub fn new() -> Self {
        let (global_tx, _) = broadcast::channel(BROADCAST_CAPACITY);
        Self {
            calls: DashMap::new(),
            global_tx,
            call_txs: DashMap::new(),
            mcp_sessions: DashMap::new(),
            call_to_mcp_token: DashMap::new(),
        }
    }

    /// Start tracking a new call
    pub fn start_call(
        &self,
        call_sid: String,
        elder_id: Uuid,
        elder_name: String,
        elder_phone: String,
    ) {
        let state = CallState::new(
            call_sid.clone(),
            elder_id,
            elder_name.clone(),
            elder_phone.clone(),
        );
        
        // Create per-call broadcast channel
        let (tx, _) = broadcast::channel(BROADCAST_CAPACITY);
        self.call_txs.insert(call_sid.clone(), tx);
        
        // Store call state
        self.calls.insert(call_sid.clone(), state);
        
        // Broadcast event
        let event = CallEvent::CallStarted {
            call_sid,
            elder_id,
            elder_name,
            elder_phone,
        };
        self.broadcast_global(event.clone());
        
        tracing::info!("Call state store: call started");
    }

    /// Mark call as active (connected to AI)
    pub fn set_active(&self, call_sid: &str) {
        if let Some(mut call) = self.calls.get_mut(call_sid) {
            call.status = LiveCallStatus::Active;
        }
        
        let event = CallEvent::StatusChanged {
            call_sid: call_sid.to_string(),
            status: LiveCallStatus::Active,
        };
        self.broadcast(call_sid, event);
    }

    /// Add transcript entry
    pub fn add_transcript(
        &self,
        call_sid: &str,
        speaker: Speaker,
        text: String,
        is_partial: bool,
    ) {
        if let Some(mut call) = self.calls.get_mut(call_sid) {
            // If this is a partial update for the same speaker, replace the last entry
            if is_partial {
                if let Some(last) = call.transcript.last_mut() {
                    if last.speaker == speaker && last.is_partial {
                        last.text = text.clone();
                        last.timestamp = Utc::now();
                        // Don't add a new entry, just broadcast update
                        let event = CallEvent::Transcript {
                            call_sid: call_sid.to_string(),
                            speaker,
                            text,
                            is_partial,
                        };
                        drop(call); // Release lock before broadcast
                        self.broadcast(call_sid, event);
                        return;
                    }
                }
            }
            
            // Add new entry
            call.transcript.push(TranscriptEntry {
                speaker,
                text: text.clone(),
                timestamp: Utc::now(),
                is_partial,
            });
        }
        
        let event = CallEvent::Transcript {
            call_sid: call_sid.to_string(),
            speaker,
            text,
            is_partial,
        };
        self.broadcast(call_sid, event);
    }

    /// Start a tool call
    pub fn add_tool_call(&self, call_sid: &str, id: String, name: String, arguments: String) {
        if let Some(mut call) = self.calls.get_mut(call_sid) {
            call.tool_calls.push(ToolCallEntry {
                id: id.clone(),
                name: name.clone(),
                arguments: arguments.clone(),
                result: None,
                timestamp: Utc::now(),
            });
        }
        
        let event = CallEvent::ToolCallStarted {
            call_sid: call_sid.to_string(),
            id,
            name,
            arguments,
        };
        self.broadcast(call_sid, event);
    }

    /// Complete a tool call with result
    pub fn complete_tool_call(&self, call_sid: &str, id: &str, result: String) {
        if let Some(mut call) = self.calls.get_mut(call_sid) {
            if let Some(tool_call) = call.tool_calls.iter_mut().find(|tc| tc.id == id) {
                tool_call.result = Some(result.clone());
            }
        }
        
        let event = CallEvent::ToolCallCompleted {
            call_sid: call_sid.to_string(),
            id: id.to_string(),
            result,
        };
        self.broadcast(call_sid, event);
    }

    /// End a call
    pub fn end_call(&self, call_sid: &str, failed: bool) {
        let duration_secs = if let Some(mut call) = self.calls.get_mut(call_sid) {
            call.status = if failed {
                LiveCallStatus::Failed
            } else {
                LiveCallStatus::Ended
            };
            call.ended_at = Some(Utc::now());
            (Utc::now() - call.started_at).num_seconds()
        } else {
            0
        };
        
        let event = CallEvent::CallEnded {
            call_sid: call_sid.to_string(),
            duration_secs,
        };
        self.broadcast(call_sid, event);
        
        // Clean up per-call channel
        self.call_txs.remove(call_sid);
        
        // Keep call in store for a while (could add TTL cleanup later)
        tracing::info!(call_sid = %call_sid, duration_secs = %duration_secs, "Call state store: call ended");
    }

    /// Get a call's current state
    pub fn get_call(&self, call_sid: &str) -> Option<CallState> {
        self.calls.get(call_sid).map(|c| c.clone())
    }

    /// List all active calls
    pub fn list_active(&self) -> Vec<CallState> {
        self.calls
            .iter()
            .filter(|c| matches!(c.status, LiveCallStatus::Connecting | LiveCallStatus::Active))
            .map(|c| c.clone())
            .collect()
    }

    /// List all calls (including recently ended)
    pub fn list_all(&self) -> Vec<CallState> {
        self.calls.iter().map(|c| c.clone()).collect()
    }

    /// Subscribe to all call events (global feed)
    pub fn subscribe_global(&self) -> broadcast::Receiver<CallEvent> {
        self.global_tx.subscribe()
    }

    /// Subscribe to events for a specific call
    pub fn subscribe_call(&self, call_sid: &str) -> Option<broadcast::Receiver<CallEvent>> {
        self.call_txs.get(call_sid).map(|tx| tx.subscribe())
    }

    /// Broadcast event to global subscribers
    fn broadcast_global(&self, event: CallEvent) {
        // Ignore errors (no subscribers)
        let _ = self.global_tx.send(event);
    }

    /// Broadcast event to call-specific and global subscribers
    fn broadcast(&self, call_sid: &str, event: CallEvent) {
        // Send to call-specific subscribers
        if let Some(tx) = self.call_txs.get(call_sid) {
            let _ = tx.send(event.clone());
        }
        
        // Also send to global subscribers
        self.broadcast_global(event);
    }

    /// Clean up old ended calls (call periodically from background task)
    pub fn cleanup_old_calls(&self, max_age_secs: i64) {
        let now = Utc::now();
        let to_remove: Vec<String> = self.calls
            .iter()
            .filter(|c| {
                if let Some(ended_at) = c.ended_at {
                    (now - ended_at).num_seconds() > max_age_secs
                } else {
                    false
                }
            })
            .map(|c| c.call_sid.clone())
            .collect();
        
        for call_sid in to_remove {
            self.calls.remove(&call_sid);
            self.call_txs.remove(&call_sid);
            // Also clean up MCP session
            if let Some((_, token)) = self.call_to_mcp_token.remove(&call_sid) {
                self.mcp_sessions.remove(&token);
            }
        }
    }

    // ========================================================================
    // MCP Session Management
    // ========================================================================

    /// Create or get an MCP session token for a call
    /// 
    /// Returns the MCP server URL that external AI providers should use
    pub fn create_mcp_session(
        &self,
        call_sid: &str,
        elder_id: Uuid,
        session_id: Uuid,
        base_url: &str,
    ) -> String {
        // Check if we already have a token for this call
        if let Some(existing_token) = self.call_to_mcp_token.get(call_sid) {
            return format!("{}/api/mcp/session/{}", base_url, existing_token.value());
        }
        
        // Generate new token
        let token = Uuid::new_v4().to_string();
        
        let ctx = McpSessionContext {
            elder_id,
            session_id,
            call_sid: call_sid.to_string(),
        };
        
        self.mcp_sessions.insert(token.clone(), ctx);
        self.call_to_mcp_token.insert(call_sid.to_string(), token.clone());
        
        tracing::info!(
            call_sid = %call_sid,
            token = %token,
            "Created MCP session"
        );
        
        format!("{}/api/mcp/session/{}", base_url, token)
    }

    /// Get MCP context by session token
    pub fn get_mcp_context(&self, token: &str) -> Option<McpSessionContext> {
        self.mcp_sessions.get(token).map(|ctx| ctx.clone())
    }

    /// Get MCP token for a call (if exists)
    pub fn get_mcp_token(&self, call_sid: &str) -> Option<String> {
        self.call_to_mcp_token.get(call_sid).map(|t| t.value().clone())
    }

    /// Remove MCP session for a call
    pub fn remove_mcp_session(&self, call_sid: &str) {
        if let Some((_, token)) = self.call_to_mcp_token.remove(call_sid) {
            self.mcp_sessions.remove(&token);
            tracing::debug!(call_sid = %call_sid, "Removed MCP session");
        }
    }
}

/// Wrapper for Arc<CallStateStore> to make it cloneable in AppState
pub type SharedCallStateStore = Arc<CallStateStore>;

