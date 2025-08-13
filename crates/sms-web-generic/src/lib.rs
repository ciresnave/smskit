use sms_core::{
    Headers, HttpStatus, InboundMessage, InboundRegistry, WebhookError, WebhookResponse,
};

/// Framework-agnostic webhook processor that handles the core SMS logic
#[derive(Clone)]
pub struct WebhookProcessor {
    registry: InboundRegistry,
}

impl WebhookProcessor {
    pub fn new(registry: InboundRegistry) -> Self {
        Self { registry }
    }

    /// Process an incoming webhook request and return a framework-agnostic response
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

/// Helper trait for framework adapters to convert headers
pub trait HeaderConverter {
    type HeaderType;

    fn to_generic_headers(headers: &Self::HeaderType) -> Headers;
}

/// Helper trait for framework adapters to convert responses
pub trait ResponseConverter {
    type ResponseType;

    fn from_webhook_response(response: WebhookResponse) -> Self::ResponseType;
}

/// Convenience macro for implementing webhook handlers in different frameworks
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
    use sms_core::InboundRegistry;

    #[test]
    fn processor_handles_unknown_provider() {
        let registry = InboundRegistry::new();
        let processor = WebhookProcessor::new(registry);

        let response = processor.process_webhook("unknown", vec![], b"test");
        assert_eq!(response.status.as_u16(), 404);
        assert!(response.body.contains("unknown provider"));
    }
}
