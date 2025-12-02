//! OpenAI Realtime API client for voice conversations

use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::sync::mpsc;
use tokio_tungstenite::{connect_async, tungstenite::Message};

/// OpenAI Realtime API client
pub struct OpenAIClient {
    api_key: String,
}

impl OpenAIClient {
    /// Create a new OpenAI client
    pub fn new(api_key: &str) -> Self {
        Self {
            api_key: api_key.to_string(),
        }
    }
    
    /// Connect to the Realtime API and return a session handle
    pub async fn connect_realtime(
        &self,
        system_prompt: &str,
        tools: Vec<ToolDefinition>,
    ) -> Result<RealtimeSession, OpenAIError> {
        let url = "wss://api.openai.com/v1/realtime?model=gpt-4o-realtime-preview-2024-10-01";
        
        let request = http::Request::builder()
            .uri(url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("OpenAI-Beta", "realtime=v1")
            .body(())
            .map_err(|e| OpenAIError::Connection(e.to_string()))?;
        
        let (ws_stream, _) = connect_async(request)
            .await
            .map_err(|e| OpenAIError::Connection(e.to_string()))?;
        
        let (write, read) = ws_stream.split();
        
        // Create channels for communication
        let (outbound_tx, outbound_rx) = mpsc::channel::<RealtimeClientEvent>(100);
        let (inbound_tx, inbound_rx) = mpsc::channel::<RealtimeServerEvent>(100);
        
        // Spawn task to handle outbound messages
        let write: std::sync::Arc<tokio::sync::Mutex<futures_util::stream::SplitSink<_, Message>>> = std::sync::Arc::new(tokio::sync::Mutex::new(write));
        let write_clone = write.clone();
        
        tokio::spawn(async move {
            let mut rx = outbound_rx;
            while let Some(event) = rx.recv().await {
                let msg = serde_json::to_string(&event).unwrap();
                let mut guard = write_clone.lock().await;
                if guard.send(Message::Text(msg.into())).await.is_err() {
                    break;
                }
            }
        });
        
        // Spawn task to handle inbound messages
        tokio::spawn(async move {
            let mut read = read;
            let tx = inbound_tx;
            while let Some(msg) = read.next().await {
                match msg {
                    Ok(Message::Text(text)) => {
                        if let Ok(event) = serde_json::from_str::<RealtimeServerEvent>(&text) {
                            if tx.send(event).await.is_err() {
                                break;
                            }
                        }
                    }
                    Ok(Message::Close(_)) => break,
                    Err(_) => break,
                    _ => {}
                }
            }
        });
        
        let session = RealtimeSession {
            outbound_tx,
            inbound_rx,
        };
        
        // Configure the session
        session.configure(system_prompt, tools).await?;
        
        Ok(session)
    }
}

/// Active realtime session
pub struct RealtimeSession {
    outbound_tx: mpsc::Sender<RealtimeClientEvent>,
    inbound_rx: mpsc::Receiver<RealtimeServerEvent>,
}

impl RealtimeSession {
    /// Configure the session with system prompt and tools
    async fn configure(&self, system_prompt: &str, tools: Vec<ToolDefinition>) -> Result<(), OpenAIError> {
        let event = RealtimeClientEvent::SessionUpdate {
            session: SessionConfig {
                modalities: vec!["text".to_string(), "audio".to_string()],
                instructions: system_prompt.to_string(),
                voice: "alloy".to_string(),
            input_audio_format: "g711_ulaw".to_string(),
            output_audio_format: "g711_ulaw".to_string(),
                input_audio_transcription: Some(InputAudioTranscription {
                    model: "whisper-1".to_string(),
                }),
                turn_detection: Some(TurnDetection {
                    r#type: "server_vad".to_string(),
                    threshold: 0.5,
                    prefix_padding_ms: 300,
                    silence_duration_ms: 500,
                }),
                tools,
                tool_choice: "auto".to_string(),
                temperature: 0.8,
            },
        };
        
        self.send_event(event).await
    }
    
    /// Send an event to the API
    pub async fn send_event(&self, event: RealtimeClientEvent) -> Result<(), OpenAIError> {
        self.outbound_tx
            .send(event)
            .await
            .map_err(|e| OpenAIError::Send(e.to_string()))
    }
    
    /// Receive the next event from the API
    pub async fn recv_event(&mut self) -> Option<RealtimeServerEvent> {
        self.inbound_rx.recv().await
    }
    
    /// Send audio input
    pub async fn send_audio(&self, audio_base64: &str) -> Result<(), OpenAIError> {
        let event = RealtimeClientEvent::InputAudioBufferAppend {
            audio: audio_base64.to_string(),
        };
        self.send_event(event).await
    }
    
    /// Commit the audio buffer (signal end of user speech)
    pub async fn commit_audio(&self) -> Result<(), OpenAIError> {
        let event = RealtimeClientEvent::InputAudioBufferCommit;
        self.send_event(event).await
    }
    
    /// Send a function call result
    pub async fn send_function_result(
        &self,
        call_id: &str,
        result: serde_json::Value,
    ) -> Result<(), OpenAIError> {
        let event = RealtimeClientEvent::ConversationItemCreate {
            item: ConversationItem {
                r#type: "function_call_output".to_string(),
                call_id: Some(call_id.to_string()),
                output: Some(serde_json::to_string(&result).unwrap()),
                role: None,
                content: None,
            },
        };
        self.send_event(event).await?;
        
        // Trigger response generation
        let response_event = RealtimeClientEvent::ResponseCreate {
            response: ResponseConfig {
                modalities: vec!["text".to_string(), "audio".to_string()],
                instructions: None,
            },
        };
        self.send_event(response_event).await
    }
    
    /// Cancel the current response
    pub async fn cancel_response(&self) -> Result<(), OpenAIError> {
        let event = RealtimeClientEvent::ResponseCancel;
        self.send_event(event).await
    }
}

/// OpenAI API errors
#[derive(Debug, thiserror::Error)]
pub enum OpenAIError {
    #[error("Connection error: {0}")]
    Connection(String),
    
    #[error("Send error: {0}")]
    Send(String),
    
    #[error("API error: {0}")]
    Api(String),
}

/// Tool definition for function calling
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    pub r#type: String,
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

impl ToolDefinition {
    pub fn function(name: &str, description: &str, parameters: serde_json::Value) -> Self {
        Self {
            r#type: "function".to_string(),
            name: name.to_string(),
            description: description.to_string(),
            parameters,
        }
    }
}

/// Session configuration
#[derive(Debug, Serialize)]
pub struct SessionConfig {
    pub modalities: Vec<String>,
    pub instructions: String,
    pub voice: String,
    pub input_audio_format: String,
    pub output_audio_format: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_audio_transcription: Option<InputAudioTranscription>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub turn_detection: Option<TurnDetection>,
    pub tools: Vec<ToolDefinition>,
    pub tool_choice: String,
    pub temperature: f32,
}

#[derive(Debug, Serialize)]
pub struct InputAudioTranscription {
    pub model: String,
}

#[derive(Debug, Serialize)]
pub struct TurnDetection {
    pub r#type: String,
    pub threshold: f32,
    pub prefix_padding_ms: i32,
    pub silence_duration_ms: i32,
}

#[derive(Debug, Serialize)]
pub struct ResponseConfig {
    pub modalities: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,
}

/// Client events sent to the API
#[derive(Debug, Serialize)]
#[serde(tag = "type")]
#[serde(rename_all = "snake_case")]
pub enum RealtimeClientEvent {
    #[serde(rename = "session.update")]
    SessionUpdate { session: SessionConfig },
    
    #[serde(rename = "input_audio_buffer.append")]
    InputAudioBufferAppend { audio: String },
    
    #[serde(rename = "input_audio_buffer.commit")]
    InputAudioBufferCommit,
    
    #[serde(rename = "input_audio_buffer.clear")]
    InputAudioBufferClear,
    
    #[serde(rename = "conversation.item.create")]
    ConversationItemCreate { item: ConversationItem },
    
    #[serde(rename = "response.create")]
    ResponseCreate { response: ResponseConfig },
    
    #[serde(rename = "response.cancel")]
    ResponseCancel,
}

#[derive(Debug, Serialize)]
pub struct ConversationItem {
    pub r#type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<Vec<ContentPart>>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ContentPart {
    pub r#type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audio: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transcript: Option<String>,
}

/// Server events received from the API
#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
#[serde(rename_all = "snake_case")]
pub enum RealtimeServerEvent {
    #[serde(rename = "session.created")]
    SessionCreated { session: serde_json::Value },
    
    #[serde(rename = "session.updated")]
    SessionUpdated { session: serde_json::Value },
    
    #[serde(rename = "conversation.created")]
    ConversationCreated { conversation: serde_json::Value },
    
    #[serde(rename = "conversation.item.created")]
    ConversationItemCreated { item: serde_json::Value },
    
    #[serde(rename = "response.created")]
    ResponseCreated { response: serde_json::Value },
    
    #[serde(rename = "response.output_item.added")]
    ResponseOutputItemAdded { item: serde_json::Value },
    
    #[serde(rename = "response.audio.delta")]
    ResponseAudioDelta { delta: String },
    
    #[serde(rename = "response.audio.done")]
    ResponseAudioDone,
    
    #[serde(rename = "response.audio_transcript.delta")]
    ResponseAudioTranscriptDelta { delta: String },
    
    #[serde(rename = "response.audio_transcript.done")]
    ResponseAudioTranscriptDone { transcript: String },
    
    #[serde(rename = "response.text.delta")]
    ResponseTextDelta { delta: String },
    
    #[serde(rename = "response.text.done")]
    ResponseTextDone { text: String },
    
    #[serde(rename = "response.function_call_arguments.delta")]
    ResponseFunctionCallArgumentsDelta {
        call_id: String,
        delta: String,
    },
    
    #[serde(rename = "response.function_call_arguments.done")]
    ResponseFunctionCallArgumentsDone {
        call_id: String,
        name: String,
        arguments: String,
    },
    
    #[serde(rename = "response.done")]
    ResponseDone { response: serde_json::Value },
    
    #[serde(rename = "input_audio_buffer.speech_started")]
    InputAudioBufferSpeechStarted,
    
    #[serde(rename = "input_audio_buffer.speech_stopped")]
    InputAudioBufferSpeechStopped,
    
    #[serde(rename = "input_audio_buffer.committed")]
    InputAudioBufferCommitted,
    
    #[serde(rename = "error")]
    Error { error: OpenAIApiError },
    
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Deserialize)]
pub struct OpenAIApiError {
    pub r#type: String,
    pub message: String,
}

/// Build the tool definitions for the voice assistant
pub fn build_assistant_tools() -> Vec<ToolDefinition> {
    vec![
        ToolDefinition::function(
            "get_saved_locations",
            "Get the list of saved locations for the elder (home, doctor's office, etc.)",
            json!({
                "type": "object",
                "properties": {},
                "required": []
            }),
        ),
        ToolDefinition::function(
            "request_ride",
            "Request an Uber ride to a saved location",
            json!({
                "type": "object",
                "properties": {
                    "location_name": {
                        "type": "string",
                        "description": "The name of the saved location to go to (e.g., 'doctor', 'grocery store', 'home')"
                    }
                },
                "required": ["location_name"]
            }),
        ),
        ToolDefinition::function(
            "get_upcoming_medications",
            "Get the upcoming medication reminders for today",
            json!({
                "type": "object",
                "properties": {},
                "required": []
            }),
        ),
        ToolDefinition::function(
            "get_medication_schedule",
            "Get the full medication schedule for the elder",
            json!({
                "type": "object",
                "properties": {},
                "required": []
            }),
        ),
        ToolDefinition::function(
            "get_contact_info",
            "Get contact information for a person (family member, doctor, etc.)",
            json!({
                "type": "object",
                "properties": {
                    "name_or_relationship": {
                        "type": "string",
                        "description": "The name or relationship of the contact (e.g., 'John', 'my daughter', 'Dr. Smith')"
                    }
                },
                "required": ["name_or_relationship"]
            }),
        ),
        ToolDefinition::function(
            "call_contact",
            "Transfer the call to a contact (family member, doctor, etc.)",
            json!({
                "type": "object",
                "properties": {
                    "name_or_relationship": {
                        "type": "string",
                        "description": "The name or relationship of the contact to call"
                    }
                },
                "required": ["name_or_relationship"]
            }),
        ),
    ]
}

/// System prompt for the voice assistant
pub const SYSTEM_PROMPT: &str = r#"You are a kind, patient phone assistant for an elderly user. Speak slowly and clearly. Help them with:

1. Booking rides to pre-saved locations (using request_ride)
2. Medication reminders and schedules (using get_upcoming_medications or get_medication_schedule)
3. Reaching their contacts (using get_contact_info or call_contact to transfer the call)

Guidelines:
- Be warm, friendly, and reassuring
- Speak in simple, clear sentences
- Confirm important actions before taking them (like booking a ride)
- If the user seems confused, gently clarify
- Never give medical advice beyond reading their medication schedule
- If they say "help" or "emergency", offer to call their emergency contact

When using tools:
- For rides: First get their saved locations if you don't know them, then request the ride
- For medications: Tell them what they need to take and when
- For contacts: You can either read out the phone number or offer to connect them directly

Always be patient - the user may need extra time to respond or may repeat themselves."#;

