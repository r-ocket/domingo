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
use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;
use tokio::time::{timeout, Duration};
use tokio_tungstenite::{connect_async, tungstenite::Message};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;

use crate::mcp;

const DEFAULT_LIVE_WSS_ENDPOINT: &str =
    "wss://generativelanguage.googleapis.com/ws/google.ai.generativelanguage.v1beta.GenerativeService.BidiGenerateContent";

#[derive(Debug, Clone, Default)]
pub struct GeminiLiveSetupOverrides {
    /// Merged into the default `generationConfig`
    pub generation_config: Option<serde_json::Value>,
    /// Merged into the default `realtimeInputConfig`
    pub realtime_input_config: Option<serde_json::Value>,
    /// Overrides `inputAudioTranscription` if provided
    pub input_audio_transcription: Option<serde_json::Value>,
    /// Overrides `outputAudioTranscription` if provided
    pub output_audio_transcription: Option<serde_json::Value>,
    /// Voice name override (e.g., "Orus", "Kore", "Charon")
    /// See: https://cloud.google.com/vertex-ai/generative-ai/docs/live-api/configure-language-voice
    pub voice_name: Option<String>,
    /// Language code override (e.g., "es-US", "en-US")
    pub language_code: Option<String>,
    /// Override affective dialog setting
    pub enable_affective_dialog: Option<bool>,
    /// Override proactive audio setting
    pub enable_proactive_audio: Option<bool>,
}

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
        overrides: GeminiLiveSetupOverrides,
    ) -> Result<GeminiLiveSession, GeminiLiveError> {
        tracing::info!("Gemini Live: connecting websocket");
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

        let (ws_stream, _) = timeout(Duration::from_secs(8), connect_async(request))
            .await
            .map_err(|_| GeminiLiveError::Connection("websocket connect timeout".to_string()))?
            .map_err(|e| GeminiLiveError::Connection(e.to_string()))?;
        tracing::info!("Gemini Live: websocket connected");

        let (write, read) = ws_stream.split();

        let (outbound_tx, outbound_rx) = mpsc::channel::<ClientMessage>(200);
        let (inbound_tx, inbound_rx) = mpsc::channel::<ServerMessage>(200);
        let (shutdown_tx, shutdown_rx) = watch::channel(false);

        // Outbound writer task with graceful shutdown
        let write = std::sync::Arc::new(tokio::sync::Mutex::new(write));
        let write_clone = write.clone();
        let mut shutdown_rx_writer = shutdown_rx.clone();
        let writer_handle = tokio::spawn(async move {
            let mut rx = outbound_rx;
            loop {
                tokio::select! {
                    biased;
                    _ = shutdown_rx_writer.changed() => {
                        if *shutdown_rx_writer.borrow() {
                            // Graceful shutdown: send WebSocket close frame
                            let mut guard = write_clone.lock().await;
                            let close_frame = tokio_tungstenite::tungstenite::protocol::CloseFrame {
                                code: tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode::Normal,
                                reason: "session ended".into(),
                            };
                            if let Err(e) = guard.send(Message::Close(Some(close_frame))).await {
                                tracing::debug!(error = %e, "Gemini Live: failed to send close frame");
                            }
                            tracing::debug!("Gemini Live: writer task shutting down gracefully");
                            break;
                        }
                    }
                    msg_opt = rx.recv() => {
                        match msg_opt {
                            Some(msg) => {
                                let json_str = serde_json::to_string(&msg).unwrap_or_else(|_| "{}".to_string());
                                let mut guard = write_clone.lock().await;
                                if guard.send(Message::Text(json_str.into())).await.is_err() {
                                    tracing::warn!("Gemini Live: websocket send failed (writer exiting)");
                                    break;
                                }
                            }
                            None => {
                                tracing::debug!("Gemini Live: outbound channel closed (writer exiting)");
                                break;
                            }
                        }
                    }
                }
            }
        });

        // Inbound reader task with graceful shutdown
        let mut shutdown_rx_reader = shutdown_rx.clone();
        let reader_handle = tokio::spawn(async move {
            let mut read = read;
            let tx = inbound_tx;
            let mut logged_parse_error = false;
            let mut dropped_messages: u64 = 0;

            loop {
                tokio::select! {
                    biased;
                    _ = shutdown_rx_reader.changed() => {
                        if *shutdown_rx_reader.borrow() {
                            tracing::debug!("Gemini Live: reader task shutting down gracefully");
                            break;
                        }
                    }
                    msg_opt = read.next() => {
                        let msg = match msg_opt {
                            Some(Ok(m)) => m,
                            Some(Err(e)) => {
                                tracing::warn!(error = %e, "Gemini Live: websocket read error (reader exiting)");
                                break;
                            }
                            None => {
                                tracing::debug!("Gemini Live: websocket stream ended");
                                break;
                            }
                        };

                        match msg {
                            Message::Text(text) => {
                                match serde_json::from_str::<ServerMessage>(&text) {
                                    Ok(event) => {
                                        // Use try_send to avoid blocking; log if channel is full
                                        match tx.try_send(event) {
                                            Ok(_) => {}
                                            Err(mpsc::error::TrySendError::Full(evt)) => {
                                                dropped_messages += 1;
                                                if dropped_messages == 1 || dropped_messages % 100 == 0 {
                                                    tracing::warn!(
                                                        dropped = dropped_messages,
                                                        "Gemini Live: inbound channel full, dropping message"
                                                    );
                                                }
                                                // Try one more time with blocking send for critical messages
                                                if evt.tool_call.is_some() || evt.go_away.is_some() {
                                                    let _ = tx.send(evt).await;
                                                }
                                            }
                                            Err(mpsc::error::TrySendError::Closed(_)) => {
                                                tracing::debug!("Gemini Live: inbound channel closed");
                                                break;
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        if !logged_parse_error {
                                            logged_parse_error = true;
                                            let preview: String = text.chars().take(800).collect();
                                            tracing::warn!(
                                                error = %e,
                                                preview = %preview,
                                                "Gemini Live: failed to parse server message (schema mismatch?)"
                                            );
                                        }
                                    }
                                }
                            }
                            Message::Binary(bin) => {
                                match serde_json::from_slice::<ServerMessage>(&bin) {
                                    Ok(event) => {
                                        if tx.try_send(event).is_err() {
                                            dropped_messages += 1;
                                        }
                                    }
                                    Err(e) => {
                                        if !logged_parse_error {
                                            logged_parse_error = true;
                                            let preview_len = std::cmp::min(bin.len(), 80);
                                            let preview_hex = bin[..preview_len]
                                                .iter()
                                                .map(|b| format!("{:02x}", b))
                                                .collect::<Vec<_>>()
                                                .join("");
                                            tracing::warn!(
                                                error = %e,
                                                preview_hex = %preview_hex,
                                                "Gemini Live: failed to parse binary server message"
                                            );
                                        }
                                    }
                                }
                            }
                            Message::Close(frame_opt) => {
                                if let Some(frame) = frame_opt {
                                    tracing::info!(
                                        code = %frame.code,
                                        reason = %frame.reason,
                                        "Gemini Live: websocket closed by server"
                                    );
                                } else {
                                    tracing::info!("Gemini Live: websocket closed by server");
                                }
                                break;
                            }
                            Message::Ping(_) | Message::Pong(_) | Message::Frame(_) => {
                                // Protocol frames handled by tungstenite
                            }
                        }
                    }
                }
            }

            if dropped_messages > 0 {
                tracing::warn!(
                    total_dropped = dropped_messages,
                    "Gemini Live: reader exiting, total messages dropped due to backpressure"
                );
            }
        });

        let session = GeminiLiveSession {
            outbound_tx,
            inbound_rx,
            shutdown_tx,
            writer_handle,
            reader_handle,
        };

        session
            .send_setup(system_instruction, tools, resume_handle, overrides)
            .await?;

        Ok(session)
    }
}

/// Active Gemini Live session with managed background tasks.
///
/// When dropped, automatically signals shutdown to reader/writer tasks
/// and attempts graceful WebSocket closure.
pub struct GeminiLiveSession {
    outbound_tx: mpsc::Sender<ClientMessage>,
    inbound_rx: mpsc::Receiver<ServerMessage>,
    /// Shutdown signal for background tasks
    shutdown_tx: watch::Sender<bool>,
    /// Handle to the writer task (for join on drop)
    writer_handle: JoinHandle<()>,
    /// Handle to the reader task (for join on drop)
    reader_handle: JoinHandle<()>,
}

impl Drop for GeminiLiveSession {
    fn drop(&mut self) {
        // Signal shutdown to background tasks
        let _ = self.shutdown_tx.send(true);
        // Abort tasks if they haven't finished (non-blocking)
        self.writer_handle.abort();
        self.reader_handle.abort();
    }
}

impl GeminiLiveSession {
    pub async fn recv_event(&mut self) -> Option<ServerMessage> {
        self.inbound_rx.recv().await
    }

    /// Explicitly shutdown the session, sending a close frame.
    /// This is also called automatically on drop.
    pub fn shutdown(&self) {
        let _ = self.shutdown_tx.send(true);
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
        overrides: GeminiLiveSetupOverrides,
    ) -> Result<(), GeminiLiveError> {
        let msg = ClientMessage::setup(system_instruction, tools, resume_handle, overrides);
        tracing::info!("Gemini Live: sending setup");
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
        overrides: GeminiLiveSetupOverrides,
    ) -> Self {
        let tool_defs = build_function_declarations(tools);

        // Note: these knobs are intentionally conservative. We can tune after field tests.
        // Live API requires responseModalities to be either TEXT or AUDIO (not both).
        let mut generation_config = json!({
            "responseModalities": ["AUDIO"],
            "temperature": 0.7,
            "maxOutputTokens": 1024
        });

        fn merge_json(dst: &mut serde_json::Value, src: serde_json::Value) {
            match (dst, src) {
                (serde_json::Value::Object(dst_obj), serde_json::Value::Object(src_obj)) => {
                    for (k, v) in src_obj {
                        match dst_obj.get_mut(&k) {
                            Some(existing) => merge_json(existing, v),
                            None => {
                                dst_obj.insert(k, v);
                            }
                        }
                    }
                }
                (dst_slot, src_other) => {
                    *dst_slot = src_other;
                }
            }
        }

        if let Some(ov) = overrides.generation_config {
            merge_json(&mut generation_config, ov);
        }

        let system_instruction = Content {
            role: Some("system".to_string()),
            parts: vec![Part {
                text: Some(system_instruction_text),
                ..Default::default()
            }],
        };

        let default_realtime_input_config = json!({
            "automaticActivityDetection": {
                "disabled": false,
                // telephony audio is quiet + bandlimited; defaulting to LOW is too inert in practice.
                "startOfSpeechSensitivity": "START_SENSITIVITY_HIGH",
                // NOTE: some live models reject END_SENSITIVITY_MEDIUM; stick to known-good enums.
                "endOfSpeechSensitivity": "END_SENSITIVITY_LOW",
                "prefixPaddingMs": 160,
                "silenceDurationMs": 450
            }
        });

        let mut realtime_input_config = default_realtime_input_config;
        if let Some(ov) = overrides.realtime_input_config {
            merge_json(&mut realtime_input_config, ov);
        }

        // sanitize enums so bad UI/advanced-json values don't kill the websocket (1007 invalid payload).
        // if gemini rejects a value, we prefer to coerce to a safe default rather than end the call.
        if let Some(aad) = realtime_input_config
            .get_mut("automaticActivityDetection")
            .and_then(|v| v.as_object_mut())
        {
            let sanitize_enum = |val: &mut serde_json::Value, allowed: &[&str], fallback: &str| {
                let Some(s) = val.as_str() else {
                    *val = serde_json::Value::String(fallback.to_string());
                    return;
                };
                if !allowed.iter().any(|a| *a == s) {
                    *val = serde_json::Value::String(fallback.to_string());
                }
            };

            if let Some(v) = aad.get_mut("startOfSpeechSensitivity") {
                sanitize_enum(
                    v,
                    &["START_SENSITIVITY_LOW", "START_SENSITIVITY_HIGH"],
                    "START_SENSITIVITY_HIGH",
                );
            }
            if let Some(v) = aad.get_mut("endOfSpeechSensitivity") {
                sanitize_enum(
                    v,
                    &["END_SENSITIVITY_LOW", "END_SENSITIVITY_HIGH"],
                    "END_SENSITIVITY_LOW",
                );
            }
        }

        let setup = Setup {
            // Use the GA model for production stability.
            model: "models/gemini-live-2.5-flash-native-audio".to_string(),
            generationConfig: generation_config,
            systemInstruction: Some(system_instruction),
            tools: tool_defs,
            // Enable automatic (server-side) VAD; tune per call via overrides.
            realtimeInputConfig: Some(realtime_input_config),
            // Enable transcriptions (we use these for admin monitoring).
            // Note: this config's schema is strict; don't guess fields here.
            // If no overrides are provided, send `{}` (enables default transcription).
            inputAudioTranscription: Some(overrides.input_audio_transcription.unwrap_or_else(|| json!({}))),
            outputAudioTranscription: Some(overrides.output_audio_transcription.unwrap_or_else(|| json!({}))),
            // Always request session resumption so we can reconnect after transient server errors (1011/goAway).
            // If we don't have a handle yet, this serializes as `{}` and the server should send a handle via
            // `sessionResumptionUpdate.newHandle` once the session is resumable.
            sessionResumption: Some(SessionResumptionConfig { handle: resume_handle }),
            // Avoid hitting the 128k context cap in long calls.
            // Native audio accumulates ~25 tokens/second; trigger at 10k, compress to 2k.
            contextWindowCompression: Some(json!({
                "triggerTokens": 10000,
                "slidingWindow": { "targetTokens": 2048 }
            })),
            // Voice configuration: use a warm/friendly voice for elderly users.
            // Available voices: Kore, Orus, Umbriel, Laomedeia, Sadachbia (warm), etc.
            // Language: es-US (Spanish US) - es-MX not available for native audio.
            speechConfig: Some(json!({
                "voiceConfig": {
                    "prebuiltVoiceConfig": {
                        "voiceName": overrides.voice_name.as_deref().unwrap_or("Orus")
                    }
                },
                "languageCode": overrides.language_code.as_deref().unwrap_or("es-US")
            })),
            // Affective dialog: model adapts tone/style to match user's emotional expression.
            // Helpful for elderly users who may speak with varied emotional states.
            nativeAudioOutputConfig: Some(json!({
                "enableAffectiveDialog": overrides.enable_affective_dialog.unwrap_or(true)
            })),
            // Proactive audio: model controls when to respond, reducing interruptions
            // from background noise and waiting for user to finish speaking.
            proactivityConfig: Some(json!({
                "proactiveAudio": overrides.enable_proactive_audio.unwrap_or(true)
            })),
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
                // live api expects mediaChunks for realtime audio; `audio` is not consistently honored.
                audio: None,
                audioStreamEnd: None,
                activityStart: None,
                activityEnd: None,
                text: None,
                video: None,
                mediaChunks: Some(vec![Blob::from_bytes(audio_bytes, mime_type)]),
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
    /// Voice and language configuration (voiceConfig + languageCode)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub speechConfig: Option<serde_json::Value>,
    /// Native audio output settings (e.g., enableAffectiveDialog)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nativeAudioOutputConfig: Option<serde_json::Value>,
    /// Proactivity settings (e.g., proactiveAudio)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub proactivityConfig: Option<serde_json::Value>,
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
    /// Live tool-response scheduling hint (e.g. "INTERRUPT", "WHEN_IDLE", "SILENT")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scheduling: Option<String>,
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

    fn jsonschema_type_to_gemini_enum(t: &str) -> Option<&'static str> {
        match t {
            "object" => Some("OBJECT"),
            "string" => Some("STRING"),
            "number" => Some("NUMBER"),
            "integer" => Some("INTEGER"),
            "boolean" => Some("BOOLEAN"),
            "array" => Some("ARRAY"),
            _ => None,
        }
    }

    // Gemini Live API function declarations use the Generative Language "Schema" proto, whose `type`
    // is an enum serialized as strings like "OBJECT"/"STRING" in JSON. Our MCP tools use JSON Schema
    // ("object"/"string"/...) so we need a small conversion layer or tools simply won't be callable.
    fn jsonschema_to_gemini_schema(v: serde_json::Value) -> serde_json::Value {
        use serde_json::{Map, Value};

        let Value::Object(obj) = v else {
            return json!({});
        };

        // type can be string or array (e.g. ["string","null"])
        let (type_enum, nullable) = match obj.get("type") {
            Some(Value::String(t)) => (jsonschema_type_to_gemini_enum(t).unwrap_or("OBJECT"), false),
            Some(Value::Array(arr)) => {
                let mut nullable = false;
                let mut chosen: Option<&'static str> = None;
                for item in arr {
                    if let Value::String(s) = item {
                        if s == "null" {
                            nullable = true;
                            continue;
                        }
                        if chosen.is_none() {
                            chosen = jsonschema_type_to_gemini_enum(s);
                        }
                    }
                }
                (chosen.unwrap_or("OBJECT"), nullable)
            }
            _ => {
                // if type is missing but properties exist, assume object
                if obj.get("properties").is_some() { ("OBJECT", false) } else { ("OBJECT", false) }
            }
        };

        let mut out = Map::new();
        out.insert("type".to_string(), Value::String(type_enum.to_string()));

        if let Some(Value::String(desc)) = obj.get("description") {
            out.insert("description".to_string(), Value::String(desc.clone()));
        }
        if let Some(Value::Array(en)) = obj.get("enum") {
            out.insert("enum".to_string(), Value::Array(en.clone()));
        }
        if nullable {
            out.insert("nullable".to_string(), Value::Bool(true));
        }

        // object
        if type_enum == "OBJECT" {
            if let Some(Value::Object(props)) = obj.get("properties") {
                let mut new_props = Map::new();
                for (k, v) in props {
                    new_props.insert(k.clone(), jsonschema_to_gemini_schema(v.clone()));
                }
                out.insert("properties".to_string(), Value::Object(new_props));
            }
            if let Some(Value::Array(req)) = obj.get("required") {
                out.insert("required".to_string(), Value::Array(req.clone()));
            }
        }

        // array
        if type_enum == "ARRAY" {
            if let Some(items) = obj.get("items") {
                out.insert("items".to_string(), jsonschema_to_gemini_schema(items.clone()));
            }
        }

        Value::Object(out)
    }

    let fns: Vec<serde_json::Value> = tools
        .into_iter()
        .map(|t| {
            json!({
                "name": t.name,
                "description": t.description,
                "parameters": jsonschema_to_gemini_schema(t.input_schema)
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
    /// Whether this is a final transcription (no more updates expected for this segment)
    #[serde(default)]
    pub finished: Option<bool>,
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


