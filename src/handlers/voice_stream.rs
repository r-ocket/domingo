//! WebSocket handler for Twilio media stream - supports OpenAI Realtime and ElevenLabs

use std::sync::Arc;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::atomic::AtomicU64;
use axum::{
    extract::{Path, State, WebSocketUpgrade},
    response::IntoResponse,
};
use futures_util::{SinkExt, StreamExt};
use serde_json::json;
use tokio::sync::mpsc;
use tokio::sync::watch;
use uuid::Uuid;
use base64::Engine as _;
use chrono::{Datelike, FixedOffset, Utc, Weekday};

use crate::clients::{
    TwilioStreamMessage, TwilioOutboundMedia, TwilioOutboundClear,
    RealtimeServerEvent, build_assistant_tools,
    AgentConfig, ServerMessage as ElevenLabsServerMessage,
    build_elevenlabs_tools, ELEVENLABS_SYSTEM_PROMPT,
    gemini_live::GeminiLiveSetupOverrides,
    audio::{
        twilio_ulaw_base64_to_gemini_pcm16_16khz_bytes,
        gemini_pcm16_24khz_bytes_to_twilio_ulaw_base64,
        ulaw_to_pcm16,
        GeminiAudioBatcher,
    },
    XaiSessionUpdate,
    XaiSessionConfig,
    XaiTurnDetection,
    XaiAudioConfig,
    XaiAudioSide,
    XaiAudioFormat,
    XaiToolConfig,
    XaiServerEvent,
};
use crate::domain::{CallStatus, Elder, LocationType, VoiceProvider};
use crate::mcp::ToolContext;
use crate::repositories::postgres::CaregiverRepository;
use crate::services::{
    CallService, ContactService, ElderService, LocationService,
    MedicationService, Speaker,
    CallAudioRecorder,
};
use crate::AppState;

// debug mode: keep the realtime path as clean as possible.
// set to true if/when you want call recordings + s3 upload back.
const ENABLE_CALL_RECORDING: bool = false;

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

    // External shutdown (e.g., Twilio call status webhook says the call completed).
    // If triggered, we force-stop the media stream even if Twilio's websocket doesn't close cleanly.
    let external_shutdown_rx = state.call_state.register_shutdown(&call_sid);
    
    // Build dynamic context with elder's data
    let dynamic_prompt = build_dynamic_prompt(&state, &elder).await;

    // dump dynamic context for debugging (this is the ground truth for meds/contacts/locations).
    // WARNING: may include sensitive addresses/notes; keep at debug level.
    tracing::info!(
        elder_id = %elder.id,
        dynamic_context_len = dynamic_prompt.len(),
        "voice: built dynamic context"
    );
    tracing::debug!(
        elder_id = %elder.id,
        dynamic_context = %dynamic_prompt,
        "voice: dynamic context dump"
    );

    // Provider-specific prompt packing.
    //
    // NOTE: some realtime models appear to truncate long instructions; we prefer losing
    // generic behavioral prompt content over losing per-elder facts (medications/contacts/etc).
    //
    // - gemini: keep the existing ordering (system -> context)
    // - xai: put context first, then system prompt
    let mut gemini_prompt = format!("{}\n\n---\n\n{}", crate::clients::SYSTEM_PROMPT, dynamic_prompt);
    let mut xai_prompt = format!(
        "## idioma\nSIEMPRE responde en español (es-MX).\n\n---\n\n{}\n\n---\n\n{}",
        dynamic_prompt,
        crate::clients::SYSTEM_PROMPT
    );

    tracing::info!(
        elder_id = %elder.id,
        xai_instructions_len = xai_prompt.len(),
        gemini_instructions_len = gemini_prompt.len(),
        "voice: packed provider instructions"
    );
    tracing::debug!(
        elder_id = %elder.id,
        xai_instructions = %xai_prompt,
        "voice: xai session.update.instructions dump"
    );

    // Per-call overrides (from call_sessions.metadata, set by the call center debug UI).
    let mut gemini_overrides = GeminiLiveSetupOverrides::default();
    let mut xai_voice_override: Option<String> = None;
    if let Some(meta) = session.metadata.as_ref() {
        if let Some(p) = meta.get("prompt_override").and_then(|v| v.as_str()) {
            let p = p.trim();
            if !p.is_empty() {
                for prompt in [&mut gemini_prompt, &mut xai_prompt] {
                    prompt.push_str("\n\n---\n\n## instrucciones extra (solo para esta llamada)\n");
                    prompt.push_str(p);
                }
            }
        }

        if let Some(gemini) = meta.get("gemini").and_then(|v| v.as_object()) {
            if let Some(gc) = gemini.get("generationConfig") {
                if !gc.is_null() {
                    gemini_overrides.generation_config = Some(gc.clone());
                }
            }
            if let Some(ric) = gemini.get("realtimeInputConfig") {
                if !ric.is_null() {
                    gemini_overrides.realtime_input_config = Some(ric.clone());
                }
            }
            if let Some(tc) = gemini.get("transcriptionConfig") {
                if !tc.is_null() {
                    gemini_overrides.input_audio_transcription = Some(tc.clone());
                    gemini_overrides.output_audio_transcription = Some(tc.clone());
                }
            }
        }

        if let Some(xai) = meta.get("xai").and_then(|v| v.as_object()) {
            if let Some(v) = xai.get("voice").and_then(|v| v.as_str()) {
                let v = v.trim();
                if !v.is_empty() {
                    xai_voice_override = Some(v.to_string());
                }
            }
        }
    }
    
    // Create tool context for MCP
    let tool_ctx = ToolContext {
        db: state.db.clone(),
        twilio: state.twilio.clone(),
        uber: state.uber.clone(),
        elder_id: elder.id,
        session_id: session.id,
        call_sid: call_sid.clone(),
    };
    
    match session.voice_provider {
        VoiceProvider::XaiGrok => {
            handle_xai_stream(
                &state,
                &mut ws_sender,
                &mut ws_receiver,
                &call_sid,
                &elder,
                session.id,
                xai_prompt,
                tool_ctx,
                xai_voice_override,
                external_shutdown_rx,
            )
            .await;
        }
        VoiceProvider::GeminiLive => {
            handle_gemini_stream(
                &state,
                &mut ws_sender,
                &mut ws_receiver,
                &call_sid,
                &elder,
                session.id,
                gemini_prompt,
                tool_ctx,
                gemini_overrides,
                external_shutdown_rx,
            )
            .await;
        }
        other => {
            tracing::warn!(
                requested = %other.to_string(),
                "voice provider coerced to xai_grok (xai-only runtime)"
            );
            handle_xai_stream(
                &state,
                &mut ws_sender,
                &mut ws_receiver,
                &call_sid,
                &elder,
                session.id,
                xai_prompt,
                tool_ctx,
                xai_voice_override,
                external_shutdown_rx,
            )
            .await;
        }
    }
    
    // End call in live state store
    state.call_state.end_call(&call_sid, false);
    
    tracing::info!("Voice session ended for call {}", call_sid);
}

/// Handle xAI Grok Voice Agent voice stream (Realtime over WebSockets)
async fn handle_xai_stream(
    state: &AppState,
    ws_sender: &mut futures_util::stream::SplitSink<axum::extract::ws::WebSocket, axum::extract::ws::Message>,
    ws_receiver: &mut futures_util::stream::SplitStream<axum::extract::ws::WebSocket>,
    call_sid: &str,
    _elder: &Elder,
    session_id: Uuid,
    system_prompt: String,
    tool_ctx: ToolContext,
    voice_override: Option<String>,
    mut external_shutdown_rx: watch::Receiver<bool>,
) {
    if state.config.xai_api_key.is_empty() {
        tracing::error!("XAI_API_KEY not set; cannot start xai grok voice session");
        state.call_state.end_call(call_sid, true);
        return;
    }

    // Track stream SID for sending audio back
    let stream_sid = Arc::new(tokio::sync::RwLock::new(String::new()));
    let stream_sid_clone = stream_sid.clone();

    // Shared transcript accumulator (optional, only for DB transcript field)
    let transcript = Arc::new(tokio::sync::Mutex::new(String::new()));
    let transcript_clone = transcript.clone();

    // Call recording worker: records both tracks while the call is running, then uploads to S3 after call end.
    #[derive(Debug)]
    enum RecEvt {
        UserUlawB64(String),
        AssistantUlawB64(String),
        End,
    }

    let rec_tx: Option<mpsc::Sender<RecEvt>> = if ENABLE_CALL_RECORDING {
        let (rec_tx, mut rec_rx) = mpsc::channel::<RecEvt>(2000);
        let call_sid_for_recording = call_sid.to_string();
        let session_id_for_recording = session_id;
        let db_for_recording = state.db.clone();
        let s3_for_recording = state.call_recordings.clone();
        let call_state_for_recording = state.call_state.clone();
        tokio::spawn(async move {
            let mut recorder = match CallAudioRecorder::new(&call_sid_for_recording).await {
                Ok(r) => r,
                Err(e) => {
                    tracing::warn!(error = %e, "call recording: failed to init recorder");
                    return;
                }
            };

            while let Some(evt) = rec_rx.recv().await {
                match evt {
                    RecEvt::UserUlawB64(b64) => {
                        let _ = recorder.write_user_ulaw_b64(&b64).await;
                    }
                    RecEvt::AssistantUlawB64(b64) => {
                        let _ = recorder.write_assistant_ulaw_b64(&b64).await;
                    }
                    RecEvt::End => break,
                }
            }

            let artifacts = match recorder.finish().await {
                Ok(a) => a,
                Err(e) => {
                    tracing::warn!(error = %e, "call recording: failed to finalize wav");
                    return;
                }
            };

            let key = s3_for_recording.key_for_call(&call_sid_for_recording);
            let uploaded = match s3_for_recording.upload_wav_path(&key, &artifacts.wav_path).await {
                Ok(u) => u,
                Err(e) => {
                    tracing::warn!(error = %e, "call recording: s3 upload failed");
                    return;
                }
            };

            // Persist into call_sessions.metadata without clobbering prompt/config metadata.
            let patch = json!({
                "recording": {
                    "bucket": uploaded.bucket,
                    "key": uploaded.key,
                    "content_type": uploaded.content_type,
                    "duration_secs": artifacts.duration_secs,
                    "size_bytes": artifacts.size_bytes,
                    "sample_rate_hz": artifacts.sample_rate_hz,
                    "channels": artifacts.channels,
                    "bits_per_sample": artifacts.bits_per_sample,
                    "format": "wav_pcm8_mono"
                }
            });

            if let Err(e) = CallService::merge_metadata(&db_for_recording, session_id_for_recording, patch).await {
                tracing::warn!(error = %e, "call recording: failed to persist metadata");
            }

            call_state_for_recording.set_recording_ready(
                &call_sid_for_recording,
                crate::services::RecordingEntry {
                    bucket: s3_for_recording.bucket().to_string(),
                    key,
                    content_type: "audio/wav".to_string(),
                    duration_secs: artifacts.duration_secs,
                    size_bytes: artifacts.size_bytes,
                    created_at: chrono::Utc::now(),
                }
            );

            // best-effort cleanup local wav
            let _ = tokio::fs::remove_file(&artifacts.wav_path).await;
        });
        Some(rec_tx)
    } else {
        None
    };

    // Channels for audio flow
    let (twilio_tx, mut twilio_rx) = mpsc::channel::<String>(200);
    let (xai_audio_tx, mut xai_audio_rx) = mpsc::channel::<String>(200);
    let (ws_out_tx, mut ws_out_rx) = mpsc::channel::<String>(200);

    // Shutdown signal: flips true when the Twilio stream stops/closes so background tasks halt.
    let (shutdown_tx, shutdown_rx) = watch::channel::<bool>(false);

    // Outbound audio task: send Twilio outbound media frames
    let stream_sid_for_sender = stream_sid.clone();
    let rec_tx_for_sender = rec_tx.clone();
    let outbound_audio_handle = tokio::spawn(async move {
        while let Some(audio_base64) = twilio_rx.recv().await {
            let sid = stream_sid_for_sender.read().await.clone();
            if !sid.is_empty() {
                // record only what we actually send
                if let Some(tx) = rec_tx_for_sender.as_ref() {
                    let _ = tx.try_send(RecEvt::AssistantUlawB64(audio_base64.clone()));
                }
                let msg = TwilioOutboundMedia::new(&sid, &audio_base64);
                if let Ok(json) = serde_json::to_string(&msg) {
                    if ws_out_tx.send(json).await.is_err() {
                        break;
                    }
                }
            }
        }
    });

    // xAI session task
    let state_clone = state.clone();
    let call_sid_for_xai = call_sid.to_string();
    let tool_ctx_clone = tool_ctx.clone();
    let mut shutdown_rx_xai = shutdown_rx.clone();
    let voice = voice_override.unwrap_or_else(|| "Ara".to_string());
    let xai_handle = tokio::spawn(async move {
        use std::collections::VecDeque;

        let mut xai_speech_started: u64 = 0;
        let mut xai_speech_stopped: u64 = 0;
        let mut xai_input_transcripts: u64 = 0;
        let mut xai_output_audio_deltas: u64 = 0;
        let mut xai_output_audio_bytes_b64: u64 = 0;
        let mut xai_last_input_transcript: Option<String> = None;

        // Build tool declarations from our MCP registry.
        let tools: Vec<crate::mcp::ToolDefinition> = crate::mcp::ToolRegistry::list_tools();
        let tool_cfgs: Vec<XaiToolConfig> = tools
            .into_iter()
            .map(|t| XaiToolConfig::Function {
                name: t.name,
                description: t.description,
                parameters: t.input_schema,
            })
            .collect();

        let session_update = XaiSessionUpdate {
            session: XaiSessionConfig {
                instructions: system_prompt,
                voice,
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
                tools: Some(tool_cfgs),
            },
        };

        let connect_fut = state_clone.xai.connect(session_update);
        let connect_result = tokio::select! {
            _ = shutdown_rx_xai.changed() => {
                return;
            }
            res = connect_fut => res,
        };

        let mut session = match connect_result {
            Ok(s) => s,
            Err(e) => {
                tracing::error!("failed to connect to xai realtime: {}", e);
                state_clone.call_state.end_call(&call_sid_for_xai, true);
                return;
            }
        };

        let mut setup_complete = false;
        let mut pending_audio_ulaw: VecDeque<String> = VecDeque::with_capacity(200);
        let mut pending_greeting = false;

        let mut current_assistant_text = String::new();

        loop {
            if *shutdown_rx_xai.borrow() {
                break;
            }

            tokio::select! {
                _ = shutdown_rx_xai.changed() => {
                    if *shutdown_rx_xai.borrow() {
                        break;
                    }
                }
                Some(audio_b64_ulaw) = xai_audio_rx.recv() => {
                    if audio_b64_ulaw == "__GREETING__" {
                        pending_greeting = true;
                        continue;
                    }

                    if !setup_complete {
                        if pending_audio_ulaw.len() >= 200 {
                            pending_audio_ulaw.pop_front();
                        }
                        pending_audio_ulaw.push_back(audio_b64_ulaw);
                        continue;
                    }

                    // normal audio path: twilio g711_ulaw base64 passthrough to xai (audio/pcmu)
                    if let Err(e) = session.append_audio_b64(audio_b64_ulaw).await {
                        tracing::error!("failed to send audio to xai realtime: {}", e);
                        break;
                    }
                }
                evt_opt = session.recv_event() => {
                    let evt = match evt_opt {
                        Some(e) => e,
                        None => break,
                    };

                    match evt {
                        XaiServerEvent::SessionUpdated { .. } => {
                            if !setup_complete {
                                setup_complete = true;
                                state_clone.call_state.set_active(&call_sid_for_xai);
                                tracing::info!("xai realtime session.updated received");

                                if !pending_audio_ulaw.is_empty() {
                                    tracing::info!(
                                        buffered_audio_frames = pending_audio_ulaw.len(),
                                        "xai realtime: flushing buffered twilio audio frames after session.updated"
                                    );
                                }
                                while let Some(ulaw_b64) = pending_audio_ulaw.pop_front() {
                                    let _ = session.append_audio_b64(ulaw_b64).await;
                                }

                                if pending_greeting {
                                    pending_greeting = false;
                                    let _ = session.send_user_text("hola").await;
                                    let _ = session.request_response().await;
                                }
                            }
                        }
                        XaiServerEvent::ConversationCreated { .. } => {
                            tracing::debug!("xai realtime conversation.created");
                        }
                        XaiServerEvent::InputAudioBufferSpeechStarted { item_id, .. } => {
                            xai_speech_started = xai_speech_started.saturating_add(1);
                            tracing::debug!(
                                count = xai_speech_started,
                                item_id = %item_id,
                                "xai realtime: vad speech_started"
                            );
                        }
                        XaiServerEvent::InputAudioBufferSpeechStopped { item_id, .. } => {
                            xai_speech_stopped = xai_speech_stopped.saturating_add(1);
                            tracing::debug!(
                                count = xai_speech_stopped,
                                item_id = %item_id,
                                "xai realtime: vad speech_stopped"
                            );
                        }
                        XaiServerEvent::ConversationItemInputAudioTranscriptionCompleted { transcript, .. } => {
                            xai_input_transcripts = xai_input_transcripts.saturating_add(1);
                            xai_last_input_transcript = Some(transcript.clone());
                            tracing::info!(
                                count = xai_input_transcripts,
                                transcript = %transcript,
                                "xai realtime: input transcription completed"
                            );
                            state_clone.call_state.add_transcript(
                                &call_sid_for_xai,
                                Speaker::User,
                                transcript,
                                false,
                            );
                        }
                        XaiServerEvent::ResponseOutputAudioTranscriptDelta { delta, .. } => {
                            if !delta.trim().is_empty() {
                                current_assistant_text.push_str(&delta);
                                state_clone.call_state.add_transcript(
                                    &call_sid_for_xai,
                                    Speaker::Assistant,
                                    current_assistant_text.clone(),
                                    true,
                                );
                            }
                        }
                        XaiServerEvent::ResponseOutputAudioTranscriptDone { .. } => {
                            let final_text = current_assistant_text.trim().to_string();
                            if !final_text.is_empty() {
                                state_clone.call_state.add_transcript(
                                    &call_sid_for_xai,
                                    Speaker::Assistant,
                                    final_text.clone(),
                                    false,
                                );
                                let mut acc = transcript_clone.lock().await;
                                acc.push_str(&final_text);
                                acc.push('\n');
                            }
                            current_assistant_text.clear();
                        }
                        XaiServerEvent::ResponseOutputAudioDelta { delta, .. } => {
                            xai_output_audio_deltas = xai_output_audio_deltas.saturating_add(1);
                            xai_output_audio_bytes_b64 = xai_output_audio_bytes_b64.saturating_add(delta.len() as u64);
                            if xai_output_audio_deltas % 50 == 0 {
                                tracing::debug!(
                                    deltas = xai_output_audio_deltas,
                                    total_b64_chars = xai_output_audio_bytes_b64,
                                    last_input_transcript = ?xai_last_input_transcript,
                                    "xai realtime: outbound audio deltas flowing"
                                );
                            }
                            let _ = twilio_tx.send(delta).await;
                        }
                        XaiServerEvent::ResponseFunctionCallArgumentsDone { name, call_id, arguments, .. } => {
                            state_clone.call_state.add_tool_call(
                                &call_sid_for_xai,
                                call_id.clone(),
                                name.clone(),
                                arguments.clone(),
                            );

                            let args_json: serde_json::Value = serde_json::from_str(&arguments).unwrap_or_else(|_| serde_json::json!({}));
                            let result = crate::mcp::execute_tool(&tool_ctx_clone, &name, args_json).await;
                            let _ = CallService::record_tool_usage(&state_clone.db, session_id, &name).await;

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

                            let is_error = result.is_error.unwrap_or(false)
                                || result_json.get("error").is_some()
                                || result_json.get("errors").is_some();

                            let result_str = serde_json::to_string(&result_json).unwrap_or_default();
                            state_clone.call_state.complete_tool_call_with_meta(
                                &call_sid_for_xai,
                                &call_id,
                                result_str,
                                Some(is_error),
                            );

                            let _ = session.send_function_result(&call_id, result_json).await;
                            let _ = session.request_response().await;
                        }
                        XaiServerEvent::Error { error } => {
                            tracing::error!(error = %error, "xai realtime server error");
                        }
                        _ => {}
                    }
                }
                else => break,
            }
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

    // Twilio websocket loop (provider-agnostic plumbing)
    let mut twilio_media_dropped_outbound: u64 = 0;
    let mut twilio_media_frames_inbound: u64 = 0;
    let mut last_twilio_audio_avg_abs: i32 = -1;
    let mut last_twilio_audio_max_abs: i32 = -1;
    let mut logged_twilio_parse_error = false;
    loop {
        tokio::select! {
            _ = external_shutdown_rx.changed() => {
                if *external_shutdown_rx.borrow() {
                    tracing::info!("external shutdown requested (twilio status callback)");
                    let _ = shutdown_tx.send(true);
                    if let Some(tx) = rec_tx.as_ref() {
                        let _ = tx.try_send(RecEvt::End);
                    }
                    break;
                }
            }
            Some(json) = ws_out_rx.recv() => {
                if ws_sender.send(axum::extract::ws::Message::Text(json)).await.is_err() {
                    break;
                }
            }
            msg = ws_receiver.next() => {
                match msg {
                    Some(Ok(axum::extract::ws::Message::Text(text))) => {
                        match serde_json::from_str::<TwilioStreamMessage>(&text) {
                            Ok(twilio_msg) => {
                                match twilio_msg {
                                    TwilioStreamMessage::Start { stream_sid: sid, start } => {
                                        *stream_sid_clone.write().await = sid;
                                        tracing::info!(
                                            call_sid = %start.call_sid,
                                            tracks = ?start.tracks,
                                            encoding = %start.media_format.encoding,
                                            sample_rate = start.media_format.sample_rate,
                                            channels = start.media_format.channels,
                                            "Stream started"
                                        );
                                        tracing::info!(
                                            encoding = %start.media_format.encoding,
                                            sample_rate = start.media_format.sample_rate,
                                            "xai realtime: configured for audio/pcmu passthrough (twilio should be audio/x-mulaw@8khz)"
                                        );

                                        // trigger a greeting shortly after connect
                                        let greeting_tx = xai_audio_tx.clone();
                                        tokio::spawn(async move {
                                            tokio::time::sleep(tokio::time::Duration::from_millis(400)).await;
                                            let _ = greeting_tx.send("__GREETING__".to_string()).await;
                                        });
                                    }
                                    TwilioStreamMessage::Media { media, .. } => {
                                        // Twilio may send outbound track too; ignore it.
                                        match media.track.as_deref() {
                                            Some("outbound") => {
                                                twilio_media_dropped_outbound = twilio_media_dropped_outbound.saturating_add(1);
                                                continue;
                                            }
                                            Some("inbound") | None | Some("") => {}
                                            Some(other) => {
                                                tracing::debug!(track = %other, "twilio media: unknown track; treating as inbound");
                                            }
                                        }

                                        twilio_media_frames_inbound = twilio_media_frames_inbound.saturating_add(1);

                                        // low-frequency sanity sampling of inbound audio energy to catch "i'm talking but nothing arrives"
                                        if twilio_media_frames_inbound % 80 == 0 {
                                            // 80 frames ~= 1.6s if 20ms frames
                                            if let Ok(ulaw) = base64::engine::general_purpose::STANDARD.decode(media.payload.as_bytes()) {
                                                let pcm = ulaw_to_pcm16(&ulaw);
                                                if !pcm.is_empty() {
                                                    let mut acc: i64 = 0;
                                                    let mut mx: i32 = 0;
                                                    for &s in &pcm {
                                                        let a = (s as i32).abs();
                                                        acc += a as i64;
                                                        if a > mx { mx = a; }
                                                    }
                                                    last_twilio_audio_avg_abs = (acc / (pcm.len() as i64)) as i32;
                                                    last_twilio_audio_max_abs = mx;
                                                } else {
                                                    last_twilio_audio_avg_abs = 0;
                                                    last_twilio_audio_max_abs = 0;
                                                }
                                            }

                                            tracing::debug!(
                                                inbound_frames = twilio_media_frames_inbound,
                                                dropped_outbound = twilio_media_dropped_outbound,
                                                payload_b64_len = media.payload.len(),
                                                avg_abs = last_twilio_audio_avg_abs,
                                                max_abs = last_twilio_audio_max_abs,
                                                "xai realtime: twilio inbound audio sanity"
                                            );
                                        }

                                        // feed xai
                                        if xai_audio_tx.send(media.payload.clone()).await.is_err() {
                                            tracing::warn!("xai audio channel closed; ending twilio stream loop");
                                            let _ = shutdown_tx.send(true);
                                            if let Some(tx) = rec_tx.as_ref() {
                                                let _ = tx.try_send(RecEvt::End);
                                            }
                                            break;
                                        }

                                        // record without decoding
                                        if let Some(tx) = rec_tx.as_ref() {
                                            let _ = tx.try_send(RecEvt::UserUlawB64(media.payload.clone()));
                                        }
                                    }
                                    TwilioStreamMessage::Stop { .. } => {
                                        tracing::info!("Stream stopped");
                                        let _ = shutdown_tx.send(true);
                                        if let Some(tx) = rec_tx.as_ref() {
                                            let _ = tx.try_send(RecEvt::End);
                                        }
                                        break;
                                    }
                                    _ => {}
                                }
                            }
                            Err(e) => {
                                if !logged_twilio_parse_error {
                                    logged_twilio_parse_error = true;
                                    let preview: String = text.chars().take(600).collect();
                                    tracing::warn!(error = %e, preview = %preview, "failed to parse twilio stream message (schema mismatch?)");
                                }
                            }
                        }
                    }
                    Some(Ok(axum::extract::ws::Message::Close(_))) => {
                        tracing::info!("WebSocket closed");
                        let _ = shutdown_tx.send(true);
                        if let Some(tx) = rec_tx.as_ref() {
                            let _ = tx.try_send(RecEvt::End);
                        }
                        break;
                    }
                    Some(Err(e)) => {
                        tracing::error!("WebSocket error: {}", e);
                        let _ = shutdown_tx.send(true);
                        if let Some(tx) = rec_tx.as_ref() {
                            let _ = tx.try_send(RecEvt::End);
                        }
                        break;
                    }
                    None => break,
                    _ => {}
                }
            }
        }
    }

    // ensure background tasks don't linger/burn money if the call is over.
    let _ = shutdown_tx.send(true);
    drop(xai_audio_tx);

    let mut xai_handle = xai_handle;
    tokio::select! {
        _ = &mut xai_handle => {}
        _ = tokio::time::sleep(tokio::time::Duration::from_secs(2)) => {
            xai_handle.abort();
        }
    }

    let mut outbound_audio_handle = outbound_audio_handle;
    tokio::select! {
        _ = &mut outbound_audio_handle => {}
        _ = tokio::time::sleep(tokio::time::Duration::from_secs(1)) => {
            outbound_audio_handle.abort();
        }
    }
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
    gemini_overrides: GeminiLiveSetupOverrides,
    mut external_shutdown_rx: watch::Receiver<bool>,
) {
    if state.config.gemini_api_key.is_empty() {
        tracing::error!("GEMINI_API_KEY/GOOGLE_API_KEY not set; cannot start Gemini Live session");
        state.call_state.end_call(call_sid, true);
        return;
    }

    // Track stream SID for sending audio back
    let stream_sid = Arc::new(tokio::sync::RwLock::new(String::new()));
    let stream_sid_clone = stream_sid.clone();

    // Barge-in state: when true, we suppress outbound AI audio to Twilio.
    let suppress_outbound_audio = Arc::new(AtomicBool::new(false));
    // Whether the assistant is currently speaking (heuristic set by model audio output).
    let assistant_speaking = Arc::new(AtomicBool::new(false));
    // Millis since UNIX epoch when we last actually sent assistant audio to Twilio.
    let last_assistant_audio_sent_ms = Arc::new(AtomicU64::new(0));

    // Shared transcript accumulator (optional, only for DB transcript field)
    let transcript = Arc::new(tokio::sync::Mutex::new(String::new()));
    let transcript_clone = transcript.clone();

    // Call recording worker: records both tracks while the call is running, then uploads to S3 after call end.
    #[derive(Debug)]
    enum RecEvt {
        UserUlawB64(String),
        AssistantUlawB64(String),
        End,
    }

    let rec_tx: Option<mpsc::Sender<RecEvt>> = if ENABLE_CALL_RECORDING {
        let (rec_tx, mut rec_rx) = mpsc::channel::<RecEvt>(2000);
        let call_sid_for_recording = call_sid.to_string();
        let session_id_for_recording = session_id;
        let db_for_recording = state.db.clone();
        let s3_for_recording = state.call_recordings.clone();
        let call_state_for_recording = state.call_state.clone();
        tokio::spawn(async move {
            let mut recorder = match CallAudioRecorder::new(&call_sid_for_recording).await {
                Ok(r) => r,
                Err(e) => {
                    tracing::warn!(error = %e, "call recording: failed to init recorder");
                    return;
                }
            };

            while let Some(evt) = rec_rx.recv().await {
                match evt {
                    RecEvt::UserUlawB64(b64) => {
                        let _ = recorder.write_user_ulaw_b64(&b64).await;
                    }
                    RecEvt::AssistantUlawB64(b64) => {
                        let _ = recorder.write_assistant_ulaw_b64(&b64).await;
                    }
                    RecEvt::End => break,
                }
            }

            let artifacts = match recorder.finish().await {
                Ok(a) => a,
                Err(e) => {
                    tracing::warn!(error = %e, "call recording: failed to finalize wav");
                    return;
                }
            };

            let key = s3_for_recording.key_for_call(&call_sid_for_recording);
            let uploaded = match s3_for_recording.upload_wav_path(&key, &artifacts.wav_path).await {
                Ok(u) => u,
                Err(e) => {
                    tracing::warn!(error = %e, "call recording: s3 upload failed");
                    return;
                }
            };

            // Persist into call_sessions.metadata without clobbering prompt/config metadata.
            let patch = json!({
                "recording": {
                    "bucket": uploaded.bucket,
                    "key": uploaded.key,
                    "content_type": uploaded.content_type,
                    "duration_secs": artifacts.duration_secs,
                    "size_bytes": artifacts.size_bytes,
                    "sample_rate_hz": artifacts.sample_rate_hz,
                    "channels": artifacts.channels,
                    "bits_per_sample": artifacts.bits_per_sample,
                    "format": "wav_pcm8_mono"
                }
            });

            if let Err(e) = CallService::merge_metadata(&db_for_recording, session_id_for_recording, patch).await {
                tracing::warn!(error = %e, "call recording: failed to persist metadata");
            }

            call_state_for_recording.set_recording_ready(
                &call_sid_for_recording,
                crate::services::RecordingEntry {
                    bucket: s3_for_recording.bucket().to_string(),
                    key,
                    content_type: "audio/wav".to_string(),
                    duration_secs: artifacts.duration_secs,
                    size_bytes: artifacts.size_bytes,
                    created_at: chrono::Utc::now(),
                }
            );

            // best-effort cleanup local wav
            let _ = tokio::fs::remove_file(&artifacts.wav_path).await;
        });
        Some(rec_tx)
    } else {
        None
    };

    // Channels for audio flow
    let (twilio_tx, mut twilio_rx) = mpsc::channel::<String>(200);
    let (gemini_audio_tx, mut gemini_audio_rx) = mpsc::channel::<String>(200);
    let (ws_out_tx, mut ws_out_rx) = mpsc::channel::<String>(200);

    // Channel for signaling Twilio to clear its outbound buffer (on Gemini's `interrupted`)
    let (clear_buffer_tx, mut clear_buffer_rx) = mpsc::channel::<()>(8);

    // Shutdown signal: flips true when the Twilio stream stops/closes so background tasks halt.
    let (shutdown_tx, shutdown_rx) = watch::channel::<bool>(false);

    // Outbound audio task: send Twilio outbound media frames
    let stream_sid_for_sender = stream_sid.clone();
    let suppress_for_sender = suppress_outbound_audio.clone();
    let last_sent_for_sender = last_assistant_audio_sent_ms.clone();
    let rec_tx_for_sender = rec_tx.clone();
    let outbound_audio_handle = tokio::spawn(async move {
        while let Some(audio_base64) = twilio_rx.recv().await {
            if suppress_for_sender.load(Ordering::Relaxed) {
                // barge-in: drop any queued assistant audio
                continue;
            }
            let sid = stream_sid_for_sender.read().await.clone();
            if !sid.is_empty() {
                // record only what we actually send (so barge-in suppressed audio doesn't appear in playback)
                if let Some(tx) = rec_tx_for_sender.as_ref() {
                    let _ = tx.try_send(RecEvt::AssistantUlawB64(audio_base64.clone()));
                }
                // barge-in hangover: mark "assistant speaking recently" when we actually send audio
                let now_ms = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_millis() as u64)
                    .unwrap_or(0);
                last_sent_for_sender.store(now_ms, Ordering::Relaxed);

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
    let gemini_overrides = gemini_overrides.clone();
    let suppress_outbound_audio_clone = suppress_outbound_audio.clone();
    let assistant_speaking_clone = assistant_speaking.clone();
    let clear_buffer_tx_clone = clear_buffer_tx.clone();
    let mut shutdown_rx_gemini = shutdown_rx.clone();
    let gemini_handle = tokio::spawn(async move {
        use crate::clients::gemini_live::FunctionResponse;
        use std::collections::HashSet;

        let client = state_clone.gemini.clone();
        let mut resume_handle: Option<String> = None;
        let mut reconnect_attempts_without_handle: u32 = 0;
        let mut reconnect_attempts_with_handle_no_setup: u32 = 0;
        let mut greeted = false;
        let mut pending_greeting = false;
        let mut current_user_text = String::new();
        let mut current_assistant_text = String::new();
        let mut cancelled_tool_call_ids: HashSet<String> = HashSet::new();

        // Channel for async tool execution results.
        // Tools are spawned as separate tasks to avoid blocking the audio loop.
        let (tool_response_tx, mut tool_response_rx) = mpsc::channel::<FunctionResponse>(32);

        // Audio batcher: ensures we send chunks of at least 20ms to Gemini per best practices.
        // Twilio sends 20ms @ 8kHz, which after upsampling becomes exactly 640 bytes (20ms @ 16kHz),
        // so this is mostly a pass-through, but provides safety if chunk sizes vary.
        let mut audio_batcher = GeminiAudioBatcher::new();

        // Tool declarations come from our MCP registry.
        let tools: Vec<crate::mcp::ToolDefinition> = crate::mcp::ToolRegistry::list_tools();

        fn merge_streaming_text(current: &mut String, incoming: &str) {
            let incoming = incoming.trim();
            if incoming.is_empty() {
                return;
            }
            if current.is_empty() {
                current.push_str(incoming);
                return;
            }
            // Some streams send cumulative text; some send deltas.
            if incoming.starts_with(current.as_str()) {
                current.clear();
                current.push_str(incoming);
                return;
            }
            if current.starts_with(incoming) {
                // ignore regressions (can happen with partial rescoring)
                return;
            }
            // otherwise treat as delta and append with spacing
            if !current.ends_with(' ') && !incoming.starts_with(' ') {
                current.push(' ');
            }
            current.push_str(incoming);
        }

        fn maybe_commit_final(
            call_state: &crate::services::CallStateStore,
            call_sid: &str,
            speaker: Speaker,
            current: &mut String,
        ) -> Option<String> {
            let final_text = current.trim().to_string();
            if final_text.is_empty() {
                current.clear();
                return None;
            }
            call_state.add_transcript(call_sid, speaker, final_text.clone(), false);
            current.clear();
            Some(final_text)
        }

        'outer: loop {
            if *shutdown_rx_gemini.borrow() {
                break 'outer;
            }

            let used_resume_handle = resume_handle.is_some();
            let connect_fut = client.connect_live(
                dynamic_prompt.clone(),
                tools.clone(),
                resume_handle.clone(),
                gemini_overrides.clone(),
            );

            let connect_result = tokio::select! {
                _ = shutdown_rx_gemini.changed() => {
                    break 'outer;
                }
                res = connect_fut => res,
            };

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
            let setup_deadline = tokio::time::sleep(tokio::time::Duration::from_secs(6));
            tokio::pin!(setup_deadline);
            loop {
                tokio::select! {
                    _ = shutdown_rx_gemini.changed() => {
                        if *shutdown_rx_gemini.borrow() {
                            // Flush any remaining buffered audio before signaling stream end
                            if let Some(remaining) = audio_batcher.flush() {
                                let _ = session.send_realtime_audio_pcm16_16khz(&remaining).await;
                            }
                            // best-effort: tell gemini the audio stream ended, then exit.
                            let _ = session.send_audio_stream_end().await;
                            break;
                        }
                    }
                    _ = &mut setup_deadline, if !setup_complete => {
                        tracing::error!("Gemini Live setup_complete timeout (no server response)");
                        break;
                    }
                    // Handle completed tool executions (non-blocking)
                    Some(response) = tool_response_rx.recv() => {
                        // Check if this tool was cancelled while executing
                        if cancelled_tool_call_ids.contains(&response.id) {
                            tracing::info!(
                                id = %response.id,
                                name = %response.name,
                                "Gemini Live: tool completed but was cancelled, not sending response"
                            );
                            continue;
                        }

                        tracing::info!(
                            id = %response.id,
                            name = %response.name,
                            "Gemini Live: sending tool response"
                        );
                        let _ = session.send_tool_responses(vec![response]).await;
                    }
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

                        // Normal audio path: transcode and batch to ensure 20-40ms chunks per best practices
                        match twilio_ulaw_base64_to_gemini_pcm16_16khz_bytes(&audio_b64_ulaw) {
                            Ok(pcm16_16k) => {
                                // Batch audio into 20ms chunks (Twilio usually sends exactly 20ms,
                                // so this is mostly pass-through but provides safety margin)
                                for chunk in audio_batcher.push(&pcm16_16k) {
                                    if let Err(e) = session.send_realtime_audio_pcm16_16khz(&chunk).await {
                                        tracing::error!("Failed to send audio to Gemini: {}", e);
                                        break;
                                    }
                                }
                            }
                            Err(e) => tracing::warn!("Audio transcode error (twilio->gemini): {}", e),
                        }
                    }
                    msg_opt = session.recv_event() => {
                        let msg = match msg_opt {
                            Some(m) => m,
                            None => {
                                tracing::warn!("Gemini Live websocket closed (no more events)");
                                break;
                            }
                        };

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
                                    reconnect_attempts_without_handle = 0;
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
                            // Uses `finished` field to determine if this is final or streaming
                            if let Some(t) = content.input_transcription {
                                let is_final = t.finished.unwrap_or(false);
                                tracing::debug!(text = %t.text, finished = is_final, "Gemini input transcription");

                                if is_final {
                                    // Final transcription: use directly without merging heuristics
                                    current_user_text = t.text.clone();
                                    state_clone.call_state.add_transcript(
                                        &call_sid_for_gemini,
                                        Speaker::User,
                                        current_user_text.clone(),
                                        false, // not partial
                                    );
                                    // Commit immediately since it's final
                                    let _ = maybe_commit_final(&state_clone.call_state, &call_sid_for_gemini, Speaker::User, &mut current_user_text);
                                } else {
                                    // Streaming: accumulate with heuristics
                                    merge_streaming_text(&mut current_user_text, &t.text);
                                    state_clone.call_state.add_transcript(
                                        &call_sid_for_gemini,
                                        Speaker::User,
                                        current_user_text.clone(),
                                        true, // partial
                                    );
                                }
                            }
                            // Output transcription (assistant)
                            if let Some(t) = content.output_transcription {
                                let is_final = t.finished.unwrap_or(false);
                                tracing::debug!(text = %t.text, finished = is_final, "Gemini output transcription");

                                // If user was mid-partial, commit it once the assistant starts responding
                                let _ = maybe_commit_final(&state_clone.call_state, &call_sid_for_gemini, Speaker::User, &mut current_user_text);

                                if is_final {
                                    // Final transcription: use directly
                                    current_assistant_text = t.text.clone();
                                    state_clone.call_state.add_transcript(
                                        &call_sid_for_gemini,
                                        Speaker::Assistant,
                                        current_assistant_text.clone(),
                                        false,
                                    );
                                } else {
                                    // Streaming: accumulate with heuristics
                                    merge_streaming_text(&mut current_assistant_text, &t.text);
                                    state_clone.call_state.add_transcript(
                                        &call_sid_for_gemini,
                                        Speaker::Assistant,
                                        current_assistant_text.clone(),
                                        true,
                                    );
                                }
                            }

                            // Handle Gemini's `interrupted` signal (user barged in)
                            // This is more reliable than client-side VAD because Gemini
                            // has access to the full audio context.
                            if content.interrupted.unwrap_or(false) {
                                tracing::debug!("Gemini Live: interrupted signal received, clearing buffer");
                                // Immediately suppress any pending outbound audio
                                suppress_outbound_audio_clone.store(true, Ordering::Relaxed);
                                assistant_speaking_clone.store(false, Ordering::Relaxed);
                                // Signal Twilio to clear its buffer
                                let _ = clear_buffer_tx_clone.try_send(());
                            }

                            // Turn boundaries: commit any partial bubbles
                            let turn_done =
                                content.turn_complete.unwrap_or(false)
                                || content.generation_complete.unwrap_or(false)
                                || content.interrupted.unwrap_or(false);

                            if turn_done {
                                // model turn ended; allow outbound audio again
                                assistant_speaking_clone.store(false, Ordering::Relaxed);
                                // Only un-suppress after interrupted if we're done with the turn
                                suppress_outbound_audio_clone.store(false, Ordering::Relaxed);

                                if let Some(final_text) = maybe_commit_final(
                                    &state_clone.call_state,
                                    &call_sid_for_gemini,
                                    Speaker::Assistant,
                                    &mut current_assistant_text,
                                ) {
                                    let mut acc = transcript_clone.lock().await;
                                    acc.push_str(&final_text);
                                    acc.push('\n');
                                }
                                let _ = maybe_commit_final(&state_clone.call_state, &call_sid_for_gemini, Speaker::User, &mut current_user_text);
                            }

                            // Audio chunks
                            if let Some(model_turn) = content.model_turn {
                                for part in model_turn.parts {
                                    if let Some(inline) = part.inline_data {
                                        // Only handle audio payloads for now.
                                        if inline.mime_type.starts_with("audio/pcm") {
                                            assistant_speaking_clone.store(true, Ordering::Relaxed);
                                            if suppress_outbound_audio_clone.load(Ordering::Relaxed) {
                                                continue;
                                            }
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

                        // Tool calls (function calling) - spawn async to avoid blocking audio
                        if let Some(tool_call) = msg.tool_call {
                            if !tool_call.function_calls.is_empty() {
                                // commit any pending partial user text before we act on tools
                                let _ = maybe_commit_final(&state_clone.call_state, &call_sid_for_gemini, Speaker::User, &mut current_user_text);

                                tracing::info!(
                                    count = tool_call.function_calls.len(),
                                    "Gemini Live: tool_call received, spawning async execution"
                                );

                                for fc in tool_call.function_calls {
                                    if cancelled_tool_call_ids.contains(&fc.id) {
                                        tracing::info!(id = %fc.id, name = %fc.name, "skipping cancelled tool call");
                                        continue;
                                    }

                                    let call_id = fc.id.clone();
                                    let tool_name = fc.name.clone();
                                    let args = fc.args.clone();
                                    let args_str = serde_json::to_string(&args).unwrap_or_default();

                                    tracing::info!(
                                        id = %call_id,
                                        name = %tool_name,
                                        args = %args_str,
                                        "Gemini Live: spawning tool execution"
                                    );

                                    state_clone.call_state.add_tool_call(
                                        &call_sid_for_gemini,
                                        call_id.clone(),
                                        tool_name.clone(),
                                        args_str,
                                    );

                                    // Spawn tool execution to avoid blocking the audio loop
                                    let tool_ctx = tool_ctx_clone.clone();
                                    let state_for_tool = state_clone.clone();
                                    let call_sid_for_tool = call_sid_for_gemini.clone();
                                    let response_tx = tool_response_tx.clone();
                                    let session_id_for_tool = session_id;

                                    tokio::spawn(async move {
                                        let result = crate::mcp::execute_tool(&tool_ctx, &tool_name, args).await;
                                        let _ = CallService::record_tool_usage(&state_for_tool.db, session_id_for_tool, &tool_name).await;

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

                                        let is_error = result.is_error.unwrap_or(false)
                                            || result_json.get("error").is_some()
                                            || result_json.get("errors").is_some();

                                        let scheduling = match tool_name.as_str() {
                                            "call_contact" | "call_contact_by_id" => "INTERRUPT",
                                            "request_ride" | "request_ride_by_location_id" => "INTERRUPT",
                                            _ => "WHEN_IDLE",
                                        };

                                        let result_str = serde_json::to_string(&result_json).unwrap_or_default();

                                        state_for_tool.call_state.complete_tool_call_with_meta(
                                            &call_sid_for_tool,
                                            &call_id,
                                            result_str,
                                            Some(is_error),
                                        );

                                        // Send response back to main loop
                                        let response = FunctionResponse {
                                            id: call_id.clone(),
                                            name: tool_name.clone(),
                                            scheduling: Some(scheduling.to_string()),
                                            response: result_json,
                                        };

                                        if response_tx.send(response).await.is_err() {
                                            tracing::warn!(
                                                id = %call_id,
                                                name = %tool_name,
                                                "Gemini Live: failed to send tool response (channel closed)"
                                            );
                                        }
                                    });
                                }
                            }
                        }

                        if let Some(cancel) = msg.tool_call_cancellation {
                            for id in cancel.ids {
                                cancelled_tool_call_ids.insert(id.clone());
                                state_clone.call_state.complete_tool_call_with_meta(
                                    &call_sid_for_gemini,
                                    &id,
                                    "{\"error\":\"tool_call_cancelled\"}".to_string(),
                                    Some(true),
                                );
                            }
                        }
                    }
                    else => break,
                }
            }

            if *shutdown_rx_gemini.borrow() {
                break 'outer;
            }

            // If we were trying to resume and the server dropped us before setup_complete, assume
            // the handle is invalid/expired (often manifests as code=1008 "session not found").
            if used_resume_handle && !setup_complete {
                reconnect_attempts_with_handle_no_setup = reconnect_attempts_with_handle_no_setup.saturating_add(1);
                tracing::warn!(
                    attempt = reconnect_attempts_with_handle_no_setup,
                    "Gemini Live ended before setup_complete while resuming; treating resumption handle as suspect"
                );

                if reconnect_attempts_with_handle_no_setup >= 2 {
                    tracing::warn!("Clearing Gemini resumption handle to avoid infinite resume loop");
                    resume_handle = None;
                    reconnect_attempts_without_handle = 0;
                    reconnect_attempts_with_handle_no_setup = 0;
                }
            } else if setup_complete {
                reconnect_attempts_with_handle_no_setup = 0;
            }

            // If we don't have a resumable handle, don't spin forever.
            if resume_handle.is_none() {
                reconnect_attempts_without_handle = reconnect_attempts_without_handle.saturating_add(1);
                if reconnect_attempts_without_handle > 3 {
                    tracing::warn!("Gemini Live session ended and no resumable handle available; stopping");
                    break;
                }
                tracing::warn!(
                    attempt = reconnect_attempts_without_handle,
                    "Gemini Live session ended before resumption handle; retrying connect"
                );
                let backoff_ms = 250u64.saturating_mul(1u64 << (reconnect_attempts_without_handle.saturating_sub(1)));
                tokio::time::sleep(tokio::time::Duration::from_millis(std::cmp::min(backoff_ms, 2000))).await;
                continue;
            }

            // Otherwise loop to reconnect.
            tracing::info!("Reconnecting Gemini Live session (session resumption)...");
            tokio::time::sleep(tokio::time::Duration::from_millis(150)).await;
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

    // Twilio websocket loop
    // Note: Barge-in detection is handled server-side by Gemini's VAD, which sends
    // `interrupted` when the user starts speaking. We just forward audio and handle
    // the clear signal when it arrives.
    let mut twilio_media_frames: u64 = 0;
    let mut twilio_media_dropped_outbound: u64 = 0;
    let mut logged_twilio_parse_error = false;

    loop {
        tokio::select! {
            _ = external_shutdown_rx.changed() => {
                if *external_shutdown_rx.borrow() {
                    tracing::info!("external shutdown requested (twilio status callback)");
                    let _ = shutdown_tx.send(true);
                    if let Some(tx) = rec_tx.as_ref() {
                        let _ = tx.try_send(RecEvt::End);
                    }
                    break;
                }
            }
            // Handle clear buffer signal from Gemini's interrupted handler
            Some(()) = clear_buffer_rx.recv() => {
                let sid = stream_sid_clone.read().await.clone();
                if !sid.is_empty() {
                    let clear = TwilioOutboundClear::new(&sid);
                    if let Ok(clear_json) = serde_json::to_string(&clear) {
                        let _ = ws_sender.send(axum::extract::ws::Message::Text(clear_json)).await;
                    }
                }
                // Also drain any pending outbound media
                while let Ok(_dropped) = ws_out_rx.try_recv() {}
                tracing::debug!("Twilio buffer cleared on Gemini interrupted signal");
            }
            Some(json) = ws_out_rx.recv() => {
                if ws_sender.send(axum::extract::ws::Message::Text(json)).await.is_err() {
                    break;
                }
            }
            msg = ws_receiver.next() => {
                match msg {
                    Some(Ok(axum::extract::ws::Message::Text(text))) => {
                        match serde_json::from_str::<TwilioStreamMessage>(&text) {
                            Ok(twilio_msg) => {
                                match twilio_msg {
                                    TwilioStreamMessage::Start { stream_sid: sid, start } => {
                                        *stream_sid_clone.write().await = sid;
                                        tracing::info!(
                                            call_sid = %start.call_sid,
                                            tracks = ?start.tracks,
                                            encoding = %start.media_format.encoding,
                                            sample_rate = start.media_format.sample_rate,
                                            channels = start.media_format.channels,
                                            "Stream started"
                                        );

                                        // send a greeting trigger to gemini after a short delay
                                        let greeting_tx = gemini_audio_tx.clone();
                                        tokio::spawn(async move {
                                            tokio::time::sleep(tokio::time::Duration::from_millis(400)).await;
                                            let _ = greeting_tx.send("__GREETING__".to_string()).await;
                                        });
                                    }

                                    TwilioStreamMessage::Media { media, .. } => {
                                        // IMPORTANT: Twilio may send both inbound and outbound tracks.
                                        // Track may be omitted for single-track streams; treat missing/empty as inbound.
                                        match media.track.as_deref() {
                                            Some("outbound") => {
                                                twilio_media_dropped_outbound = twilio_media_dropped_outbound.saturating_add(1);
                                                continue;
                                            }
                                            Some("inbound") | None | Some("") => {}
                                            Some(other) => {
                                                tracing::debug!(track = %other, "twilio media: unknown track; treating as inbound");
                                            }
                                        }

                                        twilio_media_frames = twilio_media_frames.saturating_add(1);
                                        if twilio_media_frames % 80 == 0 {
                                            // sanity-check inbound audio energy (helps debug "i talk and nothing happens").
                                            // this is low-frequency to avoid hot-path overhead.
                                            let mut avg_abs: i32 = -1;
                                            let mut max_abs: i32 = -1;
                                            if let Ok(ulaw) = base64::engine::general_purpose::STANDARD.decode(media.payload.as_bytes()) {
                                                let pcm = crate::clients::audio::ulaw_to_pcm16(&ulaw);
                                                if !pcm.is_empty() {
                                                    let mut acc: i64 = 0;
                                                    let mut mx: i32 = 0;
                                                    for &s in &pcm {
                                                        let a = (s as i32).abs();
                                                        acc += a as i64;
                                                        if a > mx { mx = a; }
                                                    }
                                                    avg_abs = (acc / (pcm.len() as i64)) as i32;
                                                    max_abs = mx;
                                                } else {
                                                    avg_abs = 0;
                                                    max_abs = 0;
                                                }
                                            }
                                            tracing::debug!(
                                                frames = twilio_media_frames,
                                                dropped_outbound = twilio_media_dropped_outbound,
                                                payload_b64_len = media.payload.len(),
                                                avg_abs = avg_abs,
                                                max_abs = max_abs,
                                                "twilio media frames flowing"
                                            );
                                        }

                                        // Forward audio to Gemini (barge-in is handled server-side)
                                        if gemini_audio_tx.send(media.payload.clone()).await.is_err() {
                                            tracing::warn!("gemini audio channel closed; ending twilio stream loop");
                                            let _ = shutdown_tx.send(true);
                                            if let Some(tx) = rec_tx.as_ref() {
                                                let _ = tx.try_send(RecEvt::End);
                                            }
                                            break;
                                        }

                                        // Enqueue recording
                                        if let Some(tx) = rec_tx.as_ref() {
                                            let _ = tx.try_send(RecEvt::UserUlawB64(media.payload.clone()));
                                        }
                                    }
                                    TwilioStreamMessage::Stop { .. } => {
                                        tracing::info!("Stream stopped");
                                        let _ = shutdown_tx.send(true);
                                        if let Some(tx) = rec_tx.as_ref() {
                                            let _ = tx.try_send(RecEvt::End);
                                        }
                                        break;
                                    }
                                    _ => {}
                                }
                            }
                            Err(e) => {
                                if !logged_twilio_parse_error {
                                    logged_twilio_parse_error = true;
                                    let preview: String = text.chars().take(600).collect();
                                    tracing::warn!(error = %e, preview = %preview, "failed to parse twilio stream message (schema mismatch?)");
                                }
                            }
                        }
                    }
                    Some(Ok(axum::extract::ws::Message::Close(_))) => {
                        tracing::info!("WebSocket closed");
                        let _ = shutdown_tx.send(true);
                        if let Some(tx) = rec_tx.as_ref() {
                            let _ = tx.try_send(RecEvt::End);
                        }
                        break;
                    }
                    Some(Err(e)) => {
                        tracing::error!("WebSocket error: {}", e);
                        let _ = shutdown_tx.send(true);
                        if let Some(tx) = rec_tx.as_ref() {
                            let _ = tx.try_send(RecEvt::End);
                        }
                        break;
                    }
                    None => break,
                    _ => {}
                }
            }
        }
    }

    // ensure background tasks don't linger/burn money if the call is over.
    // try graceful shutdown first so the gemini task can persist end_session; fall back to abort.
    let _ = shutdown_tx.send(true);
    drop(gemini_audio_tx);

    let mut gemini_handle = gemini_handle;
    tokio::select! {
        _ = &mut gemini_handle => {}
        _ = tokio::time::sleep(tokio::time::Duration::from_secs(2)) => {
            gemini_handle.abort();
        }
    }

    let mut outbound_audio_handle = outbound_audio_handle;
    tokio::select! {
        _ = &mut outbound_audio_handle => {}
        _ = tokio::time::sleep(tokio::time::Duration::from_secs(1)) => {
            outbound_audio_handle.abort();
        }
    }
}

/// Handle OpenAI Realtime voice stream
#[allow(dead_code)]
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
#[allow(dead_code)]
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

    // Date context (Mexico City timezone, Spanish long-form)
    context_parts.push(format!(
        "## Fecha\nLA FECHA DEL DIA DE HOY: {}",
        today_spanish_mexico_city()
    ));
    
    // Add elder's name for personalization
    context_parts.push(format!(
        "## Información del Usuario\nEstás hablando con **{}**.",
        elder.name
    ));
    
    // Fetch caregiver relationship notes if available
    match CaregiverRepository::get_relationship(&state.db, elder.caregiver_id, elder.id).await {
        Ok(Some(relationship)) => {
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
        Ok(None) => {}
        Err(e) => {
            tracing::warn!(error = %e, elder_id = %elder.id, "voice context: failed to load caregiver relationship");
        }
    }
    
    // Fetch and add contacts
    match ContactService::get_all_contacts(&state.db, elder.id).await {
        Ok(contacts) => {
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
        Err(e) => {
            tracing::warn!(error = %e, elder_id = %elder.id, "voice context: failed to load contacts");
        }
    }
    
    // Fetch and add locations (separated by type)
    match LocationService::get_all_locations(&state.db, elder.id).await {
        Ok(locations) => {
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
        Err(e) => {
            tracing::warn!(error = %e, elder_id = %elder.id, "voice context: failed to load locations");
        }
    }
    
    // Fetch and add medications - FULL details in context (no tool needed)
    match MedicationService::get_all_medications(&state.db, elder.id).await {
        Ok(meds) => {
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
        Err(e) => {
            tracing::warn!(error = %e, elder_id = %elder.id, "voice context: failed to load medications");
            context_parts.push("## Medicamentos\nERROR: no se pudieron cargar los medicamentos por un error del sistema.".to_string());
        }
    }
    
    context_parts.join("\n\n")
}

fn today_spanish_mexico_city() -> String {
    // Mexico City is UTC-6 year-round (DST removed for most of Mexico).
    let mx = FixedOffset::west_opt(6 * 3600).unwrap();
    let now = Utc::now().with_timezone(&mx);

    let weekday = match now.weekday() {
        Weekday::Mon => "lunes",
        Weekday::Tue => "martes",
        Weekday::Wed => "miércoles",
        Weekday::Thu => "jueves",
        Weekday::Fri => "viernes",
        Weekday::Sat => "sábado",
        Weekday::Sun => "domingo",
    };

    let month = match now.month() {
        1 => "enero",
        2 => "febrero",
        3 => "marzo",
        4 => "abril",
        5 => "mayo",
        6 => "junio",
        7 => "julio",
        8 => "agosto",
        9 => "septiembre",
        10 => "octubre",
        11 => "noviembre",
        12 => "diciembre",
        _ => "???",
    };

    format!("{weekday} {} de {month} de {}", now.day(), now.year())
}
