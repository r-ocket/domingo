//! ElevenLabs Conversational AI client
//!
//! Provides real-time voice conversation capabilities using ElevenLabs' 
//! Conversational AI with Claude Sonnet 4.5 as the LLM backend.

use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::sync::mpsc;
use tokio_tungstenite::{connect_async, tungstenite::Message};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;

/// ElevenLabs Conversational AI client
pub struct ElevenLabsClient {
    api_key: String,
}

impl ElevenLabsClient {
    /// Create a new ElevenLabs client
    pub fn new(api_key: &str) -> Self {
        Self {
            api_key: api_key.to_string(),
        }
    }
    
    /// Connect to the Conversational AI and return a session handle
    pub async fn connect_conversation(
        &self,
        agent_config: AgentConfig,
    ) -> Result<ConversationSession, ElevenLabsError> {
        // ElevenLabs Conversational AI WebSocket endpoint
        let url = "wss://api.elevenlabs.io/v1/convai/conversation";
        
        // Create WebSocket request with auth
        let mut request = url.into_client_request()
            .map_err(|e| ElevenLabsError::Connection(e.to_string()))?;
        
        request.headers_mut().insert(
            "xi-api-key",
            self.api_key.parse().unwrap(),
        );
        
        let (ws_stream, _) = connect_async(request)
            .await
            .map_err(|e| ElevenLabsError::Connection(e.to_string()))?;
        
        let (write, read) = ws_stream.split();
        
        // Create channels for communication
        let (outbound_tx, outbound_rx) = mpsc::channel::<ClientMessage>(100);
        let (inbound_tx, inbound_rx) = mpsc::channel::<ServerMessage>(100);
        
        // Spawn task to handle outbound messages
        let write = std::sync::Arc::new(tokio::sync::Mutex::new(write));
        let write_clone = write.clone();
        
        tokio::spawn(async move {
            let mut rx = outbound_rx;
            while let Some(msg) = rx.recv().await {
                let json = serde_json::to_string(&msg).unwrap();
                let mut guard = write_clone.lock().await;
                if guard.send(Message::Text(json.into())).await.is_err() {
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
                        if let Ok(event) = serde_json::from_str::<ServerMessage>(&text) {
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
        
        let session = ConversationSession {
            outbound_tx,
            inbound_rx,
        };
        
        // Initialize the conversation with agent config
        session.initialize(agent_config).await?;
        
        Ok(session)
    }
}

/// Configuration for the ElevenLabs conversation agent
#[derive(Debug, Clone)]
pub struct AgentConfig {
    /// System prompt / instructions for the agent
    pub system_prompt: String,
    /// First message the agent should say (optional)
    pub first_message: Option<String>,
    /// Tool definitions for function calling
    pub tools: Vec<ToolDefinition>,
}

/// Tool definition for ElevenLabs function calling
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    #[serde(rename = "type")]
    pub tool_type: String,
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

impl ToolDefinition {
    pub fn function(name: &str, description: &str, parameters: serde_json::Value) -> Self {
        Self {
            tool_type: "function".to_string(),
            name: name.to_string(),
            description: description.to_string(),
            parameters,
        }
    }
}

/// Active conversation session
pub struct ConversationSession {
    outbound_tx: mpsc::Sender<ClientMessage>,
    inbound_rx: mpsc::Receiver<ServerMessage>,
}

impl ConversationSession {
    /// Initialize the conversation with configuration
    async fn initialize(&self, config: AgentConfig) -> Result<(), ElevenLabsError> {
        // Build conversation config message
        let init_msg = ClientMessage::ConversationInitiation {
            conversation_config_override: ConversationConfigOverride {
                agent: AgentOverride {
                    prompt: PromptOverride {
                        prompt: config.system_prompt,
                    },
                    first_message: config.first_message,
                    language: "es".to_string(),
                },
                tts: TtsOverride {
                    model_id: "eleven_flash_v2_5".to_string(),
                    voice_id: "pFZP5JQG7iQjIQuC4Bku".to_string(), // Juan - Spanish voice
                    optimize_streaming_latency: Some(3),
                },
                stt: SttOverride {
                    model: "nova-2".to_string(),
                    language: "es".to_string(),
                },
                // Configure LLM to use Claude Sonnet 4.5
                llm: LlmOverride {
                    provider: "anthropic".to_string(),
                    model: "claude-sonnet-4-5-20241022".to_string(),
                },
                tools: Some(config.tools),
            },
        };
        
        self.send_message(init_msg).await
    }
    
    /// Send a message to ElevenLabs
    async fn send_message(&self, msg: ClientMessage) -> Result<(), ElevenLabsError> {
        self.outbound_tx
            .send(msg)
            .await
            .map_err(|e| ElevenLabsError::Send(e.to_string()))
    }
    
    /// Receive the next event from ElevenLabs
    pub async fn recv_event(&mut self) -> Option<ServerMessage> {
        self.inbound_rx.recv().await
    }
    
    /// Send audio input (expects base64-encoded audio)
    /// Note: ElevenLabs expects PCM16 audio, so we may need to convert from g711_ulaw
    pub async fn send_audio(&self, audio_base64: &str) -> Result<(), ElevenLabsError> {
        let msg = ClientMessage::UserAudioChunk {
            audio: audio_base64.to_string(),
        };
        self.send_message(msg).await
    }
    
    /// Send a tool call result back to ElevenLabs
    pub async fn send_tool_result(
        &self,
        tool_call_id: &str,
        result: serde_json::Value,
    ) -> Result<(), ElevenLabsError> {
        let msg = ClientMessage::ClientToolResult {
            tool_call_id: tool_call_id.to_string(),
            result: serde_json::to_string(&result).unwrap_or_default(),
            is_error: false,
        };
        self.send_message(msg).await
    }
    
    /// Signal end of user input
    pub async fn end_user_input(&self) -> Result<(), ElevenLabsError> {
        let msg = ClientMessage::UserInputAudioBufferCommit;
        self.send_message(msg).await
    }
    
    /// Interrupt the agent's speech
    pub async fn interrupt(&self) -> Result<(), ElevenLabsError> {
        let msg = ClientMessage::UserInterrupt;
        self.send_message(msg).await
    }
}

/// Client messages sent to ElevenLabs
#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientMessage {
    /// Initialize conversation with config
    ConversationInitiation {
        conversation_config_override: ConversationConfigOverride,
    },
    /// Send audio chunk
    #[serde(rename = "user_audio_chunk")]
    UserAudioChunk {
        audio: String,
    },
    /// Commit the audio buffer (signal end of speech)
    #[serde(rename = "user_input_audio_buffer_commit")]
    UserInputAudioBufferCommit,
    /// User interruption
    #[serde(rename = "user_interrupt")]
    UserInterrupt,
    /// Tool call result
    #[serde(rename = "client_tool_result")]
    ClientToolResult {
        tool_call_id: String,
        result: String,
        is_error: bool,
    },
    /// Ping for keepalive
    Ping,
}

#[derive(Debug, Serialize)]
pub struct ConversationConfigOverride {
    pub agent: AgentOverride,
    pub tts: TtsOverride,
    pub stt: SttOverride,
    pub llm: LlmOverride,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<ToolDefinition>>,
}

#[derive(Debug, Serialize)]
pub struct AgentOverride {
    pub prompt: PromptOverride,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub first_message: Option<String>,
    pub language: String,
}

#[derive(Debug, Serialize)]
pub struct PromptOverride {
    pub prompt: String,
}

#[derive(Debug, Serialize)]
pub struct TtsOverride {
    pub model_id: String,
    pub voice_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub optimize_streaming_latency: Option<i32>,
}

#[derive(Debug, Serialize)]
pub struct SttOverride {
    pub model: String,
    pub language: String,
}

#[derive(Debug, Serialize)]
pub struct LlmOverride {
    pub provider: String,
    pub model: String,
}

/// Server messages received from ElevenLabs
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMessage {
    /// Conversation started
    ConversationInitiationMetadata {
        conversation_id: String,
    },
    /// Audio chunk from the agent
    Audio {
        audio: String,  // base64-encoded audio
    },
    /// Audio generation completed
    #[serde(rename = "audio_done")]
    AudioDone,
    /// Agent transcript (what the agent is saying)
    AgentTranscript {
        text: String,
    },
    /// User transcript (what the user said)
    UserTranscript {
        text: String,
    },
    /// Tool call request from the agent
    #[serde(rename = "client_tool_call")]
    ClientToolCall {
        tool_call_id: String,
        tool_name: String,
        parameters: serde_json::Value,
    },
    /// Agent is thinking/processing
    AgentThinking,
    /// User started speaking
    UserStartedSpeaking,
    /// User stopped speaking  
    UserStoppedSpeaking,
    /// Agent started speaking
    AgentStartedSpeaking,
    /// Agent stopped speaking
    AgentStoppedSpeaking,
    /// Interruption detected
    Interruption,
    /// Error occurred
    Error {
        message: String,
        code: Option<String>,
    },
    /// Pong response
    Pong,
    /// Unknown message type
    #[serde(other)]
    Unknown,
}

/// ElevenLabs API errors
#[derive(Debug, thiserror::Error)]
pub enum ElevenLabsError {
    #[error("Connection error: {0}")]
    Connection(String),
    
    #[error("Send error: {0}")]
    Send(String),
    
    #[error("API error: {0}")]
    Api(String),
}

/// Build the tool definitions for ElevenLabs (same tools as OpenAI)
pub fn build_elevenlabs_tools() -> Vec<ToolDefinition> {
    vec![
        ToolDefinition::function(
            "request_ride",
            "Solicitar un viaje en Uber. Usa los nombres de ubicaciones que están en el contexto.",
            json!({
                "type": "object",
                "properties": {
                    "to_location": {
                        "type": "string",
                        "description": "El nombre del destino (debe ser una ubicación guardada del contexto)"
                    },
                    "from_location": {
                        "type": "string",
                        "description": "Opcional: punto de recogida. Si no se especifica, se usa la casa."
                    }
                },
                "required": ["to_location"]
            }),
        ),
        ToolDefinition::function(
            "call_contact",
            "Transferir la llamada a un contacto. Usa el nombre exacto del contacto que aparece en el contexto.",
            json!({
                "type": "object",
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

/// System prompt for ElevenLabs (same as OpenAI, adapted slightly)
pub const ELEVENLABS_SYSTEM_PROMPT: &str = r#"## Identidad
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
Solo tienes DOS herramientas:
- **request_ride**: Para pedir un Uber. Usa los nombres EXACTOS de los destinos del contexto.
- **call_contact**: Para transferir la llamada. Usa el nombre EXACTO del contacto del contexto.

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
4. Usa la herramienta request_ride

## Flujo para Transferir Llamada
1. Confirma con quién quiere hablar
2. Busca el nombre exacto en la lista de contactos del contexto
3. Avisa: "Lo comunico con [nombre], un momento..."
4. Usa la herramienta call_contact con el nombre EXACTO

## Flujo para Medicamentos
1. Consulta la información que está en el contexto
2. Responde usando SOLO esa información
3. Si el medicamento no está listado, di "No tengo ese medicamento en su registro"
"#;

