use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use sms_core::{InboundMessage, SendRequest, SendResponse, SmsClient, SmsError};

const PROVIDER: &str = "plivo";

/// Plivo REST client.
#[derive(Clone, Debug)]
pub struct PlivoClient {
    /// Plivo Auth ID (aka account SID).
    pub auth_id: String,
    /// Plivo Auth Token (password for Basic auth).
    pub auth_token: String,
    /// API base URL; override for testing/mocking.
    pub base_url: String,
    /// Optional custom HTTP client (behind feature).
    #[cfg(feature = "reqwest")]
    http: reqwest::Client,
}

impl PlivoClient {
    pub fn new<S: Into<String>>(auth_id: S, auth_token: S) -> Self {
        Self::with_base_url(auth_id, auth_token, "https://api.plivo.com".to_string())
    }

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

#[derive(Debug, Serialize)]
struct PlivoSendRequest<'a> {
    src: &'a str,
    dst: &'a str,
    text: &'a str,
}

#[derive(Debug, Deserialize, Serialize)]
struct PlivoSendResponse {
    message: String,
    message_uuid: Vec<String>,
    api_id: String,
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

            // Attempt to parse structured response
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

/// Types used to parse Plivo inbound webhooks for SMS replies.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PlivoInbound {
    #[serde(rename = "From")]
    pub from: String,
    #[serde(rename = "To")]
    pub to: String,
    #[serde(rename = "Text")]
    pub text: String,
    #[serde(rename = "Type")]
    pub r#type: Option<String>,
    #[serde(rename = "MessageUUID")]
    pub message_uuid: Option<String>,
    #[serde(rename = "Time")]
    pub time: Option<String>,
    #[serde(flatten)]
    pub extra: serde_json::Value,
}

impl From<PlivoInbound> for InboundMessage {
    fn from(p: PlivoInbound) -> Self {
        let ts = p.time.as_deref().and_then(|s| {
            // Plivo uses ISO 8601-like formats; best-effort parse
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

    /// Accept `application/x-www-form-urlencoded` webhook from Plivo and return normalized JSON.
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
        // Plivo sends application/x-www-form-urlencoded by default for SMS inbound webhooks.
        let inbound: PlivoInbound = serde_urlencoded::from_bytes(body)
            .map_err(|e| sms_core::SmsError::Invalid(format!("form decode: {}", e)))?;
        Ok(inbound.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn parses_send_response() {
        let payload = PlivoSendRequest {
            src: "123",
            dst: "456",
            text: "hi",
        };
        let j = serde_json::to_string(&payload).unwrap();
        assert!(j.contains("src") && j.contains("dst") && j.contains("text"));

        let raw = json!({
            "message": "message(s) queued",
            "message_uuid": ["abc-123"],
            "api_id": "xyz"
        });
        let id = raw["message_uuid"][0].as_str().unwrap().to_string();
        assert_eq!(id, "abc-123");
    }

    #[test]
    fn inbound_conversion() {
        let inbound = PlivoInbound {
            from: "+15550001111".into(),
            to: "+15550002222".into(),
            text: "Hello".into(),
            r#type: Some("sms".into()),
            message_uuid: Some("uuid-1".into()),
            time: Some("2024-12-30T12:34:56Z".into()),
            extra: serde_json::json!({ "foo": "bar" }),
        };
        let msg: InboundMessage = inbound.into();
        assert_eq!(msg.from, "+15550001111");
        assert_eq!(msg.provider, "plivo");
        assert!(msg.timestamp.is_some());
    }
}
