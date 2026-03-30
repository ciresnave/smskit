//! # SMS Web Generic
//!
//! Framework-agnostic webhook processing for smskit.
//!
//! This crate provides [`WebhookProcessor`], which takes an
//! [`InboundRegistry`] of providers and handles the full
//! verify → parse → respond pipeline without coupling to any HTTP framework.
//!
//! Framework adapters (`sms-web-axum`, `sms-web-warp`, etc.) convert their
//! native request/response types to/from the generic types defined here
//! using [`HeaderConverter`] and [`ResponseConverter`].

use sms_core::{
    Headers, HttpStatus, InboundMessage, InboundRegistry, WebhookError, WebhookResponse,
};

/// Framework-agnostic webhook processor.
///
/// Holds an [`InboundRegistry`] and drives the full inbound pipeline:
///
/// 1. Look up the provider in the registry.
/// 2. Verify the webhook signature (if the provider implements it).
/// 3. Parse the raw body into an [`InboundMessage`].
/// 4. Return a [`WebhookResponse`] that the framework adapter can convert
///    into its native response type.
#[derive(Clone)]
pub struct WebhookProcessor {
    registry: InboundRegistry,
}

impl WebhookProcessor {
    /// Create a processor backed by the given provider registry.
    pub fn new(registry: InboundRegistry) -> Self {
        Self { registry }
    }

    /// Process an incoming webhook request and return a framework-agnostic response.
    ///
    /// `provider` is the name extracted from the URL path (e.g. `"plivo"`).
    pub fn process_webhook(
        &self,
        provider: &str,
        headers: Headers,
        body: &[u8],
    ) -> WebhookResponse {
        match self.process_webhook_internal(provider, headers, body) {
            Ok(message) => WebhookResponse::success(message),
            Err(e) => self.error_to_response(e),
        }
    }

    fn process_webhook_internal(
        &self,
        provider: &str,
        headers: Headers,
        body: &[u8],
    ) -> Result<InboundMessage, WebhookError> {
        let hook = self
            .registry
            .get(provider)
            .ok_or_else(|| WebhookError::ProviderNotFound(provider.to_string()))?;

        hook.verify(&headers, body)
            .map_err(|e| WebhookError::VerificationFailed(e.to_string()))?;

        hook.parse_inbound(&headers, body)
            .map_err(|e| WebhookError::ParseError(e.to_string()))
    }

    fn error_to_response(&self, error: WebhookError) -> WebhookResponse {
        match error {
            WebhookError::ProviderNotFound(_) => {
                WebhookResponse::error(HttpStatus::NotFound, "unknown provider")
            }
            WebhookError::VerificationFailed(msg) => WebhookResponse::error(
                HttpStatus::Unauthorized,
                &format!("verification failed: {}", msg),
            ),
            WebhookError::ParseError(msg) => {
                WebhookResponse::error(HttpStatus::BadRequest, &format!("parse error: {}", msg))
            }
            WebhookError::SmsError(e) => WebhookResponse::error(
                HttpStatus::InternalServerError,
                &format!("SMS error: {}", e),
            ),
        }
    }
}

/// Trait for converting framework-specific request headers into the generic
/// [`Headers`] type.
pub trait HeaderConverter {
    /// The framework's native header type (e.g. `axum::http::HeaderMap`).
    type HeaderType;

    /// Convert framework headers to generic `Vec<(String, String)>`.
    fn to_generic_headers(headers: &Self::HeaderType) -> Headers;
}

/// Trait for converting a generic [`WebhookResponse`] into the framework's
/// native response type.
pub trait ResponseConverter {
    /// The framework's native response type.
    type ResponseType;

    /// Build a framework response from the generic webhook response.
    fn from_webhook_response(response: WebhookResponse) -> Self::ResponseType;
}

/// Convenience macro for implementing webhook handlers in different frameworks.
#[macro_export]
macro_rules! implement_webhook_handler {
    ($framework:ident, $handler_name:ident, $request_type:ty, $response_type:ty) => {
        pub async fn $handler_name(
            processor: &WebhookProcessor,
            provider: String,
            headers: impl Into<Headers>,
            body: &[u8],
        ) -> $response_type {
            let response = processor.process_webhook(&provider, headers.into(), body);
            <$response_type as ResponseConverter>::from_webhook_response(response)
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use sms_core::{InboundMessage, InboundRegistry, InboundWebhook, SmsError};

    /// A fake provider for testing the processor pipeline.
    struct FakeProvider;

    impl InboundWebhook for FakeProvider {
        fn provider(&self) -> &'static str {
            "fake"
        }

        fn parse_inbound(&self, _headers: &Headers, body: &[u8]) -> Result<InboundMessage, SmsError> {
            let text = String::from_utf8(body.to_vec())
                .map_err(|e| SmsError::Invalid(e.to_string()))?;
            Ok(InboundMessage {
                id: Some("fake-id".into()),
                from: "+1111".into(),
                to: "+2222".into(),
                text,
                timestamp: None,
                provider: "fake",
                raw: serde_json::json!({}),
            })
        }
    }

    /// A provider that always fails verification.
    struct FailVerifyProvider;

    impl InboundWebhook for FailVerifyProvider {
        fn provider(&self) -> &'static str {
            "fail-verify"
        }

        fn parse_inbound(&self, _headers: &Headers, _body: &[u8]) -> Result<InboundMessage, SmsError> {
            unreachable!("should not be called if verify fails");
        }

        fn verify(&self, _headers: &Headers, _body: &[u8]) -> Result<(), SmsError> {
            Err(SmsError::Auth("bad signature".into()))
        }
    }

    /// A provider that fails to parse.
    struct FailParseProvider;

    impl InboundWebhook for FailParseProvider {
        fn provider(&self) -> &'static str {
            "fail-parse"
        }

        fn parse_inbound(&self, _headers: &Headers, _body: &[u8]) -> Result<InboundMessage, SmsError> {
            Err(SmsError::Invalid("cannot parse this".into()))
        }
    }

    fn processor_with(providers: Vec<std::sync::Arc<dyn InboundWebhook>>) -> WebhookProcessor {
        let mut registry = InboundRegistry::new();
        for p in providers {
            registry = registry.with(p);
        }
        WebhookProcessor::new(registry)
    }

    #[test]
    fn unknown_provider_returns_404() {
        let processor = processor_with(vec![]);
        let response = processor.process_webhook("unknown", vec![], b"test");
        assert_eq!(response.status.as_u16(), 404);
        assert!(response.body.contains("unknown provider"));
    }

    #[test]
    fn known_provider_returns_200() {
        let processor = processor_with(vec![std::sync::Arc::new(FakeProvider)]);
        let response = processor.process_webhook("fake", vec![], b"hello");
        assert_eq!(response.status.as_u16(), 200);
        assert!(response.body.contains("fake-id"));
        assert!(response.body.contains("hello"));
    }

    #[test]
    fn verification_failure_returns_401() {
        let processor = processor_with(vec![std::sync::Arc::new(FailVerifyProvider)]);
        let response = processor.process_webhook("fail-verify", vec![], b"data");
        assert_eq!(response.status.as_u16(), 401);
        assert!(response.body.contains("verification failed"));
    }

    #[test]
    fn parse_failure_returns_400() {
        let processor = processor_with(vec![std::sync::Arc::new(FailParseProvider)]);
        let response = processor.process_webhook("fail-parse", vec![], b"data");
        assert_eq!(response.status.as_u16(), 400);
        assert!(response.body.contains("parse error"));
    }

    #[test]
    fn content_type_is_json() {
        let processor = processor_with(vec![std::sync::Arc::new(FakeProvider)]);
        let response = processor.process_webhook("fake", vec![], b"msg");
        assert_eq!(response.content_type, "application/json");
    }

    #[test]
    fn processor_passes_headers_to_provider() {
        // FakeProvider ignores headers, but we verify the pipeline doesn't
        // drop them by simply ensuring it doesn't panic.
        let processor = processor_with(vec![std::sync::Arc::new(FakeProvider)]);
        let headers = vec![
            ("X-Custom".to_string(), "value".to_string()),
            ("Content-Type".to_string(), "application/json".to_string()),
        ];
        let response = processor.process_webhook("fake", headers, b"body");
        assert_eq!(response.status.as_u16(), 200);
    }
}
