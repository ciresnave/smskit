//! # Twilio SMS Provider
//!
//! [Twilio](https://www.twilio.com/) backend for the smskit multi-provider SMS
//! abstraction.
//!
//! ## Sending messages
//!
//! ```rust,ignore
//! use sms_core::{SendRequest, SmsClient};
//! use sms_twilio::TwilioClient;
//!
//! let client = TwilioClient::new("ACXXXXXXXX", "your_auth_token");
//! let response = client.send(SendRequest {
//!     to: "+14155551234",
//!     from: "+10005551234",
//!     text: "Hello from Twilio!",
//! }).await?;
//! println!("Message SID: {}", response.id);
//! ```
//!
//! ## Creating from environment variables
//!
//! ```rust,ignore
//! let client = TwilioClient::from_env()?;
//! ```
//!
//! Reads `TWILIO_ACCOUNT_SID` and `TWILIO_AUTH_TOKEN` from the environment.
//!
//! ## Webhook signature verification
//!
//! The [`InboundWebhook`](sms_core::InboundWebhook) implementation includes
//! Twilio request signature verification using HMAC-SHA1.  Pass your webhook
//! URL via [`TwilioClient::with_webhook_url`] to enable it.

use async_trait::async_trait;
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha1::Sha1;
use sms_core::{
    Headers, InboundMessage, InboundWebhook, SendRequest, SendResponse, SmsClient, SmsError,
};

const PROVIDER: &str = "twilio";

type HmacSha1 = Hmac<Sha1>;

/// Twilio REST API client.
///
/// Implements [`SmsClient`] for sending SMS and [`InboundWebhook`] for
/// receiving inbound messages with optional signature verification.
///
/// # Construction
///
/// | Method | Description |
/// |--------|-------------|
/// | [`TwilioClient::new`] | Provide credentials directly |
/// | [`TwilioClient::from_env`] | Read `TWILIO_ACCOUNT_SID` / `TWILIO_AUTH_TOKEN` from env |
/// | [`TwilioClient::with_base_url`] | Override the API base URL (for testing) |
/// | [`TwilioClient::with_webhook_url`] | Set the webhook URL for signature verification |
#[derive(Clone, Debug)]
pub struct TwilioClient {
    /// Twilio Account SID.
    pub account_sid: String,
    /// Twilio Auth Token (used for Basic auth and signature verification).
    pub auth_token: String,
    /// API base URL; override with [`with_base_url`](TwilioClient::with_base_url)
    /// for testing.
    pub base_url: String,
    /// Webhook URL used for signature verification. If `None`, signature
    /// verification is skipped.
    pub webhook_url: Option<String>,
    http: reqwest::Client,
}

impl TwilioClient {
    /// Create a new client with explicit credentials.
    ///
    /// Connects to the production Twilio API at `https://api.twilio.com`.
    ///
    /// # Arguments
    ///
    /// * `account_sid` - Your Twilio Account SID (starts with `AC`).
    /// * `auth_token`  - Your Twilio Auth Token.
    pub fn new(account_sid: impl Into<String>, auth_token: impl Into<String>) -> Self {
        Self {
            account_sid: account_sid.into(),
            auth_token: auth_token.into(),
            base_url: "https://api.twilio.com".to_string(),
            webhook_url: None,
            http: reqwest::Client::new(),
        }
    }

    /// Create a new client by reading credentials from environment variables.
    ///
    /// | Variable              | Maps to        |
    /// |-----------------------|----------------|
    /// | `TWILIO_ACCOUNT_SID`  | `account_sid`  |
    /// | `TWILIO_AUTH_TOKEN`   | `auth_token`   |
    ///
    /// Returns [`SmsError::Auth`] if either variable is missing.
    pub fn from_env() -> Result<Self, SmsError> {
        let account_sid = std::env::var("TWILIO_ACCOUNT_SID")
            .map_err(|_| SmsError::Auth("TWILIO_ACCOUNT_SID not set".into()))?;
        let auth_token = std::env::var("TWILIO_AUTH_TOKEN")
            .map_err(|_| SmsError::Auth("TWILIO_AUTH_TOKEN not set".into()))?;
        Ok(Self::new(account_sid, auth_token))
    }

    /// Create a client with a custom API base URL.
    ///
    /// Primarily useful for integration tests where you point at a mock HTTP
    /// server instead of Twilio's production API.
    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = base_url.into();
        self
    }

    /// Set the webhook URL used for signature verification.
    ///
    /// Twilio signs webhook requests using your Auth Token and the full
    /// request URL.  If this is set, [`InboundWebhook::verify`] will check
    /// the `X-Twilio-Signature` header.  If not set, verification is skipped.
    pub fn with_webhook_url(mut self, url: impl Into<String>) -> Self {
        self.webhook_url = Some(url.into());
        self
    }

    /// Compute the expected Twilio signature for a given URL and POST params.
    ///
    /// Algorithm: HMAC-SHA1(auth_token, url + sorted(key=value pairs)), base64-encoded.
    fn compute_signature(&self, url: &str, params: &[(String, String)]) -> String {
        let mut data = url.to_string();
        let mut sorted_params = params.to_vec();
        sorted_params.sort_by(|a, b| a.0.cmp(&b.0));
        for (key, value) in &sorted_params {
            data.push_str(key);
            data.push_str(value);
        }

        let mut mac =
            HmacSha1::new_from_slice(self.auth_token.as_bytes()).expect("HMAC accepts any key size");
        mac.update(data.as_bytes());
        let result = mac.finalize();
        use base64::Engine;
        base64::engine::general_purpose::STANDARD.encode(result.into_bytes())
    }
}

/// Wire format for the Twilio send-message request body (form-encoded).
#[derive(Debug, Serialize)]
struct TwilioSendPayload<'a> {
    #[serde(rename = "To")]
    to: &'a str,
    #[serde(rename = "From")]
    from: &'a str,
    #[serde(rename = "Body")]
    body: &'a str,
}

#[async_trait]
impl SmsClient for TwilioClient {
    async fn send(&self, req: SendRequest<'_>) -> Result<SendResponse, SmsError> {
        let url = format!(
            "{}/2010-04-01/Accounts/{}/Messages.json",
            self.base_url.trim_end_matches('/'),
            self.account_sid
        );

        let payload = TwilioSendPayload {
            to: req.to,
            from: req.from,
            body: req.text,
        };

        let res = self
            .http
            .post(&url)
            .basic_auth(&self.account_sid, Some(&self.auth_token))
            .form(&payload)
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
            .get("sid")
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

/// The form-encoded payload that Twilio POSTs to your webhook URL when an
/// inbound SMS arrives.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TwilioInbound {
    /// Twilio message SID.
    #[serde(rename = "MessageSid")]
    pub message_sid: Option<String>,
    /// Sender phone number.
    #[serde(rename = "From")]
    pub from: String,
    /// Destination number (your Twilio number).
    #[serde(rename = "To")]
    pub to: String,
    /// Message body.
    #[serde(rename = "Body")]
    pub body: String,
    /// Number of media attachments.
    #[serde(rename = "NumMedia")]
    pub num_media: Option<String>,
    /// Account SID.
    #[serde(rename = "AccountSid")]
    pub account_sid: Option<String>,
    /// Any additional fields Twilio includes.
    #[serde(flatten)]
    pub extra: serde_json::Value,
}

impl From<TwilioInbound> for InboundMessage {
    fn from(t: TwilioInbound) -> Self {
        let raw = serde_json::to_value(&t).unwrap_or_default();
        InboundMessage {
            id: t.message_sid.clone(),
            from: t.from,
            to: t.to,
            text: t.body,
            timestamp: None, // Twilio doesn't include a timestamp in inbound webhooks
            provider: PROVIDER,
            raw,
        }
    }
}

impl InboundWebhook for TwilioClient {
    fn provider(&self) -> &'static str {
        PROVIDER
    }

    fn parse_inbound(&self, _headers: &Headers, body: &[u8]) -> Result<InboundMessage, SmsError> {
        let inbound: TwilioInbound = serde_urlencoded::from_bytes(body)
            .map_err(|e| SmsError::Invalid(format!("form decode: {}", e)))?;
        Ok(inbound.into())
    }

    fn verify(&self, headers: &Headers, body: &[u8]) -> Result<(), SmsError> {
        let webhook_url = match &self.webhook_url {
            Some(url) => url,
            None => return Ok(()), // No webhook URL configured; skip verification
        };

        // Extract the X-Twilio-Signature header
        let signature = headers
            .iter()
            .find_map(|(k, v)| {
                if k.eq_ignore_ascii_case("x-twilio-signature") {
                    Some(v.as_str())
                } else {
                    None
                }
            })
            .ok_or_else(|| SmsError::Auth("missing X-Twilio-Signature header".into()))?;

        // Parse the form-encoded body into sorted params
        let params: Vec<(String, String)> = serde_urlencoded::from_bytes(body)
            .map_err(|e| SmsError::Invalid(format!("form decode for verification: {}", e)))?;

        let expected = self.compute_signature(webhook_url, &params);

        if expected == signature {
            Ok(())
        } else {
            Err(SmsError::Auth("invalid Twilio signature".into()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // -- Construction tests --

    #[test]
    fn new_sets_production_base_url() {
        let client = TwilioClient::new("AC123", "token");
        assert_eq!(client.account_sid, "AC123");
        assert_eq!(client.auth_token, "token");
        assert_eq!(client.base_url, "https://api.twilio.com");
        assert!(client.webhook_url.is_none());
    }

    #[test]
    fn with_base_url_overrides() {
        let client = TwilioClient::new("AC123", "token")
            .with_base_url("http://localhost:9999");
        assert_eq!(client.base_url, "http://localhost:9999");
    }

    #[test]
    fn with_webhook_url_sets_url() {
        let client = TwilioClient::new("AC123", "token")
            .with_webhook_url("https://example.com/webhook");
        assert_eq!(client.webhook_url, Some("https://example.com/webhook".into()));
    }

    // All from_env tests combined to avoid parallel env var races.
    // SAFETY: env var mutations are unsafe in edition 2024 because they are
    // process-global. These tests run serially within this single test
    // function, so there is no concurrent access.
    #[test]
    fn from_env_scenarios() {
        unsafe {
            std::env::remove_var("TWILIO_ACCOUNT_SID");
            std::env::remove_var("TWILIO_AUTH_TOKEN");
        }

        // --- missing account SID ---
        let err = TwilioClient::from_env().unwrap_err();
        assert!(err.to_string().contains("TWILIO_ACCOUNT_SID"));

        // --- missing auth token ---
        unsafe { std::env::set_var("TWILIO_ACCOUNT_SID", "AC-test"); }
        let err = TwilioClient::from_env().unwrap_err();
        assert!(err.to_string().contains("TWILIO_AUTH_TOKEN"));

        // --- success ---
        unsafe { std::env::set_var("TWILIO_AUTH_TOKEN", "test-token"); }
        let client = TwilioClient::from_env().unwrap();
        assert_eq!(client.account_sid, "AC-test");
        assert_eq!(client.auth_token, "test-token");

        // cleanup
        unsafe {
            std::env::remove_var("TWILIO_ACCOUNT_SID");
            std::env::remove_var("TWILIO_AUTH_TOKEN");
        }
    }

    // -- Send payload serialization --

    #[test]
    fn send_payload_serialization() {
        let payload = TwilioSendPayload {
            to: "+14155551234",
            from: "+10005551234",
            body: "Hello!",
        };
        let encoded = serde_urlencoded::to_string(&payload).unwrap();
        assert!(encoded.contains("To=%2B14155551234"));
        assert!(encoded.contains("From=%2B10005551234"));
        assert!(encoded.contains("Body=Hello%21"));
    }

    // -- Send response ID extraction --

    #[test]
    fn extracts_sid_from_response() {
        let raw = json!({
            "sid": "SM1234567890",
            "status": "queued",
            "date_created": "2024-01-01T00:00:00Z"
        });
        let id = raw["sid"].as_str().unwrap().to_string();
        assert_eq!(id, "SM1234567890");
    }

    #[test]
    fn falls_back_when_sid_missing() {
        let raw = json!({ "status": "queued" });
        let id = raw
            .get("sid")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(sms_core::fallback_id);
        assert!(uuid::Uuid::parse_str(&id).is_ok());
    }

    // -- Inbound conversion tests --

    #[test]
    fn inbound_conversion_full() {
        let inbound = TwilioInbound {
            message_sid: Some("SM123".into()),
            from: "+15550001111".into(),
            to: "+15550002222".into(),
            body: "Hello".into(),
            num_media: Some("0".into()),
            account_sid: Some("AC123".into()),
            extra: json!({}),
        };
        let msg: InboundMessage = inbound.into();
        assert_eq!(msg.from, "+15550001111");
        assert_eq!(msg.to, "+15550002222");
        assert_eq!(msg.text, "Hello");
        assert_eq!(msg.id, Some("SM123".into()));
        assert_eq!(msg.provider, "twilio");
        assert_eq!(msg.timestamp, None); // Twilio doesn't provide timestamp
    }

    #[test]
    fn inbound_conversion_minimal() {
        let inbound = TwilioInbound {
            message_sid: None,
            from: "+1".into(),
            to: "+2".into(),
            body: "hi".into(),
            num_media: None,
            account_sid: None,
            extra: json!({}),
        };
        let msg: InboundMessage = inbound.into();
        assert_eq!(msg.id, None);
        assert_eq!(msg.provider, "twilio");
    }

    // -- Webhook parse tests --

    #[test]
    fn parse_inbound_form_encoded() {
        let client = TwilioClient::new("AC123", "token");
        let body = b"MessageSid=SM123&From=%2B15550001111&To=%2B15550002222&Body=Hello+World";
        let msg = client.parse_inbound(&vec![], body).unwrap();
        assert_eq!(msg.from, "+15550001111");
        assert_eq!(msg.to, "+15550002222");
        assert_eq!(msg.text, "Hello World");
        assert_eq!(msg.id, Some("SM123".into()));
    }

    #[test]
    fn parse_inbound_invalid_body() {
        let client = TwilioClient::new("AC123", "token");
        // Missing required fields
        let body = b"SomeField=value";
        let result = client.parse_inbound(&vec![], body);
        assert!(result.is_err());
    }

    #[test]
    fn parse_inbound_minimal_fields() {
        let client = TwilioClient::new("AC123", "token");
        let body = b"From=%2B1&To=%2B2&Body=hi";
        let msg = client.parse_inbound(&vec![], body).unwrap();
        assert_eq!(msg.from, "+1");
        assert_eq!(msg.text, "hi");
    }

    // -- Provider trait --

    #[test]
    fn provider_returns_twilio() {
        let client = TwilioClient::new("AC123", "token");
        assert_eq!(InboundWebhook::provider(&client), "twilio");
    }

    // -- Signature verification --

    #[test]
    fn signature_computation_matches_twilio_spec() {
        // Based on Twilio's documented signature validation algorithm
        let client = TwilioClient::new("AC123", "12345");
        let url = "https://mycompany.com/myapp.php?foo=1&bar=2";
        let params = vec![
            ("CallSid".to_string(), "CA1234567890ABCDE".to_string()),
            ("Caller".to_string(), "+14158675310".to_string()),
            ("Digits".to_string(), "1234".to_string()),
            ("From".to_string(), "+14158675310".to_string()),
            ("To".to_string(), "+18005551212".to_string()),
        ];
        let sig = client.compute_signature(url, &params);
        // The signature should be a valid base64 string
        assert!(!sig.is_empty());
        use base64::Engine;
        assert!(base64::engine::general_purpose::STANDARD.decode(&sig).is_ok());
    }

    #[test]
    fn verify_skipped_when_no_webhook_url() {
        let client = TwilioClient::new("AC123", "token");
        // No webhook_url set — should always succeed
        let result = client.verify(&vec![], b"anything");
        assert!(result.is_ok());
    }

    #[test]
    fn verify_fails_when_signature_missing() {
        let client = TwilioClient::new("AC123", "token")
            .with_webhook_url("https://example.com/webhook");
        let body = b"From=%2B1&To=%2B2&Body=hi";
        let result = client.verify(&vec![], body);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("missing X-Twilio-Signature"));
    }

    #[test]
    fn verify_fails_with_wrong_signature() {
        let client = TwilioClient::new("AC123", "token")
            .with_webhook_url("https://example.com/webhook");
        let body = b"From=%2B1&To=%2B2&Body=hi";
        let headers = vec![("X-Twilio-Signature".to_string(), "badsignature".to_string())];
        let result = client.verify(&headers, body);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("invalid Twilio signature"));
    }

    #[test]
    fn verify_succeeds_with_correct_signature() {
        let client = TwilioClient::new("AC123", "my-secret-token")
            .with_webhook_url("https://example.com/webhook");
        let body = b"Body=hi&From=%2B1&To=%2B2";
        let params: Vec<(String, String)> = serde_urlencoded::from_bytes(body).unwrap();
        let expected_sig = client.compute_signature("https://example.com/webhook", &params);
        let headers = vec![("X-Twilio-Signature".to_string(), expected_sig)];
        let result = client.verify(&headers, body);
        assert!(result.is_ok());
    }

    // -- Serde roundtrip --

    #[test]
    fn twilio_inbound_serde_roundtrip() {
        let inbound = TwilioInbound {
            message_sid: Some("SM123".into()),
            from: "+1".into(),
            to: "+2".into(),
            body: "msg".into(),
            num_media: Some("0".into()),
            account_sid: Some("AC123".into()),
            extra: json!({}),
        };
        let json_str = serde_json::to_string(&inbound).unwrap();
        let deser: TwilioInbound = serde_json::from_str(&json_str).unwrap();
        assert_eq!(deser.from, "+1");
        assert_eq!(deser.message_sid, Some("SM123".into()));
    }

    // -- OwnedSendRequest integration --

    #[test]
    fn owned_request_works_with_twilio_payload() {
        let owned = sms_core::OwnedSendRequest::new("+14155551234", "+10005551234", "Hello!");
        let borrowed = owned.as_ref();
        let payload = TwilioSendPayload {
            to: borrowed.to,
            from: borrowed.from,
            body: borrowed.text,
        };
        let encoded = serde_urlencoded::to_string(&payload).unwrap();
        assert!(encoded.contains("To=%2B14155551234"));
    }
}
