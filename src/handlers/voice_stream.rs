//! WebSocket handler for Twilio media stream - supports OpenAI Realtime and ElevenLabs

use std::sync::Arc;
use std::collections::VecDeque;
use axum::{
    extract::{Path, State, WebSocketUpgrade},
    response::IntoResponse,
};
use futures_util::{SinkExt, StreamExt};
use serde_json::json;
use tokio::sync::mpsc;
use uuid::Uuid;
use base64::Engine as _;

use crate::clients::{
    TwilioStreamMessage, TwilioOutboundMedia,
    RealtimeServerEvent, build_assistant_tools,
    AgentConfig, ServerMessage as ElevenLabsServerMessage,
    build_elevenlabs_tools, ELEVENLABS_SYSTEM_PROMPT,
    audio::{twilio_ulaw_base64_to_gemini_pcm16_16khz_bytes, gemini_pcm16_24khz_bytes_to_twilio_ulaw_base64},
};
use crate::domain::{CallStatus, Elder, LocationType, VoiceProvider};
use crate::mcp::ToolContext;
use crate::repositories::postgres::CaregiverRepository;
use crate::services::{
    CallService, ContactService, ElderService, LocationService,
    MedicationService, Speaker,
};
use crate::AppState;

/// Handle Twilio media stream WebSocket connection
#[tracing::instrument(skip(ws, state), fields(call_sid = %call_sid))]
pub async fn media_stream(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
    Path(call_sid): Path<String>,
) -> impl IntoResponse {
    tracing::info!("Establishing media stream WebSocket connection");
    
    ws.on_upgrade(move |socket| handle_media_stream(socket, state, call_sid))
}

async fn handle_media_stream(
    socket: axum::extract::ws::WebSocket,
    state: AppState,
    call_sid: String,
) {
    let (mut ws_sender, mut ws_receiver) = socket.split();
    
    // Get call session and elder
    let session = match CallService::get_session_by_sid(&state.db, &call_sid).await {
        Ok(s) => s,
        Err(e) => {
            tracing::error!("Failed to get call session: {}", e);
            return;
        }
    };
    
    let elder = match ElderService::get_elder(&state.db, session.elder_id, Uuid::nil(), true).await {
        Ok(e) => e,
        Err(e) => {
            tracing::error!("Failed to get elder: {}", e);
            return;
        }
    };
    
    tracing::info!(
        "Starting voice session for elder {} ({}) using {:?}",
        elder.name, elder.id, session.voice_provider
    );
    
    // Register call with live state store
    state.call_state.start_call(
        call_sid.clone(),
        elder.id,
        elder.name.clone(),
        elder.phone_number.clone(),
        session.voice_provider.to_string(),
    );
    
    // Build dynamic context with elder's data
    let dynamic_prompt = build_dynamic_prompt(&state, &elder).await;
    // Full system instruction: behavioral prompt + per-elder context
    let full_prompt = format!("{}\n\n---\n\n{}", crate::clients::SYSTEM_PROMPT, dynamic_prompt);
    
    // Create tool context for MCP
    let tool_ctx = ToolContext {
        db: state.db.clone(),
        twilio: state.twilio.clone(),
        uber: state.uber.clone(),
        elder_id: elder.id,
        session_id: session.id,
        call_sid: call_sid.clone(),
    };
    
    // Dispatch to appropriate handler based on voice provider
    match session.voice_provider {
        VoiceProvider::OpenaiRealtime => {
            handle_openai_stream(
                &state,
                &mut ws_sender,
                &mut ws_receiver,
                &call_sid,
                &elder,
                session.id,
                full_prompt,
                tool_ctx,
            ).await;
        }
        VoiceProvider::Elevenlabs => {
            handle_elevenlabs_stream(
                &state,
                &mut ws_sender,
                &mut ws_receiver,
                &call_sid,
                &elder,
                session.id,
                dynamic_prompt,
                tool_ctx,
            ).await;
        }
        VoiceProvider::GeminiLive => {
            handle_gemini_stream(
                &state,
                &mut ws_sender,
                &mut ws_receiver,
                &call_sid,
                &elder,
                session.id,
                full_prompt,
                tool_ctx,
            ).await;
        }
    }
    
    // End call in live state store
    state.call_state.end_call(&call_sid, false);
    
    tracing::info!("Voice session ended for call {}", call_sid);
}

/// Handle Gemini Live voice stream (Gemini API Live over WebSockets)
async fn handle_gemini_stream(
    state: &AppState,
    ws_sender: &mut futures_util::stream::SplitSink<axum::extract::ws::WebSocket, axum::extract::ws::Message>,
    ws_receiver: &mut futures_util::stream::SplitStream<axum::extract::ws::WebSocket>,
    call_sid: &str,
    _elder: &Elder,
    session_id: Uuid,
    dynamic_prompt: String,
    tool_ctx: ToolContext,
) {
    if state.config.gemini_api_key.is_empty() {
        tracing::error!("GEMINI_API_KEY/GOOGLE_API_KEY not set; cannot start Gemini Live session");
        state.call_state.end_call(call_sid, true);
        return;
    }

    // Track stream SID for sending audio back
    let stream_sid = Arc::new(tokio::sync::RwLock::new(String::new()));
    let stream_sid_clone = stream_sid.clone();

    // Shared transcript accumulator (optional, only for DB transcript field)
    let transcript = Arc::new(tokio::sync::Mutex::new(String::new()));
    let transcript_clone = transcript.clone();

    // Channels for audio flow
    let (twilio_tx, mut twilio_rx) = mpsc::channel::<String>(200);
    let (gemini_audio_tx, mut gemini_audio_rx) = mpsc::channel::<String>(200);
    let (ws_out_tx, mut ws_out_rx) = mpsc::channel::<String>(200);

    // Outbound audio task: send Twilio outbound media frames
    let stream_sid_for_sender = stream_sid.clone();
    tokio::spawn(async move {
        while let Some(audio_base64) = twilio_rx.recv().await {
            let sid = stream_sid_for_sender.read().await.clone();
            if !sid.is_empty() {
                let msg = TwilioOutboundMedia::new(&sid, &audio_base64);
                if let Ok(json) = serde_json::to_string(&msg) {
                    if ws_out_tx.send(json).await.is_err() {
                        break;
                    }
                }
            }
        }
    });

    // Gemini session task
    let state_clone = state.clone();
    let call_sid_for_gemini = call_sid.to_string();
    let tool_ctx_clone = tool_ctx.clone();
    tokio::spawn(async move {
        use crate::clients::gemini_live::FunctionResponse;

        let client = state_clone.gemini.clone();
        let mut resume_handle: Option<String> = None;
        let mut greeted = false;
        let mut pending_greeting = false;

        // Tool declarations come from our MCP registry.
        let tools: Vec<crate::mcp::ToolDefinition> = crate::mcp::ToolRegistry::list_tools();

        loop {
            let connect_result = client
                .connect_live(dynamic_prompt.clone(), tools.clone(), resume_handle.clone())
                .await;

            let mut session = match connect_result {
                Ok(s) => s,
                Err(e) => {
                    tracing::error!("Failed to connect to Gemini Live: {}", e);
                    state_clone.call_state.end_call(&call_sid_for_gemini, true);
                    return;
                }
            };

            // Now that we're connected to the model, mark call as active (connected to AI).
            state_clone.call_state.set_active(&call_sid_for_gemini);
            let mut setup_complete = false;
            let mut pending_audio_ulaw: VecDeque<String> = VecDeque::with_capacity(200);

            // Main loop: multiplex inbound twilio-audio -> gemini, and gemini events -> twilio/transcript.
            loop {
                tokio::select! {
                    Some(audio_b64_ulaw) = gemini_audio_rx.recv() => {
                        // Special control message: greeting trigger (sent by outer twilio start handler)
                        if audio_b64_ulaw == "__GREETING__" {
                            pending_greeting = true;
                            continue;
                        }

                        // IMPORTANT: per Live API, do not send any messages until setup_complete arrives.
                        // If we send audio too early, Gemini may ignore it and we'll get silence/no transcripts.
                        if !setup_complete {
                            if pending_audio_ulaw.len() >= 200 {
                                pending_audio_ulaw.pop_front();
                            }
                            pending_audio_ulaw.push_back(audio_b64_ulaw);
                            continue;
                        }

                        // Flush greeting if requested and not yet greeted.
                        if pending_greeting && !greeted {
                            greeted = true;
                            pending_greeting = false;
                            let _ = session.send_client_text_turn("hola", true).await;
                        }

                        // Normal audio path
                        match twilio_ulaw_base64_to_gemini_pcm16_16khz_bytes(&audio_b64_ulaw) {
                            Ok(pcm16_16k) => {
                                if let Err(e) = session.send_realtime_audio_pcm16_16khz(&pcm16_16k).await {
                                    tracing::error!("Failed to send audio to Gemini: {}", e);
                                    break;
                                }
                            }
                            Err(e) => tracing::warn!("Audio transcode error (twilio->gemini): {}", e),
                        }
                    }
                    Some(msg) = session.recv_event() => {
                        if msg.setup_complete.is_some() && !setup_complete {
                            setup_complete = true;
                            tracing::info!("Gemini Live setup_complete received");

                            // Flush any buffered audio now that setup is live
                            while let Some(ulaw_b64) = pending_audio_ulaw.pop_front() {
                                if let Ok(pcm16_16k) = twilio_ulaw_base64_to_gemini_pcm16_16khz_bytes(&ulaw_b64) {
                                    let _ = session.send_realtime_audio_pcm16_16khz(&pcm16_16k).await;
                                }
                            }

                            // If the stream already started, do greeting now.
                            if pending_greeting && !greeted {
                                greeted = true;
                                pending_greeting = false;
                                let _ = session.send_client_text_turn("hola", true).await;
                            }
                        }

                        if let Some(err) = msg.error.as_ref() {
                            tracing::error!(error = %err, "Gemini Live server error");
                        }

                        // Resumption handle tracking
                        if let Some(update) = msg.session_resumption_update {
                            if update.resumable.unwrap_or(false) {
                                if let Some(h) = update.new_handle {
                                    resume_handle = Some(h);
                                }
                            }
                        }

                        // GoAway: reconnect using last resumable handle
                        if msg.go_away.is_some() {
                            tracing::info!("Gemini Live GoAway received; reconnecting using session resumption");
                            break;
                        }

                        // Audio + transcripts
                        if let Some(content) = msg.server_content {
                            // Input transcription (user)
                            if let Some(t) = content.input_transcription {
                                tracing::debug!(text = %t.text, "Gemini input transcription");
                                state_clone.call_state.add_transcript(
                                    &call_sid_for_gemini,
                                    Speaker::User,
                                    t.text.clone(),
                                    false,
                                );
                            }
                            // Output transcription (assistant)
                            if let Some(t) = content.output_transcription {
                                tracing::debug!(text = %t.text, "Gemini output transcription");
                                {
                                    let mut acc = transcript_clone.lock().await;
                                    acc.push_str(&t.text);
                                    acc.push(' ');
                                }
                                state_clone.call_state.add_transcript(
                                    &call_sid_for_gemini,
                                    Speaker::Assistant,
                                    t.text,
                                    false,
                                );
                            }

                            // Audio chunks
                            if let Some(model_turn) = content.model_turn {
                                for part in model_turn.parts {
                                    if let Some(inline) = part.inline_data {
                                        // Only handle audio payloads for now.
                                        if inline.mime_type.starts_with("audio/pcm") {
                                            let pcm24 = match base64::engine::general_purpose::STANDARD.decode(inline.data.as_bytes()) {
                                                Ok(b) => b,
                                                Err(_) => continue,
                                            };
                                            match gemini_pcm16_24khz_bytes_to_twilio_ulaw_base64(&pcm24) {
                                                Ok(twilio_b64) => {
                                                    let _ = twilio_tx.send(twilio_b64).await;
                                                }
                                                Err(e) => tracing::warn!("Audio transcode error (gemini->twilio): {}", e),
                                            }
                                        }
                                    }
                                }
                            }
                        }

                        // Tool calls will be wired in the next todo; for now we must respond with empty responses if they occur.
                        if let Some(tool_call) = msg.tool_call {
                            if !tool_call.function_calls.is_empty() {
                                let mut function_responses: Vec<FunctionResponse> = Vec::with_capacity(tool_call.function_calls.len());

                                for fc in tool_call.function_calls {
                                    let args_str = serde_json::to_string(&fc.args).unwrap_or_default();
                                    state_clone.call_state.add_tool_call(
                                        &call_sid_for_gemini,
                                        fc.id.clone(),
                                        fc.name.clone(),
                                        args_str.clone(),
                                    );

                                    // Execute via MCP (local tool runtime)
                                    let result = crate::mcp::execute_tool(&tool_ctx_clone, &fc.name, fc.args).await;
                                    let _ = CallService::record_tool_usage(&state_clone.db, session_id, &fc.name).await;

                                    // Convert MCP result to JSON value
                                    let result_json: serde_json::Value = if let Some(text_content) = result.content.first() {
                                        match text_content {
                                            crate::mcp::ToolResultContent::Text { text } => {
                                                serde_json::from_str(text).unwrap_or(json!({ "result": text }))
                                            }
                                            _ => json!({ "error": "unexpected_result_type" }),
                                        }
                                    } else {
                                        json!({ "error": "no_result" })
                                    };

                                    let result_str = serde_json::to_string(&result_json).unwrap_or_default();
                                    state_clone.call_state.complete_tool_call(
                                        &call_sid_for_gemini,
                                        &fc.id,
                                        result_str,
                                    );

                                    function_responses.push(FunctionResponse {
                                        id: fc.id,
                                        name: fc.name,
                                        response: result_json,
                                    });
                                }

                                let _ = session.send_tool_responses(function_responses).await;
                            }
                        }

                        if let Some(cancel) = msg.tool_call_cancellation {
                            for id in cancel.ids {
                                state_clone.call_state.complete_tool_call(
                                    &call_sid_for_gemini,
                                    &id,
                                    "{\"error\":\"tool_call_cancelled\"}".to_string(),
                                );
                            }
                        }
                    }
                    else => break,
                }
            }

            // If we don't have a resumable handle, don't spin forever.
            if resume_handle.is_none() {
                tracing::warn!("Gemini Live session ended and no resumable handle available; stopping");
                break;
            }
            // Otherwise loop to reconnect.
            tracing::info!("Reconnecting Gemini Live session...");
        }

        // End session in DB with transcript if any
        let final_transcript = {
            let t = transcript_clone.lock().await;
            if t.is_empty() { None } else { Some(t.clone()) }
        };
        let _ = CallService::end_session(
            &state_clone.db,
            session_id,
            CallStatus::Completed,
            None,
            final_transcript,
        ).await;
    });

    // Twilio websocket loop (same shape as other providers)
    loop {
        tokio::select! {
            Some(json) = ws_out_rx.recv() => {
                if ws_sender.send(axum::extract::ws::Message::Text(json)).await.is_err() {
                    break;
                }
            }
            msg = ws_receiver.next() => {
                match msg {
                    Some(Ok(axum::extract::ws::Message::Text(text))) => {
                        if let Ok(twilio_msg) = serde_json::from_str::<TwilioStreamMessage>(&text) {
                            match twilio_msg {
                                TwilioStreamMessage::Start { stream_sid: sid, start } => {
                                    *stream_sid_clone.write().await = sid;
                                    tracing::info!("Stream started for call {}", start.call_sid);

                                    // send a greeting trigger to gemini after a short delay
                                    let greeting_tx = gemini_audio_tx.clone();
                                    tokio::spawn(async move {
                                        tokio::time::sleep(tokio::time::Duration::from_millis(400)).await;
                                        let _ = greeting_tx.send("__GREETING__".to_string()).await;
                                    });
                                }
                                TwilioStreamMessage::Media { media, .. } => {
                                    let _ = gemini_audio_tx.send(media.payload.clone()).await;
                                }
                                TwilioStreamMessage::Stop { .. } => {
                                    tracing::info!("Stream stopped");
                                    break;
                                }
                                _ => {}
                            }
                        }
                    }
                    Some(Ok(axum::extract::ws::Message::Close(_))) => {
                        tracing::info!("WebSocket closed");
                        break;
                    }
                    Some(Err(e)) => {
                        tracing::error!("WebSocket error: {}", e);
                        break;
                    }
                    None => break,
                    _ => {}
                }
            }
        }
    }
}

/// Handle OpenAI Realtime voice stream
async fn handle_openai_stream(
    state: &AppState,
    ws_sender: &mut futures_util::stream::SplitSink<axum::extract::ws::WebSocket, axum::extract::ws::Message>,
    ws_receiver: &mut futures_util::stream::SplitStream<axum::extract::ws::WebSocket>,
    call_sid: &str,
    _elder: &Elder,
    session_id: Uuid,
    dynamic_prompt: String,
    tool_ctx: ToolContext,
) {
    // Connect to OpenAI Realtime
    let openai_session = match state.openai.connect_realtime(&dynamic_prompt, build_assistant_tools()).await {
        Ok(s) => s,
        Err(e) => {
            tracing::error!("Failed to connect to OpenAI Realtime: {}", e);
            state.call_state.end_call(call_sid, true);
            return;
        }
    };
    
    // Mark call as active
    state.call_state.set_active(call_sid);
    
    // Track stream SID for sending audio back
    let stream_sid = Arc::new(tokio::sync::RwLock::new(String::new()));
    let stream_sid_clone = stream_sid.clone();
    
    // Shared transcript accumulator
    let transcript = Arc::new(tokio::sync::Mutex::new(String::new()));
    let transcript_clone = transcript.clone();
    
    // Channel for sending audio to Twilio
    let (twilio_tx, mut twilio_rx) = mpsc::channel::<String>(100);
    
    // Channel for sending audio to OpenAI
    let (openai_audio_tx, mut openai_audio_rx) = mpsc::channel::<String>(100);
    
    // Take ownership of ws_sender for the outbound task
    // We need to use channels since we can't clone ws_sender
    let (ws_out_tx, mut ws_out_rx) = mpsc::channel::<String>(100);
    
    let stream_sid_for_sender = stream_sid.clone();
    tokio::spawn(async move {
        while let Some(audio_base64) = twilio_rx.recv().await {
            let sid = stream_sid_for_sender.read().await.clone();
            if !sid.is_empty() {
                let msg = TwilioOutboundMedia::new(&sid, &audio_base64);
                if let Ok(json) = serde_json::to_string(&msg) {
                    if ws_out_tx.send(json).await.is_err() {
                        break;
                    }
                }
            }
        }
    });
    
    // Clone state for the OpenAI handler
    let state_clone = state.clone();
    let call_sid_for_openai = call_sid.to_string();
    let tool_ctx_clone = tool_ctx.clone();
    
    // Spawn task for OpenAI communication
    let mut openai_session = openai_session;
    let twilio_tx_clone = twilio_tx.clone();
    tokio::spawn(async move {
        let mut current_assistant_text = String::new();
        
        loop {
            tokio::select! {
                Some(audio) = openai_audio_rx.recv() => {
                    if audio == "__GREETING__" {
                        tracing::info!("Triggering initial greeting");
                        if let Err(e) = openai_session.trigger_initial_greeting().await {
                            tracing::error!("Failed to trigger greeting: {}", e);
                        }
                        continue;
                    }
                    
                    if let Err(e) = openai_session.send_audio(&audio).await {
                        tracing::error!("Failed to send audio to OpenAI: {}", e);
                        break;
                    }
                }
                Some(event) = openai_session.recv_event() => {
                    match event {
                        RealtimeServerEvent::ResponseAudioDelta { delta } => {
                            let _ = twilio_tx_clone.send(delta).await;
                        }
                        RealtimeServerEvent::ResponseAudioTranscriptDelta { delta } => {
                            {
                                let mut t = transcript_clone.lock().await;
                                t.push_str(&delta);
                            }
                            current_assistant_text.push_str(&delta);
                            state_clone.call_state.add_transcript(
                                &call_sid_for_openai,
                                Speaker::Assistant,
                                current_assistant_text.clone(),
                                true,
                            );
                        }
                        RealtimeServerEvent::ResponseAudioTranscriptDone { transcript: full_text } => {
                            state_clone.call_state.add_transcript(
                                &call_sid_for_openai,
                                Speaker::Assistant,
                                full_text,
                                false,
                            );
                            current_assistant_text.clear();
                        }
                        RealtimeServerEvent::ConversationItemInputAudioTranscriptionCompleted { transcript: user_text } => {
                            state_clone.call_state.add_transcript(
                                &call_sid_for_openai,
                                Speaker::User,
                                user_text,
                                false,
                            );
                        }
                        RealtimeServerEvent::ResponseFunctionCallArgumentsDone { call_id, name, arguments } => {
                            tracing::info!("Function call: {} with args: {}", name, arguments);
                            
                            state_clone.call_state.add_tool_call(
                                &call_sid_for_openai,
                                call_id.clone(),
                                name.clone(),
                                arguments.clone(),
                            );
                            
                            // Execute via MCP tools
                            let args: serde_json::Value = serde_json::from_str(&arguments).unwrap_or_default();
                            let result = crate::mcp::execute_tool(&tool_ctx_clone, &name, args).await;
                            
                            let _ = CallService::record_tool_usage(&state_clone.db, session_id, &name).await;
                            
                            // Convert MCP result to JSON value
                            let result_json: serde_json::Value = if let Some(text_content) = result.content.first() {
                                match text_content {
                                    crate::mcp::ToolResultContent::Text { text } => {
                                        serde_json::from_str(text).unwrap_or(json!({ "result": text }))
                                    }
                                    _ => json!({ "error": "Unexpected result type" }),
                                }
                            } else {
                                json!({ "error": "No result" })
                            };
                            
                            let result_str = serde_json::to_string(&result_json).unwrap_or_default();
                            state_clone.call_state.complete_tool_call(
                                &call_sid_for_openai,
                                &call_id,
                                result_str,
                            );
                            
                            if let Err(e) = openai_session.send_function_result(&call_id, result_json).await {
                                tracing::error!("Failed to send function result: {}", e);
                            }
                        }
                        RealtimeServerEvent::Error { error } => {
                            tracing::error!("OpenAI error: {} - {}", error.r#type, error.message);
                        }
                        _ => {}
                    }
                }
                else => break,
            }
        }
    });
    
    // Handle Twilio messages and outbound audio
    loop {
        tokio::select! {
            Some(json) = ws_out_rx.recv() => {
                if ws_sender.send(axum::extract::ws::Message::Text(json)).await.is_err() {
                    break;
                }
            }
            msg = ws_receiver.next() => {
        match msg {
                    Some(Ok(axum::extract::ws::Message::Text(text))) => {
                if let Ok(twilio_msg) = serde_json::from_str::<TwilioStreamMessage>(&text) {
                    match twilio_msg {
                        TwilioStreamMessage::Start { stream_sid: sid, start } => {
                            *stream_sid_clone.write().await = sid;
                            tracing::info!("Stream started for call {}", start.call_sid);
                            
                            let greeting_tx = openai_audio_tx.clone();
                            tokio::spawn(async move {
                                tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
                                let _ = greeting_tx.send("__GREETING__".to_string()).await;
                            });
                        }
                        TwilioStreamMessage::Media { media, .. } => {
                            let _ = openai_audio_tx.send(media.payload.clone()).await;
                        }
                        TwilioStreamMessage::Stop { .. } => {
                            tracing::info!("Stream stopped");
                            break;
                        }
                        _ => {}
                    }
                }
            }
                    Some(Ok(axum::extract::ws::Message::Close(_))) => {
                tracing::info!("WebSocket closed");
                break;
            }
                    Some(Err(e)) => {
                tracing::error!("WebSocket error: {}", e);
                break;
            }
                    None => break,
            _ => {}
                }
            }
        }
    }
    
    // Get transcript and end session
    let final_transcript = {
        let t = transcript.lock().await;
        if t.is_empty() { None } else { Some(t.clone()) }
    };
    
    let _ = CallService::end_session(
        &state.db,
        session_id,
        CallStatus::Completed,
        None,
        final_transcript,
    ).await;
}

/// Handle ElevenLabs Conversational AI voice stream
async fn handle_elevenlabs_stream(
    state: &AppState,
    ws_sender: &mut futures_util::stream::SplitSink<axum::extract::ws::WebSocket, axum::extract::ws::Message>,
    ws_receiver: &mut futures_util::stream::SplitStream<axum::extract::ws::WebSocket>,
    call_sid: &str,
    elder: &Elder,
    session_id: Uuid,
    dynamic_prompt: String,
    tool_ctx: ToolContext,
) {
    // Build agent config with dynamic prompt
    let full_prompt = format!("{}\n\n---\n\n{}", ELEVENLABS_SYSTEM_PROMPT, dynamic_prompt);
    let first_message = Some(format!(
        "Hola {}. Soy Domingo, tu asistente de voz. ¿En qué puedo ayudarte hoy?",
        elder.name
    ));
    
    let agent_config = AgentConfig {
        system_prompt: full_prompt,
        first_message,
        tools: build_elevenlabs_tools(),
    };
    
    // Connect to ElevenLabs
    let elevenlabs_session = match state.elevenlabs.connect_conversation(agent_config).await {
        Ok(s) => s,
        Err(e) => {
            tracing::error!("Failed to connect to ElevenLabs: {}", e);
            state.call_state.end_call(call_sid, true);
            return;
        }
    };
    
    // Mark call as active
    state.call_state.set_active(call_sid);
    
    // Track stream SID
    let stream_sid = Arc::new(tokio::sync::RwLock::new(String::new()));
    let stream_sid_clone = stream_sid.clone();
    
    // Shared transcript
    let transcript = Arc::new(tokio::sync::Mutex::new(String::new()));
    let transcript_clone = transcript.clone();
    
    // Channels
    let (twilio_tx, mut twilio_rx) = mpsc::channel::<String>(100);
    let (elevenlabs_audio_tx, mut elevenlabs_audio_rx) = mpsc::channel::<String>(100);
    let (ws_out_tx, mut ws_out_rx) = mpsc::channel::<String>(100);
    
    // Outbound audio task
    let stream_sid_for_sender = stream_sid.clone();
    tokio::spawn(async move {
        while let Some(audio_base64) = twilio_rx.recv().await {
            let sid = stream_sid_for_sender.read().await.clone();
            if !sid.is_empty() {
                // Note: ElevenLabs returns PCM audio which may need conversion to g711_ulaw for Twilio
                // For now, we send directly and may need to add audio conversion
                let msg = TwilioOutboundMedia::new(&sid, &audio_base64);
                if let Ok(json) = serde_json::to_string(&msg) {
                    if ws_out_tx.send(json).await.is_err() {
                        break;
                    }
                }
            }
        }
    });
    
    // Clone state for ElevenLabs handler
    let state_clone = state.clone();
    let call_sid_for_eleven = call_sid.to_string();
    let tool_ctx_clone = tool_ctx.clone();
    
    // ElevenLabs event handler
    let mut elevenlabs_session = elevenlabs_session;
    let twilio_tx_clone = twilio_tx.clone();
    tokio::spawn(async move {
        loop {
            tokio::select! {
                Some(audio) = elevenlabs_audio_rx.recv() => {
                    // Note: May need to convert from g711_ulaw to PCM16 for ElevenLabs
                    if let Err(e) = elevenlabs_session.send_audio(&audio).await {
                        tracing::error!("Failed to send audio to ElevenLabs: {}", e);
                        break;
                    }
                }
                event = elevenlabs_session.recv_event() => {
                    match event {
                        Some(ElevenLabsServerMessage::Audio { audio }) => {
                            let _ = twilio_tx_clone.send(audio).await;
                        }
                        Some(ElevenLabsServerMessage::AgentTranscript { text }) => {
                            {
                                let mut t = transcript_clone.lock().await;
                                t.push_str(&text);
                                t.push(' ');
                            }
                            state_clone.call_state.add_transcript(
                                &call_sid_for_eleven,
                                Speaker::Assistant,
                                text,
                                false,
                            );
                        }
                        Some(ElevenLabsServerMessage::UserTranscript { text }) => {
                            state_clone.call_state.add_transcript(
                                &call_sid_for_eleven,
                                Speaker::User,
                                text,
                                false,
                            );
                        }
                        Some(ElevenLabsServerMessage::ClientToolCall { tool_call_id, tool_name, parameters }) => {
                            tracing::info!("ElevenLabs tool call: {} with params: {:?}", tool_name, parameters);
                            
                            state_clone.call_state.add_tool_call(
                                &call_sid_for_eleven,
                                tool_call_id.clone(),
                                tool_name.clone(),
                                serde_json::to_string(&parameters).unwrap_or_default(),
                            );
                            
                            // Execute via MCP
                            let result = crate::mcp::execute_tool(&tool_ctx_clone, &tool_name, parameters).await;
                            
                            let _ = CallService::record_tool_usage(&state_clone.db, session_id, &tool_name).await;
                            
                            // Convert result to JSON
                            let result_json: serde_json::Value = if let Some(text_content) = result.content.first() {
                                match text_content {
                                    crate::mcp::ToolResultContent::Text { text } => {
                                        serde_json::from_str(text).unwrap_or(json!({ "result": text }))
                                    }
                                    _ => json!({ "error": "Unexpected result type" }),
                                }
                            } else {
                                json!({ "error": "No result" })
                            };
                            
                            let result_str = serde_json::to_string(&result_json).unwrap_or_default();
                            state_clone.call_state.complete_tool_call(
                                &call_sid_for_eleven,
                                &tool_call_id,
                                result_str,
                            );
                            
                            // Send result back to ElevenLabs
                            if let Err(e) = elevenlabs_session.send_tool_result(&tool_call_id, result_json).await {
                                tracing::error!("Failed to send tool result to ElevenLabs: {}", e);
                            }
                        }
                        Some(ElevenLabsServerMessage::Error { message, code }) => {
                            tracing::error!("ElevenLabs error: {} (code: {:?})", message, code);
                        }
                        None => break,
                        _ => {}
                    }
                }
            }
        }
    });
    
    // Handle Twilio messages
    loop {
        tokio::select! {
            Some(json) = ws_out_rx.recv() => {
                if ws_sender.send(axum::extract::ws::Message::Text(json)).await.is_err() {
                    break;
                }
            }
            msg = ws_receiver.next() => {
                match msg {
                    Some(Ok(axum::extract::ws::Message::Text(text))) => {
                        if let Ok(twilio_msg) = serde_json::from_str::<TwilioStreamMessage>(&text) {
                            match twilio_msg {
                                TwilioStreamMessage::Start { stream_sid: sid, start } => {
                                    *stream_sid_clone.write().await = sid;
                                    tracing::info!("Stream started for call {}", start.call_sid);
                                }
                                TwilioStreamMessage::Media { media, .. } => {
                                    let _ = elevenlabs_audio_tx.send(media.payload.clone()).await;
                                }
                                TwilioStreamMessage::Stop { .. } => {
                                    tracing::info!("Stream stopped");
                                    break;
                                }
                                _ => {}
                            }
                        }
                    }
                    Some(Ok(axum::extract::ws::Message::Close(_))) => {
                        tracing::info!("WebSocket closed");
                        break;
                    }
                    Some(Err(e)) => {
                        tracing::error!("WebSocket error: {}", e);
                        break;
                    }
                    None => break,
                    _ => {}
                }
            }
        }
    }
    
    // End session
    let final_transcript = {
        let t = transcript.lock().await;
        if t.is_empty() { None } else { Some(t.clone()) }
    };
    
    let _ = CallService::end_session(
        &state.db,
        session_id,
        CallStatus::Completed,
        None,
        final_transcript,
    ).await;
}

/// Build a dynamic system prompt that includes the elder's specific context
async fn build_dynamic_prompt(state: &AppState, elder: &Elder) -> String {
    let mut context_parts = vec![];
    
    // Add elder's name for personalization
    context_parts.push(format!(
        "## Información del Usuario\nEstás hablando con **{}**.",
        elder.name
    ));
    
    // Fetch caregiver relationship notes if available
    if let Ok(Some(relationship)) = CaregiverRepository::get_relationship(
        &state.db, elder.caregiver_id, elder.id
    ).await {
        let mut rel_info = format!(
            "## Contexto del Cuidador\nEl cuidador principal es su **{}**.",
            relationship.relationship
        );
        if let Some(notes) = relationship.notes {
            if !notes.is_empty() {
                rel_info.push_str(&format!("\nNotas importantes: {}", notes));
            }
        }
        context_parts.push(rel_info);
    }
    
    // Fetch and add contacts
    if let Ok(contacts) = ContactService::get_all_contacts(&state.db, elder.id).await {
        if !contacts.is_empty() {
            let mut contact_list = String::from("## Contactos Guardados\nPara transferir una llamada, usa call_contact con el nombre EXACTO:\n");
            for c in &contacts {
                let emergency = if c.is_emergency { " ⚠️ EMERGENCIA" } else { "" };
                let notes = c.notes.as_deref().map(|n| format!(" - {}", n)).unwrap_or_default();
                contact_list.push_str(&format!(
                    "- **{}** ({}){}:{}\n",
                    c.name, c.relationship, emergency, notes
                ));
            }
            context_parts.push(contact_list);
        }
    }
    
    // Fetch and add locations (separated by type)
    if let Ok(locations) = LocationService::get_all_locations(&state.db, elder.id).await {
        if !locations.is_empty() {
            // Destinations (places to go)
            let destinations: Vec<_> = locations.iter()
                .filter(|l| l.location_type == LocationType::Destination)
                .collect();
            if !destinations.is_empty() {
                let mut dest_list = String::from("## Destinos (lugares a donde puede ir)\n");
                for l in destinations {
                    let home = if l.is_home { " (CASA)" } else { "" };
                    let tags = if !l.tags.is_empty() { 
                        format!(" [{}]", l.tags.join(", "))
                    } else { 
                        String::new() 
                    };
                    dest_list.push_str(&format!(
                        "- **{}**{}: {}{}\n",
                        l.name, home, l.address, tags
                    ));
                }
                context_parts.push(dest_list);
            }
            
            // Common spots (places where they might be)
            let common_spots: Vec<_> = locations.iter()
                .filter(|l| l.location_type == LocationType::CommonSpot)
                .collect();
            if !common_spots.is_empty() {
                let mut spots_list = String::from("## Lugares Frecuentes (donde podría estar ahora)\n");
                spots_list.push_str("Cuando pida un Uber, pregunta si está en uno de estos lugares:\n");
                for l in common_spots {
                    let tags = if !l.tags.is_empty() { 
                        format!(" [{}]", l.tags.join(", "))
                    } else { 
                        String::new() 
                    };
                    spots_list.push_str(&format!(
                        "- **{}**: {}{}\n",
                        l.name, l.address, tags
                    ));
                }
                context_parts.push(spots_list);
            }
        }
    }
    
    // Fetch and add medications - FULL details in context (no tool needed)
    if let Ok(meds) = MedicationService::get_all_medications(&state.db, elder.id).await {
        if !meds.is_empty() {
            let mut med_list = String::from("## Medicamentos (Información Completa)\n");
            med_list.push_str("IMPORTANTE: Usa SOLO esta información para responder preguntas sobre medicamentos. NO inventes información.\n\n");
            
            for m in &meds {
                med_list.push_str(&format!("### {}\n", m.medication.name));
                med_list.push_str(&format!("- **Dosis**: {}\n", m.medication.dosage));
                
                if let Some(instructions) = &m.medication.instructions {
                    if !instructions.is_empty() {
                        med_list.push_str(&format!("- **Instrucciones**: {}\n", instructions));
                    }
                }
                
                if !m.schedules.is_empty() {
                    let days = m.schedules.first().map(|s| s.days_description()).unwrap_or_default();
                    let times: Vec<String> = m.schedules.iter()
                        .map(|s| s.time_of_day.format("%H:%M").to_string())
                        .collect();
                    med_list.push_str(&format!("- **Cuándo tomarlo**: {} a las {}\n", days, times.join(", ")));
                }
                med_list.push('\n');
            }
            context_parts.push(med_list);
        } else {
            context_parts.push("## Medicamentos\nNo hay medicamentos registrados.".to_string());
        }
    }
    
    context_parts.join("\n\n")
}
