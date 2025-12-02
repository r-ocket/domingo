//! Uber API client for ride requests

use serde::{Deserialize, Serialize};

/// Uber API client
pub struct UberClient {
    client_id: String,
    client_secret: String,
    http_client: reqwest::Client,
    access_token: std::sync::RwLock<Option<String>>,
}

impl UberClient {
    /// Create a new Uber client
    pub fn new(client_id: &str, client_secret: &str) -> Self {
        Self {
            client_id: client_id.to_string(),
            client_secret: client_secret.to_string(),
            http_client: reqwest::Client::new(),
            access_token: std::sync::RwLock::new(None),
        }
    }
    
    /// Get or refresh access token
    async fn get_access_token(&self) -> Result<String, UberError> {
        // Check if we have a cached token
        if let Some(token) = self.access_token.read().unwrap().clone() {
            return Ok(token);
        }
        
        // Get new token using client credentials
        let response = self.http_client
            .post("https://login.uber.com/oauth/v2/token")
            .form(&[
                ("client_id", &self.client_id),
                ("client_secret", &self.client_secret),
                ("grant_type", &"client_credentials".to_string()),
                ("scope", &"ride_request.all".to_string()),
            ])
            .send()
            .await
            .map_err(|e| UberError::Request(e.to_string()))?;
        
        if !response.status().is_success() {
            let error_text = response.text().await.unwrap_or_default();
            return Err(UberError::Api(error_text));
        }
        
        let token_response: TokenResponse = response.json()
            .await
            .map_err(|e| UberError::Parse(e.to_string()))?;
        
        // Cache the token
        *self.access_token.write().unwrap() = Some(token_response.access_token.clone());
        
        Ok(token_response.access_token)
    }
    
    /// Request a ride
    pub async fn request_ride(
        &self,
        pickup_lat: f64,
        pickup_lng: f64,
        dropoff_lat: f64,
        dropoff_lng: f64,
        rider_phone: &str,
    ) -> Result<RideResponse, UberError> {
        let token = self.get_access_token().await?;
        
        let request = RideRequest {
            fare_id: None,
            product_id: None, // Will use default product
            start_latitude: pickup_lat,
            start_longitude: pickup_lng,
            end_latitude: dropoff_lat,
            end_longitude: dropoff_lng,
            rider_phone_number: Some(rider_phone.to_string()),
        };
        
        let response = self.http_client
            .post("https://api.uber.com/v1.2/requests")
            .bearer_auth(&token)
            .json(&request)
            .send()
            .await
            .map_err(|e| UberError::Request(e.to_string()))?;
        
        if !response.status().is_success() {
            let error_text = response.text().await.unwrap_or_default();
            return Err(UberError::Api(error_text));
        }
        
        response.json().await.map_err(|e| UberError::Parse(e.to_string()))
    }
    
    /// Get ride estimate
    pub async fn get_estimate(
        &self,
        pickup_lat: f64,
        pickup_lng: f64,
        dropoff_lat: f64,
        dropoff_lng: f64,
    ) -> Result<EstimateResponse, UberError> {
        let token = self.get_access_token().await?;
        
        let url = format!(
            "https://api.uber.com/v1.2/estimates/price?start_latitude={}&start_longitude={}&end_latitude={}&end_longitude={}",
            pickup_lat, pickup_lng, dropoff_lat, dropoff_lng
        );
        
        let response = self.http_client
            .get(&url)
            .bearer_auth(&token)
            .send()
            .await
            .map_err(|e| UberError::Request(e.to_string()))?;
        
        if !response.status().is_success() {
            let error_text = response.text().await.unwrap_or_default();
            return Err(UberError::Api(error_text));
        }
        
        response.json().await.map_err(|e| UberError::Parse(e.to_string()))
    }
    
    /// Get ride status
    pub async fn get_ride_status(&self, ride_id: &str) -> Result<RideResponse, UberError> {
        let token = self.get_access_token().await?;
        
        let url = format!("https://api.uber.com/v1.2/requests/{}", ride_id);
        
        let response = self.http_client
            .get(&url)
            .bearer_auth(&token)
            .send()
            .await
            .map_err(|e| UberError::Request(e.to_string()))?;
        
        if !response.status().is_success() {
            let error_text = response.text().await.unwrap_or_default();
            return Err(UberError::Api(error_text));
        }
        
        response.json().await.map_err(|e| UberError::Parse(e.to_string()))
    }
    
    /// Cancel a ride
    pub async fn cancel_ride(&self, ride_id: &str) -> Result<(), UberError> {
        let token = self.get_access_token().await?;
        
        let url = format!("https://api.uber.com/v1.2/requests/{}", ride_id);
        
        let response = self.http_client
            .delete(&url)
            .bearer_auth(&token)
            .send()
            .await
            .map_err(|e| UberError::Request(e.to_string()))?;
        
        if !response.status().is_success() {
            let error_text = response.text().await.unwrap_or_default();
            return Err(UberError::Api(error_text));
        }
        
        Ok(())
    }
}

/// Uber API errors
#[derive(Debug, thiserror::Error)]
pub enum UberError {
    #[error("Request error: {0}")]
    Request(String),
    
    #[error("API error: {0}")]
    Api(String),
    
    #[error("Parse error: {0}")]
    Parse(String),
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct TokenResponse {
    access_token: String,
    token_type: String,
    expires_in: i32,
}

#[derive(Debug, Serialize)]
struct RideRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    fare_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    product_id: Option<String>,
    start_latitude: f64,
    start_longitude: f64,
    end_latitude: f64,
    end_longitude: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    rider_phone_number: Option<String>,
}

/// Ride response from Uber API
#[derive(Debug, Clone, Deserialize)]
pub struct RideResponse {
    pub request_id: String,
    pub status: String,
    #[serde(default)]
    pub driver: Option<DriverInfo>,
    #[serde(default)]
    pub vehicle: Option<VehicleInfo>,
    #[serde(default)]
    pub eta: Option<i32>,
    #[serde(default)]
    pub surge_multiplier: Option<f64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DriverInfo {
    pub name: String,
    pub phone_number: String,
    pub rating: f64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct VehicleInfo {
    pub make: String,
    pub model: String,
    pub license_plate: String,
}

/// Estimate response from Uber API
#[derive(Debug, Deserialize)]
pub struct EstimateResponse {
    pub prices: Vec<PriceEstimate>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PriceEstimate {
    pub product_id: String,
    pub display_name: String,
    pub estimate: String,
    pub currency_code: String,
    pub duration: i32,
    pub distance: f64,
}

/// Webhook event from Uber
#[derive(Debug, Deserialize)]
pub struct UberWebhookEvent {
    pub event_type: String,
    pub event_time: i64,
    pub meta: UberWebhookMeta,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct UberWebhookMeta {
    pub resource_id: String,
    pub status: Option<String>,
    pub user_id: Option<String>,
}

/// Map Uber status to our domain status
impl RideResponse {
    pub fn to_domain_status(&self) -> crate::domain::RideStatus {
        match self.status.as_str() {
            "processing" | "pending" => crate::domain::RideStatus::Requested,
            "accepted" => crate::domain::RideStatus::DriverAssigned,
            "arriving" => crate::domain::RideStatus::Arriving,
            "in_progress" => crate::domain::RideStatus::InProgress,
            "completed" => crate::domain::RideStatus::Completed,
            "driver_canceled" | "rider_canceled" => crate::domain::RideStatus::Canceled,
            _ => crate::domain::RideStatus::Failed,
        }
    }
}

