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
        // Using the stable Realtime model (August 2025)
        let url = "wss://api.openai.com/v1/realtime?model=gpt-realtime-2025-08-28";
        
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
    /// 
    /// Best practices applied:
    /// - Server VAD with tuned thresholds for elderly users (longer silence tolerance)
    /// - Spanish language hint for transcription accuracy
    /// - Moderate temperature for natural but consistent responses
    /// - Token limits to manage costs
    async fn configure(&self, system_prompt: &str, tools: Vec<ToolDefinition>) -> Result<(), OpenAIError> {
        let event = RealtimeClientEvent::SessionUpdate {
            session: SessionConfig {
                // Support both text and audio modalities
                modalities: vec!["text".to_string(), "audio".to_string()],
                instructions: system_prompt.to_string(),
                // "coral" voice - warm, friendly, clear pronunciation
                // Good for elderly care applications
                voice: "coral".to_string(),
                // g711_ulaw format for Twilio telephony compatibility
                // Note: For WebRTC/browser, prefer "pcm16" or "opus" for lower latency
                input_audio_format: "g711_ulaw".to_string(),
                output_audio_format: "g711_ulaw".to_string(),
                // Transcription settings for Spanish (Mexico)
                input_audio_transcription: Some(InputAudioTranscription {
                    // gpt-4o-transcribe is faster and more accurate than whisper-1
                    model: "gpt-4o-transcribe".to_string(),
                    // Language hint improves accuracy for Spanish speakers
                    language: Some("es".to_string()),
                    // Transcription prompt to handle elderly speech patterns
                    prompt: Some("Transcripción de llamada telefónica con adulto mayor mexicano. Puede haber pausas largas, repeticiones, o habla lenta.".to_string()),
                }),
                // Server-side Voice Activity Detection
                // Tuned for elderly users who may speak slower with longer pauses
                turn_detection: Some(TurnDetection {
                    r#type: "server_vad".to_string(),
                    // Lower threshold = more sensitive to quiet speech
                    threshold: 0.4,
                    // Include 400ms before speech starts (captures "um", "eh")
                    prefix_padding_ms: 400,
                    // Wait 800ms of silence before ending turn
                    // Longer than default to accommodate slower speakers
                    silence_duration_ms: 800,
                    // Auto-create response when turn ends
                    create_response: Some(true),
                }),
                tools,
                // "auto" lets model decide when to use tools
                // Use "required" to force tool use, "none" to disable
                tool_choice: "auto".to_string(),
                // Temperature 0.7-0.8 balances consistency with natural variation
                temperature: 0.7,
                // Limit response tokens to control costs
                // 1024 is plenty for conversational responses
                // Increase if responses get cut off
                max_response_output_tokens: Some(1024),
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
    /// Max tokens for model response output (default: inf, set to control costs)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_response_output_tokens: Option<i32>,
}

#[derive(Debug, Serialize)]
pub struct InputAudioTranscription {
    /// Transcription model: "gpt-4o-transcribe" (recommended) or "whisper-1"
    pub model: String,
    /// Language hint for better transcription accuracy (e.g., "es" for Spanish)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    /// Custom prompt to guide transcription style
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct TurnDetection {
    /// "server_vad" for voice activity detection
    pub r#type: String,
    /// VAD threshold (0.0-1.0) - lower = more sensitive to speech
    pub threshold: f32,
    /// Audio to include before detected speech (ms)
    pub prefix_padding_ms: i32,
    /// Silence duration before turn ends (ms) - longer for elderly users
    pub silence_duration_ms: i32,
    /// Whether to auto-create response after turn ends (default: true)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub create_response: Option<bool>,
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
    
    #[serde(rename = "conversation.item.input_audio_transcription.completed")]
    ConversationItemInputAudioTranscriptionCompleted { transcript: String },
    
    #[serde(rename = "conversation.item.input_audio_transcription.failed")]
    ConversationItemInputAudioTranscriptionFailed { error: serde_json::Value },
    
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
            "Obtener la lista de ubicaciones guardadas del adulto mayor (casa, consultorio médico, etc.)",
            json!({
                "type": "object",
                "properties": {},
                "required": []
            }),
        ),
        ToolDefinition::function(
            "request_ride",
            "Solicitar un viaje en Uber a una ubicación guardada",
            json!({
                "type": "object",
                "properties": {
                    "location_name": {
                        "type": "string",
                        "description": "El nombre de la ubicación guardada a donde ir (ej: 'doctor', 'supermercado', 'casa')"
                    }
                },
                "required": ["location_name"]
            }),
        ),
        ToolDefinition::function(
            "get_upcoming_medications",
            "Obtener los recordatorios de medicamentos próximos para hoy",
            json!({
                "type": "object",
                "properties": {},
                "required": []
            }),
        ),
        ToolDefinition::function(
            "get_medication_schedule",
            "Obtener el horario completo de medicamentos del adulto mayor",
            json!({
                "type": "object",
                "properties": {},
                "required": []
            }),
        ),
        ToolDefinition::function(
            "get_contact_info",
            "Obtener información de contacto de una persona (familiar, doctor, etc.)",
            json!({
                "type": "object",
                "properties": {
                    "name_or_relationship": {
                        "type": "string",
                        "description": "El nombre o parentesco del contacto (ej: 'Juan', 'mi hija', 'Dr. García')"
                    }
                },
                "required": ["name_or_relationship"]
            }),
        ),
        ToolDefinition::function(
            "call_contact",
            "Transferir la llamada a un contacto (familiar, doctor, etc.)",
            json!({
                "type": "object",
                "properties": {
                    "name_or_relationship": {
                        "type": "string",
                        "description": "El nombre o parentesco del contacto a llamar"
                    }
                },
                "required": ["name_or_relationship"]
            }),
        ),
    ]
}

/// System prompt for the voice assistant (Spanish - Mexico)
/// Following OpenAI Realtime prompting best practices:
/// - Clear role and personality definition
/// - Explicit behavioral guidelines with emphasis (CAPS)
/// - Sample phrases for natural variation
/// - Tool usage instructions with user preambles
/// - Safety boundaries clearly stated
pub const SYSTEM_PROMPT: &str = r#"## Identidad
Eres Domingo, un asistente telefónico cálido y paciente diseñado para adultos mayores en México. Tu nombre viene de "domingo" porque siempre estás disponible para ayudar, como un día de descanso con la familia. Tu voz es reconfortante como la de un familiar querido. Hablas español mexicano con claridad y a un ritmo pausado.

## Estilo de Comunicación
- Usa oraciones cortas y simples
- Habla despacio y pronuncia claramente
- Varía tus respuestas para sonar natural, no robótico
- Usa expresiones cariñosas ocasionalmente: "¿Cómo le puedo ayudar?", "Con mucho gusto", "Claro que sí"
- Si no entendiste algo, di variaciones como:
  * "Disculpe, no escuché bien. ¿Podría repetirme?"
  * "Perdón, ¿me lo puede decir otra vez?"
  * "No le entendí bien. ¿Qué me decía?"

## Capacidades
Puedes ayudar con:
1. **Viajes**: Reservar un Uber a lugares guardados (doctor, supermercado, casa de familiares)
2. **Medicamentos**: Recordar qué medicinas tomar y cuándo
3. **Contactos**: Buscar información de contactos o transferir la llamada

## Uso de Herramientas
Cuando uses una herramienta, SIEMPRE avisa al usuario primero:
- Antes de buscar ubicaciones: "Déjeme revisar sus lugares guardados..."
- Antes de pedir un viaje: "Perfecto, voy a solicitar su viaje a [destino]..."
- Antes de buscar medicamentos: "Un momento, voy a revisar sus medicamentos..."
- Antes de buscar contactos: "Déjeme buscar ese contacto..."
- Antes de transferir llamada: "Lo voy a comunicar con [nombre], un momento por favor..."

Para viajes:
1. Si no conoces los lugares, primero usa get_saved_locations
2. Confirma el destino antes de solicitar: "¿Quiere que le pida un Uber a [lugar]?"
3. Solo después de confirmación usa request_ride

## REGLAS IMPORTANTES (NUNCA ROMPER)
- NUNCA des consejos médicos ni interpretes síntomas
- NUNCA compartas información personal con terceros
- NUNCA inventes información que no tengas
- Si alguien dice "AYUDA", "EMERGENCIA" o suena muy angustiado, ofrece INMEDIATAMENTE llamar a su contacto de emergencia
- Si no puedes ayudar con algo, sé honesto: "Disculpe, eso no lo puedo hacer, pero puedo comunicarlo con alguien que sí pueda ayudarle"

## Manejo de Situaciones
- **Usuario confundido**: Repite con paciencia, simplifica, ofrece opciones concretas
- **Usuario repite lo mismo**: Responde con paciencia sin mostrar frustración
- **Silencio prolongado**: Pregunta suavemente "¿Sigue ahí?" o "¿En qué le puedo ayudar?"
- **Usuario quiere colgar**: Despídete cálidamente: "Fue un gusto ayudarle. ¡Que tenga buen día!"

## Ejemplos de Flujo Natural

Usuario: "Necesito ir al doctor"
Tú: "Claro que sí. Déjeme revisar sus lugares guardados... Veo que tiene guardado 'Consultorio Dr. García'. ¿Quiere que le pida un Uber para allá?"

Usuario: "¿Qué medicinas me tocan?"
Tú: "Con gusto le digo. Un momento... Según su horario, a las 2 de la tarde le toca tomar su Metformina de 500mg con los alimentos."

Usuario: "Quiero hablar con mi hija"
Tú: "Por supuesto. Déjeme buscar ese contacto... Encontré a María García, su hija. ¿Quiere que lo comunique con ella ahora?"
"#;

