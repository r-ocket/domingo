//! xAI Grok Voice Agent API client (Realtime over WebSocket)
//!
//! Docs: `https://docs.x.ai/docs/guides/voice/agent`
//! Endpoint: `wss://api.x.ai/v1/realtime`

use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tokio_tungstenite::{connect_async, tungstenite::Message};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;

const DEFAULT_XAI_REALTIME_WSS: &str = "wss://api.x.ai/v1/realtime";

#[derive(Debug, Clone)]
pub struct XaiVoiceAgentClient {
    api_key: String,
    endpoint: String,
}

impl XaiVoiceAgentClient {
    pub fn new(api_key: &str) -> Self {
        Self {
            api_key: api_key.to_string(),
            endpoint: DEFAULT_XAI_REALTIME_WSS.to_string(),
        }
    }

    #[allow(dead_code)]
    pub fn with_endpoint(mut self, endpoint: &str) -> Self {
        self.endpoint = endpoint.to_string();
        self
    }

    pub async fn connect(
        &self,
        session: XaiSessionUpdate,
    ) -> Result<XaiVoiceAgentSession, XaiVoiceAgentError> {
        let mut request = self.endpoint
            .clone()
            .into_client_request()
            .map_err(|e| XaiVoiceAgentError::Connection(e.to_string()))?;

        request.headers_mut().insert(
            "Authorization",
            format!("Bearer {}", self.api_key).parse().unwrap(),
        );
        request.headers_mut().insert(
            "Content-Type",
            "application/json".parse().unwrap(),
        );

        let (ws_stream, _) = connect_async(request)
            .await
            .map_err(|e| XaiVoiceAgentError::Connection(e.to_string()))?;

        let (write, read) = ws_stream.split();

        let (outbound_tx, outbound_rx) = mpsc::channel::<XaiClientEvent>(200);
        let (inbound_tx, inbound_rx) = mpsc::channel::<XaiServerEvent>(200);

        // writer
        let write = std::sync::Arc::new(tokio::sync::Mutex::new(write));
        let write_clone = write.clone();
        tokio::spawn(async move {
            let mut rx = outbound_rx;
            while let Some(evt) = rx.recv().await {
                let json = match serde_json::to_string(&evt) {
                    Ok(s) => s,
                    Err(_) => continue,
                };
                let mut guard = write_clone.lock().await;
                if guard.send(Message::Text(json.into())).await.is_err() {
                    break;
                }
            }
        });

        // reader
        tokio::spawn(async move {
            let mut read = read;
            let tx = inbound_tx;
            let mut logged_parse_error = false;
            while let Some(msg) = read.next().await {
                match msg {
                    Ok(Message::Text(text)) => {
                        match serde_json::from_str::<XaiServerEvent>(&text) {
                            Ok(evt) => {
                                let _ = tx.send(evt).await;
                            }
                            Err(e) => {
                                if !logged_parse_error {
                                    logged_parse_error = true;
                                    let preview: String = text.chars().take(800).collect();
                                    tracing::warn!(
                                        error = %e,
                                        preview = %preview,
                                        "xai realtime: failed to parse server event (schema mismatch?)"
                                    );
                                }
                            }
                        }
                    }
                    Ok(Message::Binary(bin)) => {
                        match serde_json::from_slice::<XaiServerEvent>(&bin) {
                            Ok(evt) => {
                                let _ = tx.send(evt).await;
                            }
                            Err(e) => {
                                if !logged_parse_error {
                                    logged_parse_error = true;
                                    tracing::warn!(error = %e, "xai realtime: failed to parse binary server event");
                                }
                            }
                        }
                    }
                    Ok(Message::Close(_)) => break,
                    Err(_) => break,
                    _ => {}
                }
            }
        });

        let session_handle = XaiVoiceAgentSession {
            outbound_tx,
            inbound_rx,
        };

        // send session.update immediately
        session_handle.send(XaiClientEvent::SessionUpdate(session)).await?;

        Ok(session_handle)
    }
}

pub struct XaiVoiceAgentSession {
    outbound_tx: mpsc::Sender<XaiClientEvent>,
    inbound_rx: mpsc::Receiver<XaiServerEvent>,
}

impl XaiVoiceAgentSession {
    pub async fn send(&self, evt: XaiClientEvent) -> Result<(), XaiVoiceAgentError> {
        self.outbound_tx
            .send(evt)
            .await
            .map_err(|e| XaiVoiceAgentError::Send(e.to_string()))
    }

    pub async fn recv_event(&mut self) -> Option<XaiServerEvent> {
        self.inbound_rx.recv().await
    }

    pub async fn append_audio_b64(&self, audio_b64: String) -> Result<(), XaiVoiceAgentError> {
        self.send(XaiClientEvent::InputAudioBufferAppend { audio: audio_b64 })
            .await
    }

    #[allow(dead_code)]
    pub async fn clear_input_audio_buffer(&self) -> Result<(), XaiVoiceAgentError> {
        self.send(XaiClientEvent::InputAudioBufferClear).await
    }

    #[allow(dead_code)]
    pub async fn commit_input_audio_buffer(&self) -> Result<(), XaiVoiceAgentError> {
        self.send(XaiClientEvent::InputAudioBufferCommit).await
    }

    pub async fn send_user_text(&self, text: &str) -> Result<(), XaiVoiceAgentError> {
        self.send(XaiClientEvent::ConversationItemCreate {
            previous_item_id: None,
            item: ConversationItem::user_text_message(text),
        })
        .await
    }

    pub async fn send_function_result(
        &self,
        call_id: &str,
        result: serde_json::Value,
    ) -> Result<(), XaiVoiceAgentError> {
        self.send(XaiClientEvent::ConversationItemCreate {
            previous_item_id: None,
            item: ConversationItem::function_call_output(call_id, result),
        })
        .await
    }

    pub async fn request_response(&self) -> Result<(), XaiVoiceAgentError> {
        self.send(XaiClientEvent::ResponseCreate).await
    }
}

#[derive(Debug, thiserror::Error)]
pub enum XaiVoiceAgentError {
    #[error("connection error: {0}")]
    Connection(String),
    #[error("send error: {0}")]
    Send(String),
    #[error("api error: {0}")]
    Api(String),
}

// ======================
// client events
// ======================

#[derive(Debug, Serialize, Clone)]
#[serde(tag = "type")]
pub enum XaiClientEvent {
    #[serde(rename = "session.update")]
    SessionUpdate(XaiSessionUpdate),

    #[serde(rename = "input_audio_buffer.append")]
    InputAudioBufferAppend { audio: String },

    #[serde(rename = "input_audio_buffer.clear")]
    InputAudioBufferClear,

    #[serde(rename = "input_audio_buffer.commit")]
    InputAudioBufferCommit,

    #[serde(rename = "conversation.item.create")]
    ConversationItemCreate {
        #[serde(skip_serializing_if = "Option::is_none")]
        previous_item_id: Option<String>,
        item: ConversationItem,
    },

    #[serde(rename = "response.create")]
    ResponseCreate,
}

#[derive(Debug, Serialize, Clone)]
pub struct XaiSessionUpdate {
    pub session: XaiSessionConfig,
}

#[derive(Debug, Serialize, Clone)]
pub struct XaiSessionConfig {
    pub instructions: String,
    pub voice: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub turn_detection: Option<XaiTurnDetection>,
    pub audio: XaiAudioConfig,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<XaiToolConfig>>,
}

#[derive(Debug, Serialize, Clone)]
pub struct XaiTurnDetection {
    pub r#type: Option<String>,
}

#[derive(Debug, Serialize, Clone)]
pub struct XaiAudioConfig {
    pub input: XaiAudioSide,
    pub output: XaiAudioSide,
}

#[derive(Debug, Serialize, Clone)]
pub struct XaiAudioSide {
    pub format: XaiAudioFormat,
}

#[derive(Debug, Serialize, Clone)]
pub struct XaiAudioFormat {
    pub r#type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rate: Option<u32>,
}

#[derive(Debug, Serialize, Clone)]
#[serde(tag = "type")]
pub enum XaiToolConfig {
    #[serde(rename = "function")]
    Function {
        name: String,
        description: String,
        parameters: serde_json::Value,
    },
    #[serde(rename = "web_search")]
    WebSearch,
    #[serde(rename = "x_search")]
    XSearch {
        #[serde(skip_serializing_if = "Option::is_none")]
        allowed_x_handles: Option<Vec<String>>,
    },
    #[serde(rename = "file_search")]
    FileSearch {
        vector_store_ids: Vec<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        max_num_results: Option<u32>,
    },
}

#[derive(Debug, Serialize, Clone)]
pub struct ConversationItem {
    pub r#type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<Vec<ConversationContentPart>>,

    // function_call_output
    #[serde(skip_serializing_if = "Option::is_none")]
    pub call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<String>,
}

impl ConversationItem {
    fn user_text_message(text: &str) -> Self {
        Self {
            r#type: "message".to_string(),
            role: Some("user".to_string()),
            content: Some(vec![ConversationContentPart {
                r#type: "input_text".to_string(),
                text: Some(text.to_string()),
                transcript: None,
            }]),
            call_id: None,
            output: None,
        }
    }

    fn function_call_output(call_id: &str, result: serde_json::Value) -> Self {
        Self {
            r#type: "function_call_output".to_string(),
            role: None,
            content: None,
            call_id: Some(call_id.to_string()),
            output: Some(serde_json::to_string(&result).unwrap_or_else(|_| "{}".to_string())),
        }
    }
}

#[derive(Debug, Serialize, Clone)]
pub struct ConversationContentPart {
    pub r#type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transcript: Option<String>,
}

// ======================
// server events
// ======================

#[derive(Debug, Deserialize, Clone)]
#[serde(tag = "type")]
pub enum XaiServerEvent {
    #[serde(rename = "session.updated")]
    SessionUpdated {
        event_id: String,
        session: serde_json::Value,
    },

    #[serde(rename = "conversation.created")]
    ConversationCreated {
        event_id: String,
        conversation: serde_json::Value,
    },

    #[serde(rename = "input_audio_buffer.speech_started")]
    InputAudioBufferSpeechStarted {
        event_id: String,
        item_id: String,
    },

    #[serde(rename = "input_audio_buffer.speech_stopped")]
    InputAudioBufferSpeechStopped {
        event_id: String,
        item_id: String,
    },

    #[serde(rename = "conversation.item.input_audio_transcription.completed")]
    ConversationItemInputAudioTranscriptionCompleted {
        event_id: String,
        item_id: String,
        transcript: String,
    },

    #[serde(rename = "response.created")]
    ResponseCreated {
        event_id: String,
        response: serde_json::Value,
    },

    #[serde(rename = "response.output_audio_transcript.delta")]
    ResponseOutputAudioTranscriptDelta {
        event_id: String,
        response_id: String,
        item_id: String,
        delta: String,
    },

    #[serde(rename = "response.output_audio_transcript.done")]
    ResponseOutputAudioTranscriptDone {
        event_id: String,
        response_id: String,
        item_id: String,
    },

    #[serde(rename = "response.output_audio.delta")]
    ResponseOutputAudioDelta {
        event_id: String,
        response_id: String,
        item_id: String,
        output_index: u32,
        content_index: u32,
        delta: String,
    },

    #[serde(rename = "response.output_audio.done")]
    ResponseOutputAudioDone {
        event_id: String,
        response_id: String,
        item_id: String,
    },

    #[serde(rename = "response.function_call_arguments.done")]
    ResponseFunctionCallArgumentsDone {
        event_id: String,
        response_id: Option<String>,
        item_id: Option<String>,
        name: String,
        call_id: String,
        arguments: String,
    },

    #[serde(rename = "response.done")]
    ResponseDone {
        event_id: String,
        response: serde_json::Value,
    },

    #[serde(rename = "error")]
    Error { error: serde_json::Value },

    #[serde(other)]
    Unknown,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session_update_serialization_pcmu() {
        let msg = XaiClientEvent::SessionUpdate(XaiSessionUpdate {
            session: XaiSessionConfig {
                instructions: "test".to_string(),
                voice: "Ara".to_string(),
                turn_detection: Some(XaiTurnDetection {
                    r#type: Some("server_vad".to_string()),
                }),
                audio: XaiAudioConfig {
                    input: XaiAudioSide {
                        format: XaiAudioFormat {
                            r#type: "audio/pcmu".to_string(),
                            rate: None,
                        },
                    },
                    output: XaiAudioSide {
                        format: XaiAudioFormat {
                            r#type: "audio/pcmu".to_string(),
                            rate: None,
                        },
                    },
                },
                tools: None,
            },
        });

        let s = serde_json::to_string(&msg).unwrap();
        assert!(s.contains("\"type\":\"session.update\""));
        assert!(s.contains("\"voice\":\"Ara\""));
        assert!(s.contains("\"turn_detection\""));
        assert!(s.contains("\"type\":\"server_vad\""));
        assert!(s.contains("\"audio\""));
        assert!(s.contains("\"type\":\"audio/pcmu\""));
        // for pcmu, rate is omitted
        assert!(!s.contains("\"rate\""));
    }

    #[test]
    fn test_parse_response_output_audio_delta() {
        let raw = r#"{
          "event_id": "event_4950",
          "type": "response.output_audio.delta",
          "response_id": "resp_001",
          "item_id": "msg_008",
          "output_index": 0,
          "content_index": 0,
          "delta": "AAAA"
        }"#;

        let evt: XaiServerEvent = serde_json::from_str(raw).unwrap();
        match evt {
            XaiServerEvent::ResponseOutputAudioDelta { delta, output_index, content_index, .. } => {
                assert_eq!(delta, "AAAA");
                assert_eq!(output_index, 0);
                assert_eq!(content_index, 0);
            }
            _ => panic!("wrong event variant"),
        }
    }

    #[test]
    fn test_parse_function_call_arguments_done() {
        let raw = r#"{
          "event_id": "event_999",
          "type": "response.function_call_arguments.done",
          "name": "get_weather",
          "call_id": "call_123",
          "arguments": "{\"location\":\"sf\"}"
        }"#;

        let evt: XaiServerEvent = serde_json::from_str(raw).unwrap();
        match evt {
            XaiServerEvent::ResponseFunctionCallArgumentsDone { name, call_id, arguments, .. } => {
                assert_eq!(name, "get_weather");
                assert_eq!(call_id, "call_123");
                assert!(arguments.contains("location"));
            }
            _ => panic!("wrong event variant"),
        }
    }
}


