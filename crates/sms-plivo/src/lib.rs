//! # Plivo SMS Provider
//!
//! [Plivo](https://www.plivo.com/) backend for the smskit multi-provider SMS
//! abstraction.
//!
//! ## Sending messages
//!
//! ```rust,ignore
//! use sms_core::{SendRequest, SmsClient};
//! use sms_plivo::PlivoClient;
//!
//! let client = PlivoClient::new("YOUR_AUTH_ID", "YOUR_AUTH_TOKEN");
//! let response = client.send(SendRequest {
//!     to: "+14155551234",
//!     from: "+10005551234",
//!     text: "Hello from Plivo!",
//! }).await?;
//! println!("Message ID: {}", response.id);
//! ```
//!
//! ## Creating from environment variables
//!
//! ```rust,ignore
//! let client = PlivoClient::from_env()?;
//! ```
//!
//! Reads `PLIVO_AUTH_ID` and `PLIVO_AUTH_TOKEN` from the environment.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use sms_core::{InboundMessage, SendRequest, SendResponse, SmsClient, SmsError};

const PROVIDER: &str = "plivo";

/// Plivo REST API client.
///
/// Implements [`SmsClient`] for sending SMS and [`InboundWebhook`] for
/// receiving inbound messages.
///
/// # Construction
///
/// | Method | Description |
/// |--------|-------------|
/// | [`PlivoClient::new`] | Provide credentials directly |
/// | [`PlivoClient::from_env`] | Read `PLIVO_AUTH_ID` / `PLIVO_AUTH_TOKEN` from the environment |
/// | [`PlivoClient::with_base_url`] | Override the API base URL (useful for testing) |
#[derive(Clone, Debug)]
pub struct PlivoClient {
    /// Plivo Auth ID (account SID).
    pub auth_id: String,
    /// Plivo Auth Token (used for Basic-auth on every request).
    pub auth_token: String,
    /// API base URL; override with [`with_base_url`](PlivoClient::with_base_url)
    /// for testing against a mock server.
    pub base_url: String,
    #[cfg(feature = "reqwest")]
    http: reqwest::Client,
}

impl PlivoClient {
    /// Create a new client with explicit credentials.
    ///
    /// Connects to the production Plivo API at `https://api.plivo.com`.
    ///
    /// # Arguments
    ///
    /// * `auth_id`    - Your Plivo Auth ID (found on the Plivo dashboard).
    /// * `auth_token` - Your Plivo Auth Token.
    pub fn new<S: Into<String>>(auth_id: S, auth_token: S) -> Self {
        Self::with_base_url(auth_id, auth_token, "https://api.plivo.com".to_string())
    }

    /// Create a new client by reading credentials from environment variables.
    ///
    /// | Variable           | Maps to       |
    /// |--------------------|---------------|
    /// | `PLIVO_AUTH_ID`    | `auth_id`     |
    /// | `PLIVO_AUTH_TOKEN` | `auth_token`  |
    ///
    /// Returns [`SmsError::Auth`] if either variable is missing.
    pub fn from_env() -> Result<Self, SmsError> {
        let auth_id = std::env::var("PLIVO_AUTH_ID")
            .map_err(|_| SmsError::Auth("PLIVO_AUTH_ID not set".into()))?;
        let auth_token = std::env::var("PLIVO_AUTH_TOKEN")
            .map_err(|_| SmsError::Auth("PLIVO_AUTH_TOKEN not set".into()))?;
        Ok(Self::new(auth_id, auth_token))
    }

    /// Create a client with a custom API base URL.
    ///
    /// Primarily useful for integration tests where you point at a mock HTTP
    /// server instead of Plivo's production API.
    pub fn with_base_url<S: Into<String>>(auth_id: S, auth_token: S, base_url: String) -> Self {
        Self {
            auth_id: auth_id.into(),
            auth_token: auth_token.into(),
            base_url,
            #[cfg(feature = "reqwest")]
            http: reqwest::Client::new(),
        }
    }
}

/// Wire format for the Plivo send-message request body.
#[derive(Debug, Serialize)]
struct PlivoSendRequest<'a> {
    src: &'a str,
    dst: &'a str,
    text: &'a str,
}

#[async_trait]
impl SmsClient for PlivoClient {
    async fn send(&self, req: SendRequest<'_>) -> Result<SendResponse, SmsError> {
        #[cfg(not(feature = "reqwest"))]
        {
            let _ = req;
            return Err(SmsError::Unexpected("reqwest feature disabled".into()));
        }
        #[cfg(feature = "reqwest")]
        {
            let url = format!(
                "{}/v1/Account/{}/Message/",
                self.base_url.trim_end_matches('/'),
                self.auth_id
            );
            let payload = PlivoSendRequest {
                src: req.from,
                dst: req.to,
                text: req.text,
            };
            let res = self
                .http
                .post(url)
                .basic_auth(&self.auth_id, Some(&self.auth_token))
                .json(&payload)
                .send()
                .await
                .map_err(|e| SmsError::Http(e.to_string()))?;

            if !res.status().is_success() {
                let status = res.status();
                let body = res.text().await.unwrap_or_default();
                return Err(SmsError::Provider(format!("HTTP {}: {}", status, body)));
            }

            let raw_text = res
                .text()
                .await
                .map_err(|e| SmsError::Http(e.to_string()))?;
            let raw_json: serde_json::Value = serde_json::from_str(&raw_text)
                .unwrap_or_else(|_| serde_json::json!({ "raw": raw_text }));

            let id = raw_json
                .get("message_uuid")
                .and_then(|v| v.as_array())
                .and_then(|arr| arr.first())
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .unwrap_or_else(sms_core::fallback_id);

            Ok(SendResponse {
                id,
                provider: PROVIDER,
                raw: raw_json,
            })
        }
    }
}

/// The raw form-encoded payload that Plivo POSTs to your webhook URL when an
/// inbound SMS arrives.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PlivoInbound {
    /// Sender phone number.
    #[serde(rename = "From")]
    pub from: String,
    /// Destination number (your Plivo number).
    #[serde(rename = "To")]
    pub to: String,
    /// Message body.
    #[serde(rename = "Text")]
    pub text: String,
    /// Message type (usually `"sms"`).
    #[serde(rename = "Type")]
    pub r#type: Option<String>,
    /// Plivo-assigned message UUID.
    #[serde(rename = "MessageUUID")]
    pub message_uuid: Option<String>,
    /// Timestamp from Plivo (ISO 8601-ish format).
    #[serde(rename = "Time")]
    pub time: Option<String>,
    /// Any additional fields Plivo includes.
    #[serde(flatten)]
    pub extra: serde_json::Value,
}

impl From<PlivoInbound> for InboundMessage {
    fn from(p: PlivoInbound) -> Self {
        let ts = p.time.as_deref().and_then(|s| {
            time::OffsetDateTime::parse(s, &time::format_description::well_known::Rfc3339).ok()
        });
        let raw = serde_json::to_value(&p).unwrap_or_default();
        InboundMessage {
            id: p.message_uuid.clone(),
            from: p.from,
            to: p.to,
            text: p.text,
            timestamp: ts,
            provider: PROVIDER,
            raw,
        }
    }
}

#[cfg(feature = "axum")]
pub mod axum_handlers {
    use super::*;
    use axum::http::StatusCode;
    use axum::{extract::Form, response::IntoResponse};

    /// Axum handler that accepts Plivo's `application/x-www-form-urlencoded`
    /// inbound webhook and returns the normalized message as JSON.
    pub async fn receive_webhook(Form(inbound): Form<PlivoInbound>) -> impl IntoResponse {
        let msg: InboundMessage = inbound.into();
        (StatusCode::OK, axum::Json(msg))
    }
}

use sms_core::{Headers, InboundWebhook};

impl InboundWebhook for PlivoClient {
    fn provider(&self) -> &'static str {
        PROVIDER
    }

    fn parse_inbound(
        &self,
        _headers: &Headers,
        body: &[u8],
    ) -> Result<sms_core::InboundMessage, sms_core::SmsError> {
        let inbound: PlivoInbound = serde_urlencoded::from_bytes(body)
            .map_err(|e| sms_core::SmsError::Invalid(format!("form decode: {}", e)))?;
        Ok(inbound.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // -- Construction tests --

    #[test]
    fn new_sets_production_base_url() {
        let client = PlivoClient::new("id", "token");
        assert_eq!(client.auth_id, "id");
        assert_eq!(client.auth_token, "token");
        assert_eq!(client.base_url, "https://api.plivo.com");
    }

    #[test]
    fn with_base_url_overrides() {
        let client = PlivoClient::with_base_url("id", "token", "http://localhost:9999".into());
        assert_eq!(client.base_url, "http://localhost:9999");
    }

    // All from_env tests combined to avoid parallel env var races.
    // SAFETY: env var mutations are unsafe in edition 2024 because they are
    // process-global. These tests run serially within this single test
    // function, so there is no concurrent access.
    #[test]
    fn from_env_scenarios() {
        unsafe {
            std::env::remove_var("PLIVO_AUTH_ID");
            std::env::remove_var("PLIVO_AUTH_TOKEN");
        }

        // --- missing auth id ---
        let err = PlivoClient::from_env().unwrap_err();
        assert!(err.to_string().contains("PLIVO_AUTH_ID"));

        // --- missing auth token ---
        unsafe { std::env::set_var("PLIVO_AUTH_ID", "test-id"); }
        let err = PlivoClient::from_env().unwrap_err();
        assert!(err.to_string().contains("PLIVO_AUTH_TOKEN"));

        // --- success ---
        unsafe { std::env::set_var("PLIVO_AUTH_TOKEN", "test-token"); }
        let client = PlivoClient::from_env().unwrap();
        assert_eq!(client.auth_id, "test-id");
        assert_eq!(client.auth_token, "test-token");

        // cleanup
        unsafe {
            std::env::remove_var("PLIVO_AUTH_ID");
            std::env::remove_var("PLIVO_AUTH_TOKEN");
        }
    }

    // -- Send request serialization --

    #[test]
    fn plivo_send_request_serialization() {
        let payload = PlivoSendRequest {
            src: "+10005551234",
            dst: "+14155551234",
            text: "Hello!",
        };
        let j = serde_json::to_value(&payload).unwrap();
        assert_eq!(j["src"], "+10005551234");
        assert_eq!(j["dst"], "+14155551234");
        assert_eq!(j["text"], "Hello!");
    }

    // -- Send response ID extraction --

    #[test]
    fn extracts_message_uuid_from_response() {
        let raw = json!({
            "message": "message(s) queued",
            "message_uuid": ["abc-123", "def-456"],
            "api_id": "xyz"
        });
        let id = raw["message_uuid"][0].as_str().unwrap().to_string();
        assert_eq!(id, "abc-123");
    }

    #[test]
    fn falls_back_when_uuid_missing() {
        let raw = json!({ "message": "queued" });
        let id = raw
            .get("message_uuid")
            .and_then(|v| v.as_array())
            .and_then(|arr| arr.first())
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(sms_core::fallback_id);
        // Should be a valid UUID since it fell back
        assert!(uuid::Uuid::parse_str(&id).is_ok());
    }

    // -- Inbound conversion tests --

    #[test]
    fn inbound_conversion_with_timestamp() {
        let inbound = PlivoInbound {
            from: "+15550001111".into(),
            to: "+15550002222".into(),
            text: "Hello".into(),
            r#type: Some("sms".into()),
            message_uuid: Some("uuid-1".into()),
            time: Some("2024-12-30T12:34:56Z".into()),
            extra: json!({ "foo": "bar" }),
        };
        let msg: InboundMessage = inbound.into();
        assert_eq!(msg.from, "+15550001111");
        assert_eq!(msg.to, "+15550002222");
        assert_eq!(msg.text, "Hello");
        assert_eq!(msg.id, Some("uuid-1".into()));
        assert_eq!(msg.provider, "plivo");
        assert!(msg.timestamp.is_some());
    }

    #[test]
    fn inbound_conversion_without_optional_fields() {
        let inbound = PlivoInbound {
            from: "+1".into(),
            to: "+2".into(),
            text: "hi".into(),
            r#type: None,
            message_uuid: None,
            time: None,
            extra: json!({}),
        };
        let msg: InboundMessage = inbound.into();
        assert_eq!(msg.id, None);
        assert_eq!(msg.timestamp, None);
        assert_eq!(msg.provider, "plivo");
    }

    #[test]
    fn inbound_conversion_bad_timestamp_still_works() {
        let inbound = PlivoInbound {
            from: "+1".into(),
            to: "+2".into(),
            text: "hi".into(),
            r#type: None,
            message_uuid: None,
            time: Some("not-a-valid-time".into()),
            extra: json!({}),
        };
        let msg: InboundMessage = inbound.into();
        assert_eq!(msg.timestamp, None); // gracefully None
    }

    // -- Webhook parse tests --

    #[test]
    fn parse_inbound_form_encoded() {
        let client = PlivoClient::new("id", "token");
        let body = b"From=%2B15550001111&To=%2B15550002222&Text=Hello+World&MessageUUID=uuid-1";
        let msg = client.parse_inbound(&vec![], body).unwrap();
        assert_eq!(msg.from, "+15550001111");
        assert_eq!(msg.to, "+15550002222");
        assert_eq!(msg.text, "Hello World");
        assert_eq!(msg.id, Some("uuid-1".into()));
    }

    #[test]
    fn parse_inbound_invalid_body() {
        let client = PlivoClient::new("id", "token");
        let body = b"garbage data that is not form-encoded properly";
        // This should still attempt to parse — serde_urlencoded is fairly
        // permissive, but missing required fields will fail.
        let result = client.parse_inbound(&vec![], body);
        assert!(result.is_err());
    }

    #[test]
    fn parse_inbound_minimal_fields() {
        let client = PlivoClient::new("id", "token");
        let body = b"From=%2B1&To=%2B2&Text=hi";
        let msg = client.parse_inbound(&vec![], body).unwrap();
        assert_eq!(msg.from, "+1");
        assert_eq!(msg.to, "+2");
        assert_eq!(msg.text, "hi");
    }

    // -- Provider trait --

    #[test]
    fn provider_returns_plivo() {
        let client = PlivoClient::new("id", "token");
        assert_eq!(InboundWebhook::provider(&client), "plivo");
    }

    // -- PlivoInbound serde roundtrip --

    #[test]
    fn plivo_inbound_serde_roundtrip() {
        let inbound = PlivoInbound {
            from: "+1".into(),
            to: "+2".into(),
            text: "msg".into(),
            r#type: Some("sms".into()),
            message_uuid: Some("uuid".into()),
            time: Some("2024-01-01T00:00:00Z".into()),
            extra: json!({}),
        };
        let json_str = serde_json::to_string(&inbound).unwrap();
        let deser: PlivoInbound = serde_json::from_str(&json_str).unwrap();
        assert_eq!(deser.from, "+1");
        assert_eq!(deser.message_uuid, Some("uuid".into()));
    }

    // -- OwnedSendRequest integration --

    #[test]
    fn owned_request_works_with_plivo_payload() {
        let owned = sms_core::OwnedSendRequest::new("+14155551234", "+10005551234", "Hello!");
        let borrowed = owned.as_ref();
        let payload = PlivoSendRequest {
            src: borrowed.from,
            dst: borrowed.to,
            text: borrowed.text,
        };
        let j = serde_json::to_value(&payload).unwrap();
        assert_eq!(j["dst"], "+14155551234");
    }
}
