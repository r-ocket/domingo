//! call audio recording (twilio media stream) -> compressed-ish wav for storage/playback

use std::path::PathBuf;

use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use tokio::io::{AsyncWriteExt, BufWriter};


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
    pub bits_per_sample: u16,
    pub duration_secs: f32,
    pub size_bytes: u64,
}

/// streams ulaw bytes to temp files, then builds a compact wav at the end.
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

        // Compress: output mono 8-bit PCM WAV @ 8kHz.
        // This is ~4x smaller than our previous 16-bit stereo PCM WAV.
        // (Still browser-playable; and avoids requiring external encoders.)
        let sample_rate_hz: u32 = 8000;
        let channels: u16 = 1;
        let bits_per_sample: u16 = 8;

        // We only need file lengths up front to write a correct WAV header.
        let user_len = tokio::fs::metadata(&self.user_ulaw_path)
            .await
            .map_err(|e| CallRecordingError::Io(e.to_string()))?
            .len() as usize;
        let assistant_len = tokio::fs::metadata(&self.assistant_ulaw_path)
            .await
            .map_err(|e| CallRecordingError::Io(e.to_string()))?
            .len() as usize;
        let max_len = user_len.max(assistant_len);

        let duration_secs = if max_len == 0 {
            0.0
        } else {
            (max_len as f32) / (sample_rate_hz as f32)
        };

        let wav_path = std::env::temp_dir().join(format!("domingo-{}.wav", self.call_sid));
        write_wav_pcm8_mono_from_ulaw_files(
            &wav_path,
            &self.user_ulaw_path,
            &self.assistant_ulaw_path,
            sample_rate_hz,
            max_len,
        )
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
            bits_per_sample,
            duration_secs,
            size_bytes: meta.len(),
        })
    }
}

async fn write_wav_pcm8_mono_from_ulaw_files(
    wav_path: &std::path::Path,
    user_ulaw_path: &std::path::Path,
    assistant_ulaw_path: &std::path::Path,
    sample_rate_hz: u32,
    max_samples: usize,
) -> Result<(), CallRecordingError> {
    let wav_path = wav_path.to_path_buf();
    let user_path = user_ulaw_path.to_path_buf();
    let assistant_path = assistant_ulaw_path.to_path_buf();

    tokio::task::spawn_blocking(move || -> Result<(), CallRecordingError> {
        use std::fs::File;
        use std::io::{BufReader, BufWriter as StdBufWriter, Read, Write};

        let mut user = BufReader::new(File::open(user_path).map_err(|e| CallRecordingError::Io(e.to_string()))?);
        let mut asst = BufReader::new(File::open(assistant_path).map_err(|e| CallRecordingError::Io(e.to_string()))?);
        let mut out = StdBufWriter::new(File::create(wav_path).map_err(|e| CallRecordingError::Io(e.to_string()))?);

        let channels: u16 = 1;
        let bits_per_sample: u16 = 8;
        let bytes_per_sample: u16 = 1;
        let block_align: u16 = channels * bytes_per_sample;
        let byte_rate: u32 = sample_rate_hz * (block_align as u32);

        let data_bytes: u32 = (max_samples as u32)
            .checked_mul(block_align as u32)
            .ok_or_else(|| CallRecordingError::Wav("wav too large".to_string()))?;
        let riff_size: u32 = 36 + data_bytes;

        // header
        out.write_all(b"RIFF").map_err(|e| CallRecordingError::Io(e.to_string()))?;
        out.write_all(&riff_size.to_le_bytes()).map_err(|e| CallRecordingError::Io(e.to_string()))?;
        out.write_all(b"WAVE").map_err(|e| CallRecordingError::Io(e.to_string()))?;

        out.write_all(b"fmt ").map_err(|e| CallRecordingError::Io(e.to_string()))?;
        out.write_all(&16u32.to_le_bytes()).map_err(|e| CallRecordingError::Io(e.to_string()))?; // fmt chunk size
        out.write_all(&1u16.to_le_bytes()).map_err(|e| CallRecordingError::Io(e.to_string()))?; // PCM
        out.write_all(&channels.to_le_bytes()).map_err(|e| CallRecordingError::Io(e.to_string()))?;
        out.write_all(&sample_rate_hz.to_le_bytes()).map_err(|e| CallRecordingError::Io(e.to_string()))?;
        out.write_all(&byte_rate.to_le_bytes()).map_err(|e| CallRecordingError::Io(e.to_string()))?;
        out.write_all(&block_align.to_le_bytes()).map_err(|e| CallRecordingError::Io(e.to_string()))?;
        out.write_all(&bits_per_sample.to_le_bytes()).map_err(|e| CallRecordingError::Io(e.to_string()))?;

        out.write_all(b"data").map_err(|e| CallRecordingError::Io(e.to_string()))?;
        out.write_all(&data_bytes.to_le_bytes()).map_err(|e| CallRecordingError::Io(e.to_string()))?;

        // stream decode+mix
        let mut remaining = max_samples;
        let mut buf_u = [0u8; 4096];
        let mut buf_a = [0u8; 4096];

        while remaining > 0 {
            let want = remaining.min(buf_u.len());

            let mut got_u = 0usize;
            while got_u < want {
                let n = user.read(&mut buf_u[got_u..want]).map_err(|e| CallRecordingError::Io(e.to_string()))?;
                if n == 0 { break; }
                got_u += n;
            }

            let mut got_a = 0usize;
            while got_a < want {
                let n = asst.read(&mut buf_a[got_a..want]).map_err(|e| CallRecordingError::Io(e.to_string()))?;
                if n == 0 { break; }
                got_a += n;
            }

            let chunk_len = want;
            for i in 0..chunk_len {
                let su = if i < got_u { ulaw_decode(buf_u[i]) } else { 0 };
                let sa = if i < got_a { ulaw_decode(buf_a[i]) } else { 0 };
                let mixed = ((su as i32) + (sa as i32)) / 2;
                let mixed = mixed.clamp(-32768, 32767) as i16;
                let pcm8 = pcm16_to_pcm8_u(mixed);
                out.write_all(&[pcm8]).map_err(|e| CallRecordingError::Io(e.to_string()))?;
            }

            remaining -= chunk_len;
        }

        out.flush().map_err(|e| CallRecordingError::Io(e.to_string()))?;
        Ok(())
    })
    .await
    .map_err(|e| CallRecordingError::Io(e.to_string()))?
}

fn pcm16_to_pcm8_u(s: i16) -> u8 {
    // WAV PCM 8-bit is unsigned.
    let v = (s as i32) + 32768;
    ((v >> 8) & 0xFF) as u8
}

// duplicated µ-law decode (same as src/clients/audio.rs, but that function is private there)
const ULAW_BIAS: i32 = 0x84;
fn ulaw_decode(ulaw: u8) -> i16 {
    let u = (!ulaw) as i32;
    let sign = u & 0x80;
    let exponent = (u >> 4) & 0x07;
    let mantissa = u & 0x0F;

    let mut t = ((mantissa << 3) + ULAW_BIAS) << exponent;
    t -= ULAW_BIAS;

    let sample = if sign != 0 { -t } else { t };
    sample as i16
}


