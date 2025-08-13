use bytes::Bytes;
use sms_core::{Headers, InboundRegistry};
use sms_web_generic::{HeaderConverter, ResponseConverter, WebhookProcessor};
use warp::{http::HeaderMap, hyper::StatusCode, Filter, Rejection, Reply};

#[derive(Clone)]
pub struct AppState {
    pub registry: InboundRegistry,
}

/// Warp-specific header converter
pub struct WarpHeaderConverter;

impl HeaderConverter for WarpHeaderConverter {
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

/// Warp-specific response converter
pub struct WarpResponseConverter;

impl ResponseConverter for WarpResponseConverter {
    type ResponseType = warp::reply::Response;

    fn from_webhook_response(response: sms_core::WebhookResponse) -> Self::ResponseType {
        let status = StatusCode::from_u16(response.status.as_u16())
            .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);

        warp::reply::with_status(
            warp::reply::with_header(response.body, "content-type", response.content_type),
            status,
        )
        .into_response()
    }
}

/// Unified webhook handler for Warp
pub async fn unified_webhook_handler(
    provider: String,
    headers: HeaderMap,
    body: Bytes,
    state: AppState,
) -> Result<warp::reply::Response, Rejection> {
    let processor = WebhookProcessor::new(state.registry);
    let generic_headers = WarpHeaderConverter::to_generic_headers(&headers);
    let response = processor.process_webhook(&provider, generic_headers, &body);
    Ok(WarpResponseConverter::from_webhook_response(response))
}

/// Helper function to create a Warp filter for SMS webhooks
pub fn webhook_filter(
    state: AppState,
) -> impl warp::Filter<Extract = (warp::reply::Response,), Error = Rejection> + Clone {
    warp::path!("webhooks" / String)
        .and(warp::post())
        .and(warp::header::headers_cloned())
        .and(warp::body::bytes())
        .and(warp::any().map(move || state.clone()))
        .and_then(unified_webhook_handler)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sms_core::InboundRegistry;

    #[tokio::test]
    async fn webhook_filter_compiles() {
        let registry = InboundRegistry::new();
        let state = AppState { registry };
        let _filter = webhook_filter(state);
    }
}
