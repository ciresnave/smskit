use poem::{
    web::{Data, Path},
    http::{HeaderMap, StatusCode},
    Body, Request, Response, Result,
};
use bytes::Bytes;
use sms_core::{Headers, InboundRegistry};
use sms_web_generic::{HeaderConverter, ResponseConverter, WebhookProcessor};

#[derive(Clone)]
pub struct AppState {
    pub registry: InboundRegistry,
}

/// Poem-specific header converter
pub struct PoemHeaderConverter;

impl HeaderConverter for PoemHeaderConverter {
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

/// Poem-specific response converter
pub struct PoemResponseConverter;

impl ResponseConverter for PoemResponseConverter {
    type ResponseType = Response;

    fn from_webhook_response(response: sms_core::WebhookResponse) -> Self::ResponseType {
        let status = StatusCode::from_u16(response.status.as_u16())
            .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);

        Response::builder()
            .status(status)
            .header("content-type", response.content_type)
            .body(response.body)
    }
}

/// Unified webhook handler for Poem
pub async fn unified_webhook(
    req: &Request,
    Path(provider): Path<String>,
    body: Bytes,
    Data(state): Data<&AppState>,
) -> Result<Response> {
    let processor = WebhookProcessor::new(state.registry.clone());
    let generic_headers = PoemHeaderConverter::to_generic_headers(req.headers());
    let response = processor.process_webhook(&provider, generic_headers, &body);
    Ok(PoemResponseConverter::from_webhook_response(response))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn poem_types_compile() {
        let registry = InboundRegistry::new();
        let _state = AppState { registry };
    }
}
