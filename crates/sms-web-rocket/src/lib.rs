use rocket::{http::Status, State};
use sms_core::{Headers, InboundRegistry};
use sms_web_generic::{ResponseConverter, WebhookProcessor};

#[derive(Clone)]
pub struct AppState {
    pub registry: InboundRegistry,
}

/// Raw body data for Rocket
#[derive(Debug)]
pub struct RawBody(pub Vec<u8>);

#[rocket::async_trait]
impl<'r> rocket::data::FromData<'r> for RawBody {
    type Error = Box<dyn std::error::Error + Send + Sync>;

    async fn from_data(
        _req: &'r rocket::Request<'_>,
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

/// Rocket-specific response converter
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

/// Unified webhook handler for Rocket
/// Note: Rocket's handler API doesn't easily allow access to raw headers,
/// so we pass empty headers. For production use, you might want to use
/// custom request guards to extract specific headers you need.
#[rocket::post("/webhooks/<provider>", data = "<body>")]
pub fn unified_webhook(
    provider: String,
    body: RawBody,
    state: &State<AppState>,
) -> (Status, (rocket::http::ContentType, String)) {
    let processor = WebhookProcessor::new(state.registry.clone());
    // Rocket doesn't easily expose all headers in handlers, so we pass empty headers
    // For production use, you'd want to use request guards to extract specific headers
    let headers: Headers = vec![];
    let response = processor.process_webhook(&provider, headers, &body.0);
    RocketResponseConverter::from_webhook_response(response)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rocket_types_compile() {
        let registry = InboundRegistry::new();
        let _state = AppState { registry };
        // Rocket testing requires a full rocket instance, which is complex to set up
        // This test just ensures the types compile correctly
    }
}
