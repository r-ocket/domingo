//! Gemini Live API (Gemini API) WebSocket client
//!
//! Implements the Live API raw websocket protocol (v1beta):
//! `wss://generativelanguage.googleapis.com/ws/google.ai.generativelanguage.v1beta.GenerativeService.BidiGenerateContent`
//!
//! References:
//! - https://ai.google.dev/api/live
//! - https://ai.google.dev/gemini-api/docs/live-tools

#![allow(non_snake_case)]

use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::json;
use base64::Engine;
use http::header::HeaderValue;
use tokio::sync::mpsc;
use tokio_tungstenite::{connect_async, tungstenite::Message};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;

use crate::mcp;

const DEFAULT_LIVE_WSS_ENDPOINT: &str =
    "wss://generativelanguage.googleapis.com/ws/google.ai.generativelanguage.v1beta.GenerativeService.BidiGenerateContent";

/// Gemini Live API client (Gemini API, not Vertex)
pub struct GeminiLiveClient {
    api_key: String,
}

impl GeminiLiveClient {
    pub fn new(api_key: &str) -> Self {
        Self {
            api_key: api_key.to_string(),
        }
    }

    /// Connect to Live API and configure the session.
    ///
    /// `resume_handle`: if provided, attempts to resume an existing session per SessionResumptionConfig.handle.
    pub async fn connect_live(
        &self,
        system_instruction: String,
        tools: Vec<mcp::ToolDefinition>,
        resume_handle: Option<String>,
    ) -> Result<GeminiLiveSession, GeminiLiveError> {
        // Use `?key=` because it's the most common pattern for API-key auth.
        // We also set the `x-goog-api-key` header for compatibility; servers typically accept either.
        let url = format!("{}?key={}", DEFAULT_LIVE_WSS_ENDPOINT, urlencoding::encode(&self.api_key));

        let mut request = url
            .into_client_request()
            .map_err(|e: tokio_tungstenite::tungstenite::Error| {
                GeminiLiveError::Connection(format!("{e:?}"))
            })?;

        request.headers_mut().insert(
            "x-goog-api-key",
            HeaderValue::from_str(&self.api_key)
                .map_err(|e| GeminiLiveError::Connection(format!("{e:?}")))?,
        );

        let (ws_stream, _) = connect_async(request)
            .await
            .map_err(|e| GeminiLiveError::Connection(e.to_string()))?;

        let (write, read) = ws_stream.split();

        let (outbound_tx, outbound_rx) = mpsc::channel::<ClientMessage>(200);
        let (inbound_tx, inbound_rx) = mpsc::channel::<ServerMessage>(200);

        // Outbound writer task
        let write = std::sync::Arc::new(tokio::sync::Mutex::new(write));
        let write_clone = write.clone();
        tokio::spawn(async move {
            let mut rx = outbound_rx;
            while let Some(msg) = rx.recv().await {
                let json_str = serde_json::to_string(&msg).unwrap_or_else(|_| "{}".to_string());
                let mut guard = write_clone.lock().await;
                if guard.send(Message::Text(json_str.into())).await.is_err() {
                    break;
                }
            }
        });

        // Inbound reader task
        tokio::spawn(async move {
            let mut read = read;
            let tx = inbound_tx;
            while let Some(msg) = read.next().await {
                match msg {
                    Ok(Message::Text(text)) => {
                        // The server uses a union where exactly one of fields is present.
                        // We deserialize into a permissive struct so we can ignore unknown fields.
                        if let Ok(event) = serde_json::from_str::<ServerMessage>(&text) {
                            let _ = tx.send(event).await;
                        }
                    }
                    Ok(Message::Close(_)) => break,
                    Err(_) => break,
                    _ => {}
                }
            }
        });

        let session = GeminiLiveSession {
            outbound_tx,
            inbound_rx,
        };

        session
            .send_setup(system_instruction, tools, resume_handle)
            .await?;

        Ok(session)
    }
}

/// Active Gemini Live session
pub struct GeminiLiveSession {
    outbound_tx: mpsc::Sender<ClientMessage>,
    inbound_rx: mpsc::Receiver<ServerMessage>,
}

impl GeminiLiveSession {
    pub async fn recv_event(&mut self) -> Option<ServerMessage> {
        self.inbound_rx.recv().await
    }

    pub async fn send_realtime_audio_pcm16_16khz(&self, audio_pcm16_le: &[u8]) -> Result<(), GeminiLiveError> {
        let msg = ClientMessage::realtime_audio(audio_pcm16_le, "audio/pcm;rate=16000");
        self.send(msg).await
    }

    pub async fn send_audio_stream_end(&self) -> Result<(), GeminiLiveError> {
        self.send(ClientMessage::realtime_audio_stream_end()).await
    }

    pub async fn send_client_text_turn(&self, text: &str, turn_complete: bool) -> Result<(), GeminiLiveError> {
        self.send(ClientMessage::client_text_turn(text, turn_complete))
            .await
    }

    pub async fn send_tool_responses(
        &self,
        function_responses: Vec<FunctionResponse>,
    ) -> Result<(), GeminiLiveError> {
        self.send(ClientMessage::tool_response(function_responses))
            .await
    }

    async fn send_setup(
        &self,
        system_instruction: String,
        tools: Vec<mcp::ToolDefinition>,
        resume_handle: Option<String>,
    ) -> Result<(), GeminiLiveError> {
        let msg = ClientMessage::setup(system_instruction, tools, resume_handle);
        self.send(msg).await
    }

    async fn send(&self, msg: ClientMessage) -> Result<(), GeminiLiveError> {
        self.outbound_tx
            .send(msg)
            .await
            .map_err(|e| GeminiLiveError::Send(e.to_string()))
    }
}

#[derive(Debug, thiserror::Error)]
pub enum GeminiLiveError {
    #[error("Connection error: {0}")]
    Connection(String),
    #[error("Send error: {0}")]
    Send(String),
    #[error("API error: {0}")]
    Api(String),
}

// -----------------------------
// Client messages (JSON)
// -----------------------------

#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum ClientMessage {
    Setup { setup: Setup },
    ClientContent { clientContent: ClientContent },
    RealtimeInput { realtimeInput: RealtimeInput },
    ToolResponse { toolResponse: ToolResponse },
}

impl ClientMessage {
    pub fn setup(
        system_instruction_text: String,
        tools: Vec<mcp::ToolDefinition>,
        resume_handle: Option<String>,
    ) -> Self {
        let tool_defs = build_function_declarations(tools);

        // Note: these knobs are intentionally conservative. We can tune after field tests.
        // Live API requires responseModalities to be either TEXT or AUDIO (not both).
        let generation_config = json!({
            "responseModalities": ["AUDIO"],
            "temperature": 0.7,
            "maxOutputTokens": 1024
        });

        let system_instruction = Content {
            role: Some("system".to_string()),
            parts: vec![Part {
                text: Some(system_instruction_text),
                ..Default::default()
            }],
        };

        let setup = Setup {
            model: "models/gemini-2.5-flash-native-audio-preview-12-2025".to_string(),
            generationConfig: generation_config,
            systemInstruction: Some(system_instruction),
            tools: tool_defs,
            // Enable automatic (server-side) VAD; tune for slower speech.
            realtimeInputConfig: Some(json!({
                "automaticActivityDetection": {
                    "disabled": false,
                    "startOfSpeechSensitivity": "START_SENSITIVITY_LOW",
                    "endOfSpeechSensitivity": "END_SENSITIVITY_LOW",
                    "prefixPaddingMs": 60,
                    "silenceDurationMs": 900
                }
            })),
            // Enable transcriptions (we use these for admin monitoring).
            inputAudioTranscription: Some(json!({})),
            outputAudioTranscription: Some(json!({})),
            // Keep sessions alive across websocket resets.
            sessionResumption: Some(SessionResumptionConfig { handle: resume_handle }),
            // Avoid hitting the 128k context cap in long calls.
            contextWindowCompression: Some(json!({ "slidingWindow": {} })),
        };

        ClientMessage::Setup { setup }
    }

    pub fn client_text_turn(text: &str, turn_complete: bool) -> Self {
        ClientMessage::ClientContent {
            clientContent: ClientContent {
                turns: vec![Content {
                    role: Some("user".to_string()),
                    parts: vec![Part {
                        text: Some(text.to_string()),
                        ..Default::default()
                    }],
                }],
                turnComplete: Some(turn_complete),
            },
        }
    }

    pub fn realtime_audio(audio_bytes: &[u8], mime_type: &str) -> Self {
        ClientMessage::RealtimeInput {
            realtimeInput: RealtimeInput {
                audio: Some(Blob::from_bytes(audio_bytes, mime_type)),
                audioStreamEnd: None,
                activityStart: None,
                activityEnd: None,
                text: None,
                video: None,
                mediaChunks: None,
            },
        }
    }

    pub fn realtime_audio_stream_end() -> Self {
        ClientMessage::RealtimeInput {
            realtimeInput: RealtimeInput {
                audio: None,
                audioStreamEnd: Some(true),
                activityStart: None,
                activityEnd: None,
                text: None,
                video: None,
                mediaChunks: None,
            },
        }
    }

    pub fn tool_response(function_responses: Vec<FunctionResponse>) -> Self {
        ClientMessage::ToolResponse {
            toolResponse: ToolResponse { functionResponses: function_responses },
        }
    }
}

#[derive(Debug, Serialize)]
pub struct Setup {
    pub model: String,
    #[serde(skip_serializing_if = "serde_json::Value::is_null")]
    pub generationConfig: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub systemInstruction: Option<Content>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub realtimeInputConfig: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inputAudioTranscription: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub outputAudioTranscription: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sessionResumption: Option<SessionResumptionConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub contextWindowCompression: Option<serde_json::Value>,
}

#[derive(Debug, Serialize)]
pub struct SessionResumptionConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub handle: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ClientContent {
    pub turns: Vec<Content>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub turnComplete: Option<bool>,
}

#[derive(Debug, Serialize, Default, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Content {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    pub parts: Vec<Part>,
}

#[derive(Debug, Serialize, Default, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Part {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inline_data: Option<Blob>,
}

#[derive(Debug, Serialize)]
pub struct ToolResponse {
    pub functionResponses: Vec<FunctionResponse>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct FunctionResponse {
    pub id: String,
    pub name: String,
    pub response: serde_json::Value,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RealtimeInput {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audio: Option<Blob>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub video: Option<Blob>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audioStreamEnd: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub activityStart: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub activityEnd: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mediaChunks: Option<Vec<Blob>>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Blob {
    pub mime_type: String,
    /// Base64 encoded data.
    pub data: String,
}

impl Blob {
    pub fn from_bytes(bytes: &[u8], mime_type: &str) -> Self {
        Self {
            mime_type: mime_type.to_string(),
            data: base64::engine::general_purpose::STANDARD.encode(bytes),
        }
    }
}

fn build_function_declarations(tools: Vec<mcp::ToolDefinition>) -> Vec<serde_json::Value> {
    if tools.is_empty() {
        return vec![];
    }

    let fns: Vec<serde_json::Value> = tools
        .into_iter()
        .map(|t| {
            json!({
                "name": t.name,
                "description": t.description,
                "parameters": t.input_schema
            })
        })
        .collect();

    vec![json!({ "functionDeclarations": fns })]
}

// -----------------------------
// Server messages (JSON)
// -----------------------------

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ServerMessage {
    #[serde(default)]
    pub setup_complete: Option<serde_json::Value>,
    #[serde(default)]
    pub server_content: Option<ServerContent>,
    #[serde(default)]
    pub tool_call: Option<ToolCall>,
    #[serde(default)]
    pub tool_call_cancellation: Option<ToolCallCancellation>,
    #[serde(default)]
    pub session_resumption_update: Option<SessionResumptionUpdate>,
    #[serde(default)]
    pub go_away: Option<GoAway>,
    #[serde(default)]
    pub usage_metadata: Option<serde_json::Value>,
    #[serde(default)]
    pub error: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ServerContent {
    #[serde(default)]
    pub model_turn: Option<Content>,
    #[serde(default)]
    pub turn_complete: Option<bool>,
    #[serde(default)]
    pub generation_complete: Option<bool>,
    #[serde(default)]
    pub interrupted: Option<bool>,
    #[serde(default)]
    pub input_transcription: Option<Transcription>,
    #[serde(default)]
    pub output_transcription: Option<Transcription>,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Transcription {
    pub text: String,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ToolCall {
    #[serde(default)]
    pub function_calls: Vec<FunctionCall>,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct FunctionCall {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub args: serde_json::Value,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ToolCallCancellation {
    #[serde(default)]
    pub ids: Vec<String>,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SessionResumptionUpdate {
    #[serde(default)]
    pub new_handle: Option<String>,
    #[serde(default)]
    pub resumable: Option<bool>,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct GoAway {
    #[serde(default)]
    pub time_left: Option<serde_json::Value>,
}


