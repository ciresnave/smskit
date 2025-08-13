//! # SMS Core
//!
//! Core traits and types for the smskit multi-provider SMS abstraction.
//!
//! This crate provides the fundamental building blocks for SMS operations:
//! - [`SmsClient`] trait for sending SMS messages
//! - [`InboundWebhook`] trait for processing incoming webhooks
//! - Common types for requests, responses, and errors
//!
//! ## Example
//!
//! ```rust,ignore
//! use sms_core::{SendRequest, SmsClient};
//!
//! // Any SMS provider implements SmsClient
//! let response = client.send(SendRequest {
//!     to: "+1234567890",
//!     from: "+0987654321",
//!     text: "Hello world!"
//! }).await?;
//! ```

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use uuid::Uuid;

/// Errors that can occur during SMS operations
#[derive(Debug, thiserror::Error)]
pub enum SmsError {
    /// HTTP communication error
    #[error("http error: {0}")]
    Http(String),
    /// Authentication/authorization error
    #[error("authentication error: {0}")]
    Auth(String),
    /// Invalid request parameters
    #[error("invalid request: {0}")]
    Invalid(String),
    /// SMS provider returned an error
    #[error("provider error: {0}")]
    Provider(String),
    /// Unexpected error occurred
    #[error("unexpected: {0}")]
    Unexpected(String),
}

/// Web-specific error types for webhook processing
#[derive(Debug, thiserror::Error)]
pub enum WebhookError {
    #[error("provider not found: {0}")]
    ProviderNotFound(String),
    #[error("signature verification failed: {0}")]
    VerificationFailed(String),
    #[error("parsing failed: {0}")]
    ParseError(String),
    #[error("SMS processing error: {0}")]
    SmsError(#[from] SmsError),
}

/// HTTP status code for web responses
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HttpStatus {
    Ok = 200,
    BadRequest = 400,
    Unauthorized = 401,
    NotFound = 404,
    InternalServerError = 500,
}

impl HttpStatus {
    pub fn as_u16(self) -> u16 {
        self as u16
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SendRequest<'a> {
    pub to: &'a str,
    pub from: &'a str,
    pub text: &'a str,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SendResponse {
    pub id: String,
    /// Name of the backend/provider that produced the response, e.g. "plivo".
    pub provider: &'static str,
    /// Raw provider payload for debugging / audit.
    pub raw: serde_json::Value,
}

/// Normalized inbound message (e.g., a reply).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InboundMessage {
    pub id: Option<String>,
    pub from: String,
    pub to: String,
    pub text: String,
    pub timestamp: Option<OffsetDateTime>,
    pub provider: &'static str,
    pub raw: serde_json::Value,
}

/// Result of webhook processing, containing both the message and response info
#[derive(Debug, Clone)]
pub struct WebhookResult {
    pub message: InboundMessage,
    pub status: u16,
}

/// Generic webhook response that can be converted to any framework's response type
#[derive(Debug, Clone)]
pub struct WebhookResponse {
    pub status: HttpStatus,
    pub body: String,
    pub content_type: String,
}

impl WebhookResponse {
    pub fn success(message: InboundMessage) -> Self {
        Self {
            status: HttpStatus::Ok,
            body: serde_json::to_string(&message).unwrap_or_else(|_| "{}".to_string()),
            content_type: "application/json".to_string(),
        }
    }

    pub fn error(status: HttpStatus, message: &str) -> Self {
        Self {
            status,
            body: format!(r#"{{"error": "{}"}}"#, message.replace('"', r#"\""#)),
            content_type: "application/json".to_string(),
        }
    }
}

#[async_trait]
pub trait SmsClient: Send + Sync {
    /// Send a single text SMS.
    async fn send(&self, req: SendRequest<'_>) -> Result<SendResponse, SmsError>;
}

/// Utility to create a pseudo id if a provider doesn't return one.
pub fn fallback_id() -> String {
    Uuid::new_v4().to_string()
}

/// Lightweight header representation to avoid tying the core to any HTTP framework.
pub type Headers = Vec<(String, String)>;

/// Provider-agnostic inbound webhook interface.
#[async_trait]
pub trait InboundWebhook: Send + Sync {
    /// Stable provider key, e.g., "plivo", "twilio", etc.
    fn provider(&self) -> &'static str;
    /// Parse the incoming HTTP payload (headers + raw body) into a normalized `InboundMessage`.
    fn parse_inbound(&self, headers: &Headers, body: &[u8]) -> Result<InboundMessage, SmsError>;

    /// Optional signature verification (no-op by default).
    fn verify(&self, _headers: &Headers, _body: &[u8]) -> Result<(), SmsError> {
        Ok(())
    }
}

use std::collections::HashMap;
use std::sync::Arc;

/// Runtime registry so apps can register any combination of providers and treat them interchangeably.
#[derive(Default, Clone)]
pub struct InboundRegistry {
    map: Arc<HashMap<&'static str, Arc<dyn InboundWebhook>>>,
}

impl InboundRegistry {
    pub fn new() -> Self {
        Self {
            map: Arc::new(HashMap::new()),
        }
    }

    pub fn with(mut self, hook: Arc<dyn InboundWebhook>) -> Self {
        let mut m = (*self.map).clone();
        m.insert(hook.provider(), hook);
        self.map = Arc::new(m);
        self
    }

    pub fn get(&self, provider: &str) -> Option<Arc<dyn InboundWebhook>> {
        self.map.get(provider).cloned()
    }
}
