//! audio utilities for twilio (g711 ulaw @ 8khz) <-> gemini live (pcm16 @ 16khz in / 24khz out)
//!
//! this is intentionally dependency-free: simple g.711 µ-law codec + small-ratio resampling.
//!
//! ## Gemini Live Audio Chunk Size Requirements
//!
//! Per Google's best practices, audio should be sent in chunks of 20-40ms.
//! At 16kHz PCM16:
//!   - 20ms = 320 samples = 640 bytes
//!   - 40ms = 640 samples = 1280 bytes
//!
//! Twilio sends 20ms chunks at 8kHz (160 bytes ulaw), which after upsampling
//! to 16kHz becomes exactly 640 bytes (20ms) - this is optimal.

use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};

/// Minimum recommended chunk size for Gemini Live input (20ms @ 16kHz)
pub const GEMINI_MIN_CHUNK_BYTES: usize = 640;

/// Maximum recommended chunk size for Gemini Live input (40ms @ 16kHz)
pub const GEMINI_MAX_CHUNK_BYTES: usize = 1280;

/// Twilio's standard chunk size (20ms @ 8kHz ulaw)
pub const TWILIO_CHUNK_BYTES: usize = 160;

/// Audio batcher that accumulates PCM16 bytes until minimum chunk size is reached.
/// Ensures we always send at least 20ms chunks to Gemini Live per best practices.
#[derive(Debug, Default)]
pub struct GeminiAudioBatcher {
    buffer: Vec<u8>,
}

impl GeminiAudioBatcher {
    pub fn new() -> Self {
        Self {
            buffer: Vec::with_capacity(GEMINI_MAX_CHUNK_BYTES),
        }
    }

    /// Add PCM16 bytes to the buffer. Returns chunks of at least GEMINI_MIN_CHUNK_BYTES
    /// when enough data has accumulated. May return multiple chunks if buffer is large.
    pub fn push(&mut self, pcm16_bytes: &[u8]) -> Vec<Vec<u8>> {
        self.buffer.extend_from_slice(pcm16_bytes);

        let mut chunks = Vec::new();

        // Emit chunks of exactly GEMINI_MIN_CHUNK_BYTES (20ms) when we have enough
        while self.buffer.len() >= GEMINI_MIN_CHUNK_BYTES {
            let chunk: Vec<u8> = self.buffer.drain(..GEMINI_MIN_CHUNK_BYTES).collect();
            chunks.push(chunk);
        }

        chunks
    }

    /// Flush any remaining buffered audio (may be less than minimum chunk size).
    /// Call this when the stream ends to avoid losing trailing audio.
    pub fn flush(&mut self) -> Option<Vec<u8>> {
        if self.buffer.is_empty() {
            None
        } else {
            Some(std::mem::take(&mut self.buffer))
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum AudioError {
    #[error("base64 decode error: {0}")]
    Base64Decode(String),
    #[error("invalid pcm16 byte length: {0}")]
    InvalidPcm16Length(usize),
}

/// twilio media payload (base64 of 8khz g711 ulaw bytes) -> gemini input bytes (pcm16le 16khz)
pub fn twilio_ulaw_base64_to_gemini_pcm16_16khz_bytes(payload_b64: &str) -> Result<Vec<u8>, AudioError> {
    let ulaw = BASE64
        .decode(payload_b64.as_bytes())
        .map_err(|e| AudioError::Base64Decode(e.to_string()))?;

    let pcm8 = ulaw_to_pcm16(&ulaw);
    let pcm16k = upsample_8khz_to_16khz(&pcm8);
    Ok(i16_to_le_bytes(&pcm16k))
}

/// gemini output bytes (pcm16le 24khz) -> twilio media payload (base64 of 8khz g711 ulaw bytes)
pub fn gemini_pcm16_24khz_bytes_to_twilio_ulaw_base64(pcm24_le: &[u8]) -> Result<String, AudioError> {
    let pcm24 = le_bytes_to_i16(pcm24_le)?;
    let pcm8 = downsample_24khz_to_8khz(&pcm24);
    let ulaw = pcm16_to_ulaw(&pcm8);
    Ok(BASE64.encode(ulaw))
}

/// convert ulaw bytes to pcm16 samples (@8khz)
pub fn ulaw_to_pcm16(ulaw: &[u8]) -> Vec<i16> {
    ulaw.iter().map(|&b| ulaw_decode(b)).collect()
}

/// convert pcm16 samples to ulaw bytes (@8khz)
pub fn pcm16_to_ulaw(pcm: &[i16]) -> Vec<u8> {
    pcm.iter().map(|&s| ulaw_encode(s)).collect()
}

/// upsample 8khz -> 16khz (x2). simple linear interpolation.
pub fn upsample_8khz_to_16khz(input: &[i16]) -> Vec<i16> {
    if input.is_empty() {
        return vec![];
    }
    let mut out = Vec::with_capacity(input.len() * 2);
    for i in 0..input.len() {
        let s0 = input[i] as i32;
        out.push(input[i]);
        let s1 = if i + 1 < input.len() { input[i + 1] as i32 } else { s0 };
        out.push(((s0 + s1) / 2) as i16);
    }
    out
}

/// downsample 24khz -> 8khz (/3). simple averaging.
pub fn downsample_24khz_to_8khz(input: &[i16]) -> Vec<i16> {
    if input.is_empty() {
        return vec![];
    }
    let mut out = Vec::with_capacity((input.len() + 2) / 3);
    let mut i = 0;
    while i < input.len() {
        let end = (i + 3).min(input.len());
        let mut acc: i32 = 0;
        for s in &input[i..end] {
            acc += *s as i32;
        }
        out.push((acc / ((end - i) as i32)) as i16);
        i += 3;
    }
    out
}

pub fn i16_to_le_bytes(samples: &[i16]) -> Vec<u8> {
    let mut out = Vec::with_capacity(samples.len() * 2);
    for &s in samples {
        out.extend_from_slice(&s.to_le_bytes());
    }
    out
}

pub fn le_bytes_to_i16(bytes: &[u8]) -> Result<Vec<i16>, AudioError> {
    if bytes.len() % 2 != 0 {
        return Err(AudioError::InvalidPcm16Length(bytes.len()));
    }
    let mut out = Vec::with_capacity(bytes.len() / 2);
    for chunk in bytes.chunks_exact(2) {
        out.push(i16::from_le_bytes([chunk[0], chunk[1]]));
    }
    Ok(out)
}

// -----------------------------
// g.711 µ-law codec (itu-t g.711)
// -----------------------------

const ULAW_BIAS: i32 = 0x84;
const ULAW_CLIP: i32 = 32635;
const ULAW_SEG_END: [i32; 8] = [0xFF, 0x1FF, 0x3FF, 0x7FF, 0xFFF, 0x1FFF, 0x3FFF, 0x7FFF];

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

fn ulaw_encode(sample: i16) -> u8 {
    let mut pcm = sample as i32;
    let sign = if pcm < 0 {
        pcm = -pcm;
        0x80
    } else {
        0
    };

    if pcm > ULAW_CLIP {
        pcm = ULAW_CLIP;
    }

    pcm += ULAW_BIAS;

    // find segment
    let mut exponent: i32 = 0;
    while exponent < 8 && pcm > ULAW_SEG_END[exponent as usize] {
        exponent += 1;
    }
    if exponent > 7 {
        exponent = 7;
    }

    let mantissa = (pcm >> (exponent + 3)) & 0x0F;
    let ulaw = !(sign | (exponent << 4) | mantissa) as u8;
    ulaw
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ulaw_roundtrip_sanity() {
        // not perfect (lossy), but should be stable and finite.
        let samples: [i16; 9] = [-32000, -10000, -1000, -1, 0, 1, 1000, 10000, 32000];
        for &s in &samples {
            let u = ulaw_encode(s);
            let d = ulaw_decode(u);
            // decoded should generally preserve sign, except that very small magnitudes may quantize to 0.
            if s != 0 && d != 0 {
                assert_eq!(s.is_negative(), d.is_negative());
            }
        }
    }

    #[test]
    fn resample_sizes() {
        let in8 = vec![0i16; 160]; // 20ms @ 8khz
        let out16 = upsample_8khz_to_16khz(&in8);
        assert_eq!(out16.len(), 320);

        let in24 = vec![0i16; 240]; // 10ms @ 24khz
        let out8 = downsample_24khz_to_8khz(&in24);
        assert_eq!(out8.len(), 80);
    }
}


