//! call audio recording (twilio media stream) -> stereo wav (user left, assistant right)

use std::path::{Path, PathBuf};

use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use tokio::io::{AsyncWriteExt, BufWriter};

use crate::clients::audio;

#[derive(Debug, thiserror::Error)]
pub enum CallRecordingError {
    #[error("io error: {0}")]
    Io(String),
    #[error("base64 decode error: {0}")]
    Base64(String),
    #[error("wav build error: {0}")]
    Wav(String),
}

#[derive(Debug, Clone)]
pub struct CallRecordingArtifacts {
    pub wav_path: PathBuf,
    pub sample_rate_hz: u32,
    pub channels: u16,
    pub duration_secs: f32,
    pub size_bytes: u64,
}

/// streams ulaw bytes to temp files, then builds a stereo wav at the end.
pub struct CallAudioRecorder {
    call_sid: String,
    user_ulaw_path: PathBuf,
    assistant_ulaw_path: PathBuf,
    user_file: BufWriter<tokio::fs::File>,
    assistant_file: BufWriter<tokio::fs::File>,
}

impl CallAudioRecorder {
    pub async fn new(call_sid: &str) -> Result<Self, CallRecordingError> {
        let base = std::env::temp_dir();
        let user_ulaw_path = base.join(format!("domingo-{}-user.ulaw", call_sid));
        let assistant_ulaw_path = base.join(format!("domingo-{}-assistant.ulaw", call_sid));

        let user_file = tokio::fs::File::create(&user_ulaw_path)
            .await
            .map_err(|e| CallRecordingError::Io(e.to_string()))?;
        let assistant_file = tokio::fs::File::create(&assistant_ulaw_path)
            .await
            .map_err(|e| CallRecordingError::Io(e.to_string()))?;

        Ok(Self {
            call_sid: call_sid.to_string(),
            user_ulaw_path,
            assistant_ulaw_path,
            user_file: BufWriter::new(user_file),
            assistant_file: BufWriter::new(assistant_file),
        })
    }

    pub async fn write_user_ulaw_bytes(&mut self, ulaw: &[u8]) -> Result<(), CallRecordingError> {
        self.user_file
            .write_all(ulaw)
            .await
            .map_err(|e| CallRecordingError::Io(e.to_string()))
    }

    #[allow(dead_code)]
    pub async fn write_user_ulaw_b64(&mut self, payload_b64: &str) -> Result<(), CallRecordingError> {
        let ulaw = BASE64
            .decode(payload_b64.as_bytes())
            .map_err(|e| CallRecordingError::Base64(e.to_string()))?;
        self.write_user_ulaw_bytes(&ulaw).await
    }

    pub async fn write_assistant_ulaw_b64(&mut self, payload_b64: &str) -> Result<(), CallRecordingError> {
        let ulaw = BASE64
            .decode(payload_b64.as_bytes())
            .map_err(|e| CallRecordingError::Base64(e.to_string()))?;
        self.assistant_file
            .write_all(&ulaw)
            .await
            .map_err(|e| CallRecordingError::Io(e.to_string()))
    }

    pub async fn finish(mut self) -> Result<CallRecordingArtifacts, CallRecordingError> {
        let _ = self.user_file.flush().await;
        let _ = self.assistant_file.flush().await;

        drop(self.user_file);
        drop(self.assistant_file);

        let user_ulaw = tokio::fs::read(&self.user_ulaw_path)
            .await
            .map_err(|e| CallRecordingError::Io(e.to_string()))?;
        let assistant_ulaw = tokio::fs::read(&self.assistant_ulaw_path)
            .await
            .map_err(|e| CallRecordingError::Io(e.to_string()))?;

        let user_pcm: Vec<i16> = audio::ulaw_to_pcm16(&user_ulaw);
        let assistant_pcm: Vec<i16> = audio::ulaw_to_pcm16(&assistant_ulaw);

        let sample_rate_hz: u32 = 8000;
        let channels: u16 = 2;
        let max_len = user_pcm.len().max(assistant_pcm.len());
        let duration_secs = if max_len == 0 {
            0.0
        } else {
            (max_len as f32) / (sample_rate_hz as f32)
        };

        let wav_path = std::env::temp_dir().join(format!("domingo-{}.wav", self.call_sid));
        write_wav_stereo_pcm16(&wav_path, sample_rate_hz, &user_pcm, &assistant_pcm)
            .await?;

        let meta = tokio::fs::metadata(&wav_path)
            .await
            .map_err(|e| CallRecordingError::Io(e.to_string()))?;

        // best-effort cleanup raw ulaw
        let _ = tokio::fs::remove_file(&self.user_ulaw_path).await;
        let _ = tokio::fs::remove_file(&self.assistant_ulaw_path).await;

        Ok(CallRecordingArtifacts {
            wav_path,
            sample_rate_hz,
            channels,
            duration_secs,
            size_bytes: meta.len(),
        })
    }
}

async fn write_wav_stereo_pcm16(
    path: &Path,
    sample_rate_hz: u32,
    left: &[i16],
    right: &[i16],
) -> Result<(), CallRecordingError> {
    let max_len = left.len().max(right.len());
    let byte_rate = sample_rate_hz * 2 * 2; // sr * channels * bytes_per_sample
    let block_align: u16 = 4; // channels * bytes_per_sample
    let bits_per_sample: u16 = 16;

    // data bytes = frames * block_align
    let data_bytes: u32 = (max_len as u32)
        .checked_mul(block_align as u32)
        .ok_or_else(|| CallRecordingError::Wav("wav too large".to_string()))?;

    let riff_size: u32 = 36 + data_bytes;

    let mut buf = Vec::with_capacity((44 + data_bytes as usize).min(64 * 1024 * 1024));

    // RIFF header
    buf.extend_from_slice(b"RIFF");
    buf.extend_from_slice(&riff_size.to_le_bytes());
    buf.extend_from_slice(b"WAVE");

    // fmt chunk
    buf.extend_from_slice(b"fmt ");
    buf.extend_from_slice(&16u32.to_le_bytes()); // PCM fmt chunk size
    buf.extend_from_slice(&1u16.to_le_bytes()); // audio format = PCM
    buf.extend_from_slice(&2u16.to_le_bytes()); // channels
    buf.extend_from_slice(&sample_rate_hz.to_le_bytes());
    buf.extend_from_slice(&byte_rate.to_le_bytes());
    buf.extend_from_slice(&block_align.to_le_bytes());
    buf.extend_from_slice(&bits_per_sample.to_le_bytes());

    // data chunk
    buf.extend_from_slice(b"data");
    buf.extend_from_slice(&data_bytes.to_le_bytes());

    // interleave samples (pad with silence)
    for i in 0..max_len {
        let l = left.get(i).copied().unwrap_or(0);
        let r = right.get(i).copied().unwrap_or(0);
        buf.extend_from_slice(&l.to_le_bytes());
        buf.extend_from_slice(&r.to_le_bytes());
    }

    tokio::fs::write(path, &buf)
        .await
        .map_err(|e| CallRecordingError::Io(e.to_string()))?;

    Ok(())
}


