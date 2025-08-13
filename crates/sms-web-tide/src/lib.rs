use sms_core::{Headers, InboundRegistry};
use sms_web_generic::{HeaderConverter, ResponseConverter, WebhookProcessor};
use tide::{Request, Response, Result, StatusCode};

#[derive(Clone)]
pub struct AppState {
    pub registry: InboundRegistry,
}

/// Tide-specific header converter
pub struct TideHeaderConverter;

impl HeaderConverter for TideHeaderConverter {
    type HeaderType = Request<AppState>;

    fn to_generic_headers(req: &Self::HeaderType) -> Headers {
        req.iter()
            .map(|(name, values)| {
                let value = values
                    .iter()
                    .map(|v| v.as_str())
                    .collect::<Vec<_>>()
                    .join(", ");
                (name.as_str().to_string(), value)
            })
            .collect()
    }
}

/// Tide-specific response converter
pub struct TideResponseConverter;

impl ResponseConverter for TideResponseConverter {
    type ResponseType = Result<Response>;

    fn from_webhook_response(response: sms_core::WebhookResponse) -> Self::ResponseType {
        let status = StatusCode::try_from(response.status.as_u16())
            .unwrap_or(StatusCode::InternalServerError);

        let mut res = Response::new(status);
        res.set_body(response.body);

        // Parse content type, defaulting to application/json
        let content_type = if response.content_type == "application/json" {
            tide::http::mime::JSON
        } else {
            tide::http::mime::PLAIN
        };
        res.set_content_type(content_type);
        Ok(res)
    }
}

/// Unified webhook handler for Tide
pub async fn unified_webhook(mut req: Request<AppState>) -> Result<Response> {
    let provider = req.param("provider")?.to_string();
    let body = req.body_bytes().await?;
    let processor = WebhookProcessor::new(req.state().registry.clone());
    let generic_headers = TideHeaderConverter::to_generic_headers(&req);
    let response = processor.process_webhook(&provider, generic_headers, &body);
    TideResponseConverter::from_webhook_response(response)
}

/// Helper function to configure Tide routes
pub fn configure_routes(app: &mut tide::Server<AppState>) {
    app.at("/webhooks/:provider").post(unified_webhook);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tide_types_compile() {
        let registry = InboundRegistry::new();
        let _state = AppState { registry };
        // let mut app = tide::with_state(state);
        // configure_routes(&mut app);
    }
}
