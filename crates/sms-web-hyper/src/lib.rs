use http_body_util::{BodyExt, Full};
use hyper::{HeaderMap, Request, Response, StatusCode, Uri};
use sms_core::{Headers, InboundRegistry};
use sms_web_generic::{HeaderConverter, ResponseConverter, WebhookProcessor};
use std::convert::Infallible;

type HyperServiceFuture = std::pin::Pin<
    Box<dyn std::future::Future<Output = Result<Response<Full<bytes::Bytes>>, Infallible>> + Send>,
>;

#[derive(Clone)]
pub struct AppState {
    pub registry: InboundRegistry,
}

/// Hyper-specific header converter
pub struct HyperHeaderConverter;

impl HeaderConverter for HyperHeaderConverter {
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

/// Hyper-specific response converter
pub struct HyperResponseConverter;

impl ResponseConverter for HyperResponseConverter {
    type ResponseType = Response<Full<bytes::Bytes>>;

    fn from_webhook_response(response: sms_core::WebhookResponse) -> Self::ResponseType {
        let status = StatusCode::from_u16(response.status.as_u16())
            .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);

        Response::builder()
            .status(status)
            .header("content-type", response.content_type)
            .body(Full::new(bytes::Bytes::from(response.body)))
            .unwrap()
    }
}

/// Extract provider from URI path
fn extract_provider_from_path(uri: &Uri) -> Option<String> {
    let path = uri.path();
    // Expected format: /webhooks/{provider}
    path.strip_prefix("/webhooks/")
        .map(|stripped| stripped.to_string())
}

/// Unified webhook handler for raw Hyper
pub async fn handle_webhook(
    req: Request<hyper::body::Incoming>,
    state: AppState,
) -> Result<Response<Full<bytes::Bytes>>, Infallible> {
    // Extract provider from path
    let provider = match extract_provider_from_path(req.uri()) {
        Some(p) => p,
        None => {
            return Ok(Response::builder()
                .status(StatusCode::NOT_FOUND)
                .header("content-type", "application/json")
                .body(Full::new(bytes::Bytes::from(r#"{"error":"Invalid path"}"#)))
                .unwrap());
        }
    };

    // Get headers before consuming the request
    let generic_headers = HyperHeaderConverter::to_generic_headers(req.headers());

    // Read the body
    let body_bytes = match req.collect().await {
        Ok(collected) => collected.to_bytes(),
        Err(_) => {
            return Ok(Response::builder()
                .status(StatusCode::BAD_REQUEST)
                .header("content-type", "application/json")
                .body(Full::new(bytes::Bytes::from(
                    r#"{"error":"Failed to read body"}"#,
                )))
                .unwrap());
        }
    };

    let processor = WebhookProcessor::new(state.registry);
    let response = processor.process_webhook(&provider, generic_headers, &body_bytes);
    Ok(HyperResponseConverter::from_webhook_response(response))
}

/// Helper function to create a Hyper service
pub fn make_service(
    state: AppState,
) -> impl Fn(Request<hyper::body::Incoming>) -> HyperServiceFuture + Clone {
    move |req| {
        let state = state.clone();
        Box::pin(handle_webhook(req, state))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_provider_works() {
        let uri = "/webhooks/plivo".parse::<Uri>().unwrap();
        assert_eq!(extract_provider_from_path(&uri), Some("plivo".to_string()));

        let uri = "/other/path".parse::<Uri>().unwrap();
        assert_eq!(extract_provider_from_path(&uri), None);
    }

    #[tokio::test]
    async fn hyper_service_compiles() {
        let registry = InboundRegistry::new();
        let state = AppState { registry };
        let _service = make_service(state);
    }
}
