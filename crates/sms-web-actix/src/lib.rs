use actix_web::{web, HttpRequest, HttpResponse, Result};
use bytes::Bytes;
use sms_core::{Headers, InboundRegistry};
use sms_web_generic::{HeaderConverter, ResponseConverter, WebhookProcessor};

#[derive(Clone)]
pub struct AppData {
    pub registry: InboundRegistry,
}

/// Actix-web-specific header converter
pub struct ActixHeaderConverter;

impl HeaderConverter for ActixHeaderConverter {
    type HeaderType = HttpRequest;

    fn to_generic_headers(req: &Self::HeaderType) -> Headers {
        req.headers()
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

/// Actix-web-specific response converter
pub struct ActixResponseConverter;

impl ResponseConverter for ActixResponseConverter {
    type ResponseType = HttpResponse;

    fn from_webhook_response(response: sms_core::WebhookResponse) -> Self::ResponseType {
        let mut builder = HttpResponse::build(
            actix_web::http::StatusCode::from_u16(response.status.as_u16())
                .unwrap_or(actix_web::http::StatusCode::INTERNAL_SERVER_ERROR),
        );

        builder
            .content_type(response.content_type.as_str())
            .body(response.body)
    }
}

/// Unified webhook handler for Actix-web
pub async fn unified_webhook(
    path: web::Path<String>,
    req: HttpRequest,
    body: Bytes,
    data: web::Data<AppData>,
) -> Result<HttpResponse> {
    let provider = path.into_inner();
    let processor = WebhookProcessor::new(data.registry.clone());
    let generic_headers = ActixHeaderConverter::to_generic_headers(&req);
    let response = processor.process_webhook(&provider, generic_headers, &body);
    Ok(ActixResponseConverter::from_webhook_response(response))
}

/// Helper function to configure Actix routes
pub fn configure_routes(cfg: &mut web::ServiceConfig) {
    cfg.route("/webhooks/{provider}", web::post().to(unified_webhook));
}

#[cfg(test)]
mod tests {
    use super::*;
    use actix_web::{test, App};

    #[actix_web::test]
    async fn webhook_route_compiles() {
        let registry = InboundRegistry::new();
        let app_data = AppData { registry };

        let _app = test::init_service(
            App::new()
                .app_data(web::Data::new(app_data))
                .configure(configure_routes),
        )
        .await;
    }
}
