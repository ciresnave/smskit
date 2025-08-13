//! Example showing how to integrate smskit with any web framework
//! This demonstrates the framework-agnostic approach using the generic processor

use sms_core::InboundRegistry;
use sms_plivo::PlivoClient;
use sms_web_generic::WebhookProcessor;
use std::sync::Arc;

// Simulated request from any web framework
struct GenericRequest {
    pub provider: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

// Simulated response for any web framework
struct GenericResponse {
    pub status: u16,
    pub content_type: String,
    pub body: String,
}

impl From<sms_core::WebhookResponse> for GenericResponse {
    fn from(response: sms_core::WebhookResponse) -> Self {
        Self {
            status: response.status.as_u16(),
            content_type: response.content_type,
            body: response.body,
        }
    }
}

/// This is how you would integrate smskit into ANY web framework
async fn handle_sms_webhook(req: GenericRequest) -> GenericResponse {
    // 1. Set up your SMS providers (this would typically be done once at startup)
    let plivo = PlivoClient::new("your_auth_id", "your_auth_token");
    let registry = InboundRegistry::new().with(Arc::new(plivo));

    // 2. Create the processor
    let processor = WebhookProcessor::new(registry);

    // 3. Process the webhook - framework agnostic!
    let response = processor.process_webhook(&req.provider, req.headers, &req.body);

    // 4. Convert to your framework's response type
    response.into()
}

#[tokio::main]
async fn main() {
    println!("=== SMS Webhook Generic Integration Example ===");

    // Simulate a webhook request from Plivo
    let request = GenericRequest {
        provider: "plivo".to_string(),
        headers: vec![
            ("content-type".to_string(), "application/x-www-form-urlencoded".to_string()),
            ("user-agent".to_string(), "Plivo-Webhook/1.0".to_string()),
        ],
        body: "From=%2B1234567890&To=%2B0987654321&Text=Hello%20World&MessageUUID=abc-123&Time=2024-12-30T12%3A34%3A56Z&Type=sms".as_bytes().to_vec(),
    };

    // Process the webhook
    let response = handle_sms_webhook(request).await;

    println!("Response Status: {}", response.status);
    println!("Response Content-Type: {}", response.content_type);
    println!("Response Body: {}", response.body);

    println!("\n🎉 This same pattern works with ANY web framework!");
    println!("Just adapt the request/response types and you're good to go.");
}
