//! Authentication service

use argon2::{
    password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
use chrono::Utc;
use hmac::Mac;
use uuid::Uuid;

use crate::domain::{
    Caregiver, CreateCaregiverRequest, DomainError, DomainResult, LoginRequest, Session,
};
use crate::repositories::postgres::{CaregiverRepository, PostgresPool};

/// Authentication service
pub struct AuthService;

impl AuthService {
    /// Register a new caregiver account
    pub async fn register(
        pool: &PostgresPool,
        req: &CreateCaregiverRequest,
    ) -> DomainResult<Caregiver> {
        // Validate email format
        if !req.email.contains('@') {
            return Err(DomainError::Validation("Invalid email format".to_string()));
        }
        
        // Validate password length
        if req.password.len() < 8 {
            return Err(DomainError::Validation(
                "Password must be at least 8 characters".to_string(),
            ));
        }
        
        // Hash the password
        let password_hash = Self::hash_password(&req.password)?;
        
        // Create the caregiver
        CaregiverRepository::create(pool, req, &password_hash).await
    }
    
    /// Log in a caregiver
    pub async fn login(pool: &PostgresPool, req: &LoginRequest) -> DomainResult<Session> {
        // Find caregiver by email
        let caregiver = CaregiverRepository::find_by_email(pool, &req.email).await?;
        
        // Verify password
        if !Self::verify_password(&req.password, &caregiver.password_hash)? {
            return Err(DomainError::Unauthorized("Invalid password".to_string()));
        }
        
        // Create session
        Ok(Session {
            caregiver_id: caregiver.id,
            role: caregiver.role,
            created_at: Utc::now(),
        })
    }
    
    /// Get caregiver by ID
    pub async fn get_caregiver(pool: &PostgresPool, id: Uuid) -> DomainResult<Caregiver> {
        CaregiverRepository::find_by_id(pool, id).await
    }
    
    /// Hash a password using Argon2
    fn hash_password(password: &str) -> DomainResult<String> {
        let salt = SaltString::generate(&mut OsRng);
        let argon2 = Argon2::default();
        
        argon2
            .hash_password(password.as_bytes(), &salt)
            .map(|h| h.to_string())
            .map_err(|e| DomainError::Validation(format!("Password hashing failed: {}", e)))
    }
    
    /// Verify a password against a hash
    fn verify_password(password: &str, hash: &str) -> DomainResult<bool> {
        let parsed_hash = PasswordHash::new(hash)
            .map_err(|e| DomainError::Validation(format!("Invalid password hash: {}", e)))?;
        
        Ok(Argon2::default()
            .verify_password(password.as_bytes(), &parsed_hash)
            .is_ok())
    }
}

/// Encode session to a secure cookie value
pub fn encode_session(session: &Session, secret: &str) -> String {
    let json = serde_json::to_string(session).unwrap();
    let mut mac = hmac::Hmac::<sha2::Sha256>::new_from_slice(secret.as_bytes()).unwrap();
    hmac::Mac::update(&mut mac, json.as_bytes());
    let signature = hex::encode(hmac::Mac::finalize(mac).into_bytes());
    
    let encoded = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &json);
    format!("{}.{}", encoded, signature)
}

/// Decode and verify session from cookie value
pub fn decode_session(value: &str, secret: &str) -> Option<Session> {
    let parts: Vec<&str> = value.split('.').collect();
    if parts.len() != 2 {
        return None;
    }
    
    let encoded = parts[0];
    let signature = parts[1];
    
    // Decode the JSON
    let json = base64::Engine::decode(&base64::engine::general_purpose::STANDARD, encoded).ok()?;
    let json_str = String::from_utf8(json).ok()?;
    
    // Verify the signature
    let mut mac = hmac::Hmac::<sha2::Sha256>::new_from_slice(secret.as_bytes()).ok()?;
    hmac::Mac::update(&mut mac, json_str.as_bytes());
    let expected = hex::encode(hmac::Mac::finalize(mac).into_bytes());
    
    if signature != expected {
        return None;
    }
    
    // Parse the session
    serde_json::from_str(&json_str).ok()
}

