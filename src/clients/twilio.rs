//! Twilio API client for telephony operations

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use serde::{Deserialize, Serialize};

/// Twilio API client
pub struct TwilioClient {
    account_sid: String,
    auth_token: String,
    phone_number: String,
    http_client: reqwest::Client,
}

impl TwilioClient {
    /// Create a new Twilio client
    pub fn new(account_sid: &str, auth_token: &str, phone_number: &str) -> Self {
        Self {
            account_sid: account_sid.to_string(),
            auth_token: auth_token.to_string(),
            phone_number: phone_number.to_string(),
            http_client: reqwest::Client::new(),
        }
    }
    
    /// Get the Twilio phone number
    pub fn phone_number(&self) -> &str {
        &self.phone_number
    }
    
    /// Generate Basic auth header
    fn auth_header(&self) -> String {
        let credentials = format!("{}:{}", self.account_sid, self.auth_token);
        format!("Basic {}", BASE64.encode(credentials.as_bytes()))
    }
    
    /// Make an outbound call
    pub async fn make_call(
        &self,
        to: &str,
        twiml_url: &str,
    ) -> Result<CallResponse, TwilioError> {
        let url = format!(
            "https://api.twilio.com/2010-04-01/Accounts/{}/Calls.json",
            self.account_sid
        );
        
        let params = [
            ("To", to),
            ("From", &self.phone_number),
            ("Url", twiml_url),
        ];
        
        let response = self.http_client
            .post(&url)
            .header("Authorization", self.auth_header())
            .form(&params)
            .send()
            .await
            .map_err(|e| TwilioError::Request(e.to_string()))?;
        
        if !response.status().is_success() {
            let error_text = response.text().await.unwrap_or_default();
            return Err(TwilioError::Api(error_text));
        }
        
        response.json().await.map_err(|e| TwilioError::Parse(e.to_string()))
    }
    
    /// Update an existing call (e.g., redirect to new TwiML)
    pub async fn update_call(
        &self,
        call_sid: &str,
        twiml: &str,
    ) -> Result<CallResponse, TwilioError> {
        let url = format!(
            "https://api.twilio.com/2010-04-01/Accounts/{}/Calls/{}.json",
            self.account_sid, call_sid
        );
        
        let params = [
            ("Twiml", twiml),
        ];
        
        let response = self.http_client
            .post(&url)
            .header("Authorization", self.auth_header())
            .form(&params)
            .send()
            .await
            .map_err(|e| TwilioError::Request(e.to_string()))?;
        
        if !response.status().is_success() {
            let error_text = response.text().await.unwrap_or_default();
            return Err(TwilioError::Api(error_text));
        }
        
        response.json().await.map_err(|e| TwilioError::Parse(e.to_string()))
    }
    
    /// Send an SMS
    pub async fn send_sms(
        &self,
        to: &str,
        body: &str,
    ) -> Result<SmsResponse, TwilioError> {
        let url = format!(
            "https://api.twilio.com/2010-04-01/Accounts/{}/Messages.json",
            self.account_sid
        );
        
        let params = [
            ("To", to),
            ("From", &self.phone_number),
            ("Body", body),
        ];
        
        let response = self.http_client
            .post(&url)
            .header("Authorization", self.auth_header())
            .form(&params)
            .send()
            .await
            .map_err(|e| TwilioError::Request(e.to_string()))?;
        
        if !response.status().is_success() {
            let error_text = response.text().await.unwrap_or_default();
            return Err(TwilioError::Api(error_text));
        }
        
        response.json().await.map_err(|e| TwilioError::Parse(e.to_string()))
    }
    
    /// Generate TwiML for streaming audio to our WebSocket
    pub fn generate_stream_twiml(&self, stream_url: &str) -> String {
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<Response>
    <Connect>
        <Stream url="{}" />
    </Connect>
</Response>"#,
            stream_url
        )
    }
    
    /// Generate TwiML for a medication reminder call (Spanish - Mexico)
    pub fn generate_reminder_twiml(&self, medication_name: &str, dosage: &str, reminder_id: &str) -> String {
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<Response>
    <Say voice="Polly.Mia" language="es-MX">¡Hola! Este es tu recordatorio de medicamento. Es hora de tomar tu {} - {}. Por favor presiona 1 después de tomar tu medicamento.</Say>
    <Gather numDigits="1" action="/api/twilio/reminder-confirm?reminder_id={}" method="POST">
        <Say voice="Polly.Mia" language="es-MX">Presiona 1 para confirmar que tomaste tu medicamento.</Say>
    </Gather>
    <Say voice="Polly.Mia" language="es-MX">No recibimos tu respuesta. Por favor recuerda tomar tu medicamento. Hasta luego.</Say>
</Response>"#,
            medication_name, dosage, reminder_id
        )
    }
    
    /// Generate TwiML for dialing a contact (Spanish - Mexico)
    pub fn generate_dial_twiml(&self, phone_number: &str) -> String {
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<Response>
    <Say voice="Polly.Mia" language="es-MX">Conectándote ahora.</Say>
    <Dial>{}</Dial>
</Response>"#,
            phone_number
        )
    }
    
    /// Generate TwiML for an error/fallback message (Spanish - Mexico)
    pub fn generate_error_twiml(&self, message: &str) -> String {
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<Response>
    <Say voice="Polly.Mia" language="es-MX">{}</Say>
    <Hangup/>
</Response>"#,
            message
        )
    }
}

/// Twilio API errors
#[derive(Debug, thiserror::Error)]
pub enum TwilioError {
    #[error("Request error: {0}")]
    Request(String),
    
    #[error("API error: {0}")]
    Api(String),
    
    #[error("Parse error: {0}")]
    Parse(String),
}

/// Twilio call response
#[derive(Debug, Deserialize)]
pub struct CallResponse {
    pub sid: String,
    pub status: String,
    pub to: String,
    pub from: String,
}

/// Twilio SMS response
#[derive(Debug, Deserialize)]
pub struct SmsResponse {
    pub sid: String,
    pub status: String,
    pub to: String,
    pub from: String,
}

/// Incoming call webhook payload from Twilio
#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
#[allow(dead_code)]
pub struct IncomingCallPayload {
    pub call_sid: String,
    pub from: String,
    pub to: String,
    pub call_status: String,
    pub direction: String,
}

/// Call status webhook payload from Twilio
#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
#[allow(dead_code)]
pub struct CallStatusPayload {
    pub call_sid: String,
    pub call_status: String,
    pub call_duration: Option<String>,
}

/// Twilio media stream message types
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "event")]
#[serde(rename_all = "lowercase")]
pub enum TwilioStreamMessage {
    Connected {
        protocol: String,
        version: String,
    },
    Start {
        #[serde(rename = "streamSid")]
        stream_sid: String,
        start: TwilioStreamStart,
    },
    Media {
        #[serde(rename = "streamSid")]
        stream_sid: String,
        media: TwilioStreamMedia,
    },
    Stop {
        #[serde(rename = "streamSid")]
        stream_sid: String,
    },
    Mark {
        #[serde(rename = "streamSid")]
        stream_sid: String,
        mark: TwilioStreamMark,
    },
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TwilioStreamStart {
    #[serde(rename = "streamSid")]
    pub stream_sid: String,
    #[serde(rename = "accountSid")]
    pub account_sid: String,
    #[serde(rename = "callSid")]
    pub call_sid: String,
    pub tracks: Vec<String>,
    #[serde(rename = "mediaFormat")]
    pub media_format: TwilioMediaFormat,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TwilioMediaFormat {
    pub encoding: String,
    #[serde(rename = "sampleRate")]
    pub sample_rate: i32,
    pub channels: i32,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TwilioStreamMedia {
    pub track: String,
    pub chunk: String,
    pub timestamp: String,
    pub payload: String, // base64 encoded audio
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TwilioStreamMark {
    pub name: String,
}

/// Outbound media message to send audio to Twilio
#[derive(Debug, Serialize)]
pub struct TwilioOutboundMedia {
    pub event: String,
    #[serde(rename = "streamSid")]
    pub stream_sid: String,
    pub media: TwilioOutboundMediaPayload,
}

#[derive(Debug, Serialize)]
pub struct TwilioOutboundMediaPayload {
    pub payload: String, // base64 encoded audio
}

impl TwilioOutboundMedia {
    pub fn new(stream_sid: &str, audio_payload: &str) -> Self {
        Self {
            event: "media".to_string(),
            stream_sid: stream_sid.to_string(),
            media: TwilioOutboundMediaPayload {
                payload: audio_payload.to_string(),
            },
        }
    }
}

/// Outbound "clear" message to drop any buffered audio on Twilio's side (barge-in).
#[derive(Debug, Serialize)]
pub struct TwilioOutboundClear {
    pub event: String,
    #[serde(rename = "streamSid")]
    pub stream_sid: String,
}

impl TwilioOutboundClear {
    pub fn new(stream_sid: &str) -> Self {
        Self {
            event: "clear".to_string(),
            stream_sid: stream_sid.to_string(),
        }
    }
}

