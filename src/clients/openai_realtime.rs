//! OpenAI Realtime API client for voice conversations

use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::sync::mpsc;
use tokio_tungstenite::{connect_async, tungstenite::Message};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;

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
        // OpenAI Realtime API - using gpt-4o-realtime-preview
        let url = "wss://api.openai.com/v1/realtime?model=gpt-4o-realtime-preview-2024-12-17";
        
        // Create a proper WebSocket request with required headers
        let mut request = url.into_client_request()
            .map_err(|e| OpenAIError::Connection(e.to_string()))?;
        
        // Add OpenAI authentication headers
        request.headers_mut().insert(
            "Authorization",
            format!("Bearer {}", self.api_key).parse().unwrap(),
        );
        request.headers_mut().insert(
            "OpenAI-Beta",
            "realtime=v1".parse().unwrap(),
        );
        
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
                // "coral" voice - warm and natural sounding
                // Good for conversational Spanish
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
    
    /// Trigger the initial greeting response
    /// Call this after a short delay to let the caller hear the ring tone end
    pub async fn trigger_initial_greeting(&self) -> Result<(), OpenAIError> {
        let event = RealtimeClientEvent::ResponseCreate {
            response: ResponseConfig {
                modalities: vec!["text".to_string(), "audio".to_string()],
                instructions: None,
            },
        };
        self.send_event(event).await
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
/// Only includes tools that require real actions (Uber, call transfer)
/// All other info (medications, contacts, locations) is in the context
#[allow(dead_code)]
pub fn build_assistant_tools() -> Vec<ToolDefinition> {
    vec![
        ToolDefinition::function(
            "search_contacts",
            "Buscar contactos guardados por nombre. Devuelve candidatos con IDs. Después usa call_contact_by_id.",
            json!({
                "type": "object",
                "additionalProperties": false,
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Nombre parcial del contacto (por ejemplo: 'Alejandro')"
                    }
                },
                "required": ["query"]
            }),
        ),
        ToolDefinition::function(
            "call_contact_by_id",
            "Transferir la llamada a un contacto guardado usando contact_id. NUNCA inventes números: el sistema marca el teléfono guardado.",
            json!({
                "type": "object",
                "additionalProperties": false,
                "properties": {
                    "contact_id": {
                        "type": "string",
                        "description": "UUID del contacto (de search_contacts)"
                    }
                },
                "required": ["contact_id"]
            }),
        ),
        ToolDefinition::function(
            "search_locations",
            "Buscar ubicaciones guardadas por nombre o dirección. Devuelve candidatos con IDs. Después usa request_ride_by_location_id.",
            json!({
                "type": "object",
                "additionalProperties": false,
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Nombre o parte de la dirección"
                    },
                    "include_home": {
                        "type": "boolean",
                        "description": "Si true, también incluye CASA",
                        "default": false
                    }
                },
                "required": ["query"]
            }),
        ),
        ToolDefinition::function(
            "request_ride_by_location_id",
            "Solicitar un viaje en Uber usando IDs de ubicaciones guardadas (evita errores de coincidencia).",
            json!({
                "type": "object",
                "additionalProperties": false,
                "properties": {
                    "to_location_id": {
                        "type": "string",
                        "description": "UUID del destino (de search_locations)"
                    },
                    "from_location_id": {
                        "type": "string",
                        "description": "UUID opcional del punto de recogida (de search_locations). Si no se pasa, se usa CASA."
                    }
                },
                "required": ["to_location_id"]
            }),
        ),
        ToolDefinition::function(
            "request_ride",
            "Solicitar un viaje en Uber. Preferido: usa request_ride_by_location_id. Si usas este tool, pasa el nombre/dirección guardada; si hay ambigüedad, NO se pedirá el Uber.",
            json!({
                "type": "object",
                "additionalProperties": false,
                "properties": {
                    "to_location": {
                        "type": "string",
                        "description": "Destino: nombre o dirección guardada"
                    },
                    "from_location": {
                        "type": "string",
                        "description": "Opcional: punto de recogida (nombre o dirección guardada). Si no se especifica, se usa la casa."
                    }
                },
                "required": ["to_location"]
            }),
        ),
        ToolDefinition::function(
            "call_contact",
            "Transferir la llamada a un contacto. Preferido: usa search_contacts -> call_contact_by_id. Este tool NO transferirá si el nombre no es una coincidencia única.",
            json!({
                "type": "object",
                "additionalProperties": false,
                "properties": {
                    "contact_name": {
                        "type": "string",
                        "description": "El nombre EXACTO del contacto como aparece en la lista de contactos del contexto"
                    }
                },
                "required": ["contact_name"]
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
Eres Domingo, un asistente telefónico cálido y paciente diseñado para adultos mayores en México. Tu voz es reconfortante como la de un familiar querido. Hablas español mexicano con claridad y a un ritmo pausado.

## Saludo Inicial
Al iniciar la llamada, saluda de forma cálida y personal usando el nombre del usuario (lo encontrarás en el contexto abajo).

## Estilo de Comunicación
- Usa oraciones cortas y simples
- Habla despacio y pronuncia claramente
- Usa expresiones cariñosas: "Con mucho gusto", "Claro que sí"
- Si no entendiste algo: "Disculpe, no escuché bien. ¿Podría repetirme?"

## Capacidades
1. **Viajes**: Pedir un Uber a los lugares guardados
2. **Medicamentos**: Recordar medicinas y horarios (TODA la información está en el contexto abajo)
3. **Llamadas**: Transferir la llamada a contactos guardados

## REGLAS DE HERRAMIENTAS (MUY IMPORTANTE)
Usa herramientas cuando sea necesario y sigue el patrón **COMPOSICIONAL** (en 2 pasos) cuando aplique:

- **CONTACTOS (COMPOSICIONAL)**:
  - Paso 1: **search_contacts** con un nombre parcial para obtener candidatos con **IDs**
  - Paso 2: **call_contact_by_id** con el **contact_id** elegido
  - NUNCA inventes números. NUNCA transfieras si hay ambigüedad.

- **UBER / VIAJES (COMPOSICIONAL)**:
  - Paso 1: **search_locations** con nombre o dirección para obtener candidatos con **IDs**
  - Paso 2: **request_ride_by_location_id** con **to_location_id** (y opcionalmente **from_location_id**)
  - Si hay ambigüedad, pregunta al usuario cuál de los candidatos quiere.

Herramientas legacy (úsalas solo si es necesario):
- **request_ride**: wrapper; puede fallar con ambigüedad.
- **call_contact**: wrapper; puede fallar con ambigüedad.

## REGLAS DE INFORMACIÓN (MUY IMPORTANTE)
- Para MEDICAMENTOS: Toda la información está en el contexto abajo. NUNCA inventes medicamentos, dosis u horarios. Si preguntan sobre un medicamento que no está en el contexto, di "No tengo ese medicamento registrado".
- Para CONTACTOS: Los nombres y relaciones están en el contexto. NUNCA inventes números de teléfono.
- Para UBICACIONES: Los lugares guardados están en el contexto. NUNCA inventes direcciones.

## REGLAS DE SEGURIDAD
- NUNCA des consejos médicos
- Si dicen "EMERGENCIA" o "AYUDA", ofrece llamar a su contacto de emergencia inmediatamente
- NUNCA inventes información que no esté en el contexto

## Flujo para Pedir Uber
1. Confirma el destino con el usuario
2. Si no especifica de dónde sale, asume que es de casa
3. Avisa: "Voy a pedir su Uber a [destino]..."
4. Si no tienes un ID único del destino: usa search_locations y confirma cuál candidato es
5. Usa request_ride_by_location_id (preferido)

## Flujo para Transferir Llamada
1. Confirma con quién quiere hablar
2. Si no tienes un ID único del contacto: usa search_contacts y confirma cuál candidato es
3. Avisa: "Lo comunico con [nombre], un momento..."
4. Usa call_contact_by_id (preferido)

## Flujo para Medicamentos
1. Consulta la información que está en el contexto
2. Responde usando SOLO esa información
3. Si el medicamento no está listado, di "No tengo ese medicamento en su registro"
"#;

