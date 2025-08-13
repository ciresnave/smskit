use axum::{
    extract::{Path, State},
    http::HeaderMap,
    response::IntoResponse,
};
use bytes::Bytes;
use sms_core::{Headers, InboundRegistry};
use sms_web_generic::{HeaderConverter, ResponseConverter, WebhookProcessor};

#[derive(Clone)]
pub struct AppState {
    pub registry: InboundRegistry,
}

/// Axum-specific header converter
pub struct AxumHeaderConverter;

impl HeaderConverter for AxumHeaderConverter {
    type HeaderType = HeaderMap;

    fn to_generic_headers(headers: &Self::HeaderType) -> Headers {
        headers
            .iter()
            .map(|(k, v)| {
                (
                    k.as_str().to_string(),
                    v.to_str().unwrap_or_default().to_string(),
                )
            })
            .collect()
    }
}

/// Axum-specific response converter
pub struct AxumResponseConverter;

impl ResponseConverter for AxumResponseConverter {
    type ResponseType = axum::response::Response;

    fn from_webhook_response(response: sms_core::WebhookResponse) -> Self::ResponseType {
        let status = axum::http::StatusCode::from_u16(response.status.as_u16())
            .unwrap_or(axum::http::StatusCode::INTERNAL_SERVER_ERROR);

        // For JSON responses, just return the body as-is since it's already JSON formatted
        match response.content_type.as_str() {
            "application/json" => (status, response.body).into_response(),
            _ => (status, response.body).into_response(),
        }
    }
}

/// Unified handler: POST /webhooks/:provider
pub async fn unified_webhook(
    State(state): State<AppState>,
    Path(provider): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> impl IntoResponse {
    let processor = WebhookProcessor::new(state.registry);
    let generic_headers = AxumHeaderConverter::to_generic_headers(&headers);
    let response = processor.process_webhook(&provider, generic_headers, &body);
    AxumResponseConverter::from_webhook_response(response)
}
