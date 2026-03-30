//! # SMS Web Rocket
//!
//! [Rocket](https://rocket.rs/) web framework integration for smskit webhook
//! processing.

use rocket::{http::Status, Request, State};
use sms_core::{Headers, InboundRegistry};
use sms_web_generic::{ResponseConverter, WebhookProcessor};

/// Shared application state holding the provider registry.
#[derive(Clone)]
pub struct AppState {
    pub registry: InboundRegistry,
}

/// Raw body data extractor for Rocket.
#[derive(Debug)]
pub struct RawBody(pub Vec<u8>);

#[rocket::async_trait]
impl<'r> rocket::data::FromData<'r> for RawBody {
    type Error = Box<dyn std::error::Error + Send + Sync>;

    async fn from_data(
        _req: &'r Request<'_>,
        data: rocket::Data<'r>,
    ) -> rocket::data::Outcome<'r, Self> {
        use rocket::data::ToByteUnit;

        match data.open(2.megabytes()).into_bytes().await {
            Ok(bytes) if bytes.is_complete() => {
                rocket::data::Outcome::Success(RawBody(bytes.into_inner()))
            }
            Ok(_) => rocket::data::Outcome::Error((
                Status::PayloadTooLarge,
                Box::new(std::io::Error::other("Body too large")),
            )),
            Err(e) => rocket::data::Outcome::Error((Status::BadRequest, Box::new(e))),
        }
    }
}

/// Request guard that extracts HTTP headers into smskit's generic
/// [`Headers`](sms_core::Headers) format.
pub struct ExtractedHeaders(pub Headers);

#[rocket::async_trait]
impl<'r> rocket::request::FromRequest<'r> for ExtractedHeaders {
    type Error = std::convert::Infallible;

    async fn from_request(req: &'r Request<'_>) -> rocket::request::Outcome<Self, Self::Error> {
        let headers: Headers = req
            .headers()
            .iter()
            .map(|h| (h.name().to_string(), h.value().to_string()))
            .collect();
        rocket::request::Outcome::Success(ExtractedHeaders(headers))
    }
}

/// Rocket-specific response converter.
pub struct RocketResponseConverter;

impl ResponseConverter for RocketResponseConverter {
    type ResponseType = (Status, (rocket::http::ContentType, String));

    fn from_webhook_response(response: sms_core::WebhookResponse) -> Self::ResponseType {
        let status = match response.status.as_u16() {
            200 => Status::Ok,
            400 => Status::BadRequest,
            401 => Status::Unauthorized,
            404 => Status::NotFound,
            _ => Status::InternalServerError,
        };

        let content_type = match response.content_type.as_str() {
            "application/json" => rocket::http::ContentType::JSON,
            _ => rocket::http::ContentType::Plain,
        };

        (status, (content_type, response.body))
    }
}

/// Unified webhook handler for Rocket.
///
/// Extracts the provider name from the URL path, reads headers via a request
/// guard, and delegates to the [`WebhookProcessor`].
#[rocket::post("/webhooks/<provider>", data = "<body>")]
pub fn unified_webhook(
    provider: String,
    body: RawBody,
    extracted: ExtractedHeaders,
    state: &State<AppState>,
) -> (Status, (rocket::http::ContentType, String)) {
    let processor = WebhookProcessor::new(state.registry.clone());
    let response = processor.process_webhook(&provider, extracted.0, &body.0);
    RocketResponseConverter::from_webhook_response(response)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rocket_types_compile() {
        let registry = InboundRegistry::new();
        let _state = AppState { registry };
    }

    #[test]
    fn response_converter_maps_status_codes() {
        let resp = sms_core::WebhookResponse {
            status: sms_core::HttpStatus::Ok,
            body: "{}".into(),
            content_type: "application/json".into(),
        };
        let (status, (ct, body)) = RocketResponseConverter::from_webhook_response(resp);
        assert_eq!(status, Status::Ok);
        assert_eq!(ct, rocket::http::ContentType::JSON);
        assert_eq!(body, "{}");
    }

    #[test]
    fn response_converter_handles_error_status() {
        let resp = sms_core::WebhookResponse::error(sms_core::HttpStatus::NotFound, "not found");
        let (status, _) = RocketResponseConverter::from_webhook_response(resp);
        assert_eq!(status, Status::NotFound);
    }
}
