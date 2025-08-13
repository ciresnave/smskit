# SMS Kit API Documentation

## Table of Contents

1. [Overview](#overview)
2. [Installation](#installation)
3. [Quick Start](#quick-start)
4. [Configuration](#configuration)
5. [Provider APIs](#provider-apis)
6. [Webhook Processing](#webhook-processing)
7. [Rate Limiting](#rate-limiting)
8. [Error Handling](#error-handling)
9. [Examples](#examples)
10. [Contributing](#contributing)

## Overview

SMS Kit is a comprehensive, production-ready multi-provider SMS abstraction library for Rust. It provides a unified interface for sending SMS messages and processing webhooks across multiple SMS providers.

### Key Features

- **Multi-provider support**: Plivo, Twilio, AWS SNS, and more
- **Framework agnostic**: Works with Axum, Warp, Actix, or any HTTP framework
- **Webhook processing**: Unified webhook handling for inbound SMS
- **Type safety**: Strongly typed SMS operations and responses
- **Rate limiting**: Built-in rate limiting with per-provider configuration
- **Comprehensive configuration**: Environment-based configuration management
- **Observability**: Structured logging and tracing support
- **Production ready**: Security, error handling, and reliability features

### Architecture

```
┌─────────────────┐
│   Application   │
└─────────────────┘
         │
┌─────────────────┐
│    SMS Kit      │ ← Core abstraction layer
└─────────────────┘
         │
┌─────────────────┐
│   Providers     │ ← Plivo, Twilio, AWS SNS, etc.
└─────────────────┘
```

## Installation

Add SMS Kit to your `Cargo.toml`:

```toml
[dependencies]
smskit = "0.2.0"

# Add specific providers
sms-plivo = "0.1.0"
sms-twilio = "0.2.0"
sms-aws-sns = "0.2.0"

# Web framework integration
sms-web-axum = "0.1.0"  # For Axum
```

## Quick Start

### Sending SMS with Plivo

```rust
use smskit::prelude::*;
use sms_plivo::PlivoClient;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = PlivoClient::new(
        "your_auth_id".to_string(),
        "your_auth_token".to_string(),
        None,
    );

    let response = client.send(SendRequest {
        to: "+1234567890",
        from: "+0987654321",
        text: "Hello from SMS Kit!"
    }).await?;

    println!("Message sent with ID: {}", response.id);
    Ok(())
}
```

### Sending SMS with Twilio

```rust
use smskit::prelude::*;
use sms_twilio::TwilioClient;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = TwilioClient::new(
        "your_account_sid".to_string(),
        "your_auth_token".to_string(),
        None,
    );

    let response = client.send(SendRequest {
        to: "+1234567890",
        from: "+0987654321",
        text: "Hello from Twilio via SMS Kit!"
    }).await?;

    println!("Message sent with ID: {}", response.id);
    Ok(())
}
```

### Processing Webhooks with Axum

```rust
use axum::{
    extract::{Path, State},
    response::Json,
    routing::post,
    Router,
};
use sms_web_generic::WebhookProcessor;
use sms_core::{InboundRegistry, Headers};
use std::sync::Arc;

#[derive(Clone)]
struct AppState {
    processor: WebhookProcessor,
}

async fn handle_webhook(
    Path(provider): Path<String>,
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    body: axum::body::Bytes,
) -> Result<Json<serde_json::Value>, axum::http::StatusCode> {
    let generic_headers: Headers = headers
        .iter()
        .map(|(name, value)| (
            name.to_string(),
            value.to_str().unwrap_or("").to_string()
        ))
        .collect();

    let response = state.processor.process_webhook(&provider, generic_headers, &body);

    match response.status.as_u16() {
        200..=299 => {
            let json: serde_json::Value = serde_json::from_str(&response.body)
                .unwrap_or_else(|_| serde_json::json!({"status": "ok"}));
            Ok(Json(json))
        },
        404 => Err(axum::http::StatusCode::NOT_FOUND),
        _ => Err(axum::http::StatusCode::INTERNAL_SERVER_ERROR),
    }
}

#[tokio::main]
async fn main() {
    let registry = InboundRegistry::new();
    // Register providers here

    let app_state = AppState {
        processor: WebhookProcessor::new(registry),
    };

    let app = Router::new()
        .route("/webhook/:provider", post(handle_webhook))
        .with_state(app_state);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
```

## Configuration

SMS Kit uses a comprehensive configuration system that supports environment variables and configuration files.

### Environment Variables

```bash
# Server configuration
SMS_SERVER_HOST=0.0.0.0
SMS_SERVER_PORT=3000

# Rate limiting
SMS_RATE_LIMIT_ENABLED=true
SMS_RATE_LIMIT_MAX_REQUESTS=100
SMS_RATE_LIMIT_WINDOW_SECONDS=60

# Logging
SMS_LOGGING_LEVEL=info
SMS_LOGGING_FORMAT=json

# Security
SMS_SECURITY_REQUIRE_HTTPS=true
SMS_SECURITY_CORS_ALLOWED_ORIGINS=https://example.com

# Provider configuration
SMS_PROVIDERS_PLIVO_AUTH_ID=your_plivo_auth_id
SMS_PROVIDERS_PLIVO_AUTH_TOKEN=your_plivo_auth_token

SMS_PROVIDERS_TWILIO_ACCOUNT_SID=your_twilio_account_sid
SMS_PROVIDERS_TWILIO_AUTH_TOKEN=your_twilio_auth_token

SMS_PROVIDERS_AWS_ACCESS_KEY_ID=your_aws_access_key
SMS_PROVIDERS_AWS_SECRET_ACCESS_KEY=your_aws_secret_key
SMS_PROVIDERS_AWS_REGION=us-east-1
```

### Configuration in Code

```rust
use smskit::config::AppConfig;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Load configuration from environment
    let config = AppConfig::load()?;

    println!("Server will run on {}:{}", config.server.host, config.server.port);
    println!("Rate limit: {} requests per {}s",
             config.rate_limit.max_requests,
             config.rate_limit.window_seconds);

    Ok(())
}
```

## Provider APIs

### Core Traits

All SMS providers implement the `SmsClient` trait:

```rust
#[async_trait]
pub trait SmsClient: Send + Sync {
    async fn send(&self, req: SendRequest<'_>) -> Result<SendResponse, SmsError>;
}
```

For webhook processing, providers implement `InboundWebhook`:

```rust
#[async_trait]
pub trait InboundWebhook: Send + Sync {
    fn provider(&self) -> &'static str;
    fn parse_inbound(&self, headers: &Headers, body: &[u8]) -> Result<InboundMessage, SmsError>;
    fn verify(&self, _headers: &Headers, _body: &[u8]) -> Result<(), SmsError> { Ok(()) }
}
```

### Request/Response Types

#### Send Request

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SendRequest<'a> {
    pub to: &'a str,        // Destination phone number
    pub from: &'a str,      // Source phone number
    pub text: &'a str,      // Message content
}
```

#### Send Response

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SendResponse {
    pub id: String,                    // Provider message ID
    pub provider: &'static str,        // Provider name
    pub raw: serde_json::Value,        // Raw provider response
}
```

#### Inbound Message

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InboundMessage {
    pub id: Option<String>,            // Message ID
    pub from: String,                  // Sender phone number
    pub to: String,                    // Recipient phone number
    pub text: String,                  // Message content
    pub timestamp: Option<OffsetDateTime>, // Message timestamp
    pub provider: &'static str,        // Provider name
    pub raw: serde_json::Value,        // Raw provider payload
}
```

### Error Types

```rust
#[derive(Debug, thiserror::Error)]
pub enum SmsError {
    #[error("http error: {0}")]
    Http(String),
    #[error("authentication error: {0}")]
    Auth(String),
    #[error("invalid request: {0}")]
    Invalid(String),
    #[error("provider error: {0}")]
    Provider(String),
    #[error("unexpected: {0}")]
    Unexpected(String),
}
```

## Webhook Processing

### Registry System

The `InboundRegistry` manages multiple webhook processors:

```rust
use sms_core::InboundRegistry;
use sms_plivo::PlivoClient;
use sms_twilio::TwilioClient;
use std::sync::Arc;

let plivo = Arc::new(PlivoClient::new(
    "auth_id".to_string(),
    "auth_token".to_string(),
    None
));

let twilio = Arc::new(TwilioClient::new(
    "account_sid".to_string(),
    "auth_token".to_string(),
    None
));

let registry = InboundRegistry::new()
    .with(plivo)
    .with(twilio);
```

### Processing Webhooks

```rust
use sms_web_generic::WebhookProcessor;

let processor = WebhookProcessor::new(registry);

// Process incoming webhook
let headers = vec![
    ("content-type".to_string(), "application/json".to_string()),
    ("x-plivo-signature-v2".to_string(), signature.to_string()),
];

let response = processor.process_webhook("plivo", headers, payload.as_bytes());

match response.status.as_u16() {
    200 => println!("Webhook processed successfully"),
    404 => println!("Unknown provider"),
    401 => println!("Signature verification failed"),
    400 => println!("Invalid payload"),
    _ => println!("Internal error"),
}
```

### Signature Verification

Each provider implements its own signature verification:

```rust
// Plivo uses HMAC-SHA1 and HMAC-SHA256
let plivo = PlivoClient::new(
    "auth_id".to_string(),
    "auth_token".to_string(),
    Some("https://your-webhook-url.com".to_string())  // Required for verification
);

// Twilio uses HMAC-SHA1
let twilio = TwilioClient::new(
    "account_sid".to_string(),
    "auth_token".to_string(),
    Some("https://your-webhook-url.com".to_string())  // Required for verification
);
```

## Rate Limiting

SMS Kit includes built-in rate limiting using a token bucket algorithm.

### Configuration

```rust
use smskit::rate_limiter::{RateLimiter, RateLimitConfig};
use std::collections::HashMap;

let mut per_provider = HashMap::new();
per_provider.insert("twilio".to_string(), ProviderRateLimit {
    max_requests: 50,
    window_seconds: 60,
});

let config = RateLimitConfig {
    max_requests: 100,        // Global limit
    window_seconds: 60,       // 60 second window
    enabled: true,
    per_provider,            // Provider-specific limits
};

let limiter = RateLimiter::new(config);
```

### Usage

```rust
match limiter.check_rate_limit("provider:client_ip").await {
    RateLimitResult::Allowed => {
        // Process request
    },
    RateLimitResult::Limited { retry_after } => {
        // Return 429 Too Many Requests
        let retry_seconds = retry_after.as_secs();
        return Err(format!("Rate limited, retry after {} seconds", retry_seconds));
    }
}
```

### Key Generation

```rust
use smskit::rate_limiter::{KeyGenerator, DefaultKeyGenerator};

let key_gen = DefaultKeyGenerator;

// Generate rate limit key
let key = key_gen.generate_key("plivo", client_ip);

// Extract client IP from headers
let client_ip = key_gen.extract_client_ip(&headers)
    .unwrap_or_else(|| "unknown".to_string());
```

## Error Handling

### Error Types

SMS Kit defines comprehensive error types for different scenarios:

```rust
use sms_core::{SmsError, WebhookError};

// SMS operation errors
match client.send(request).await {
    Ok(response) => println!("Sent: {}", response.id),
    Err(SmsError::Http(msg)) => eprintln!("HTTP error: {}", msg),
    Err(SmsError::Auth(msg)) => eprintln!("Auth error: {}", msg),
    Err(SmsError::Invalid(msg)) => eprintln!("Invalid request: {}", msg),
    Err(SmsError::Provider(msg)) => eprintln!("Provider error: {}", msg),
    Err(SmsError::Unexpected(msg)) => eprintln!("Unexpected error: {}", msg),
}

// Webhook processing errors
match processor.process_webhook("plivo", headers, body) {
    response if response.status.as_u16() == 200 => {
        // Success
    },
    response if response.status.as_u16() == 404 => {
        // Unknown provider
    },
    response if response.status.as_u16() == 401 => {
        // Signature verification failed
    },
    response => {
        // Other error
        eprintln!("Error: {}", response.body);
    }
}
```

### Best Practices

1. **Always handle errors gracefully**:

   ```rust
   match client.send(request).await {
       Ok(response) => {
           log::info!("SMS sent successfully: {}", response.id);
           response
       },
       Err(e) => {
           log::error!("Failed to send SMS: {}", e);
           return Err(e.into());
       }
   }
   ```

2. **Use structured logging**:

   ```rust
   use tracing::{info, warn, error};

   match result {
       Ok(response) => info!(
           message_id = response.id,
           provider = response.provider,
           "SMS sent successfully"
       ),
       Err(e) => error!(
           error = %e,
           "Failed to send SMS"
       ),
   }
   ```

3. **Implement retry logic for transient errors**:

   ```rust
   use tokio::time::{sleep, Duration};

   let mut attempts = 0;
   let max_attempts = 3;

   loop {
       match client.send(request).await {
           Ok(response) => return Ok(response),
           Err(SmsError::Http(_)) if attempts < max_attempts => {
               attempts += 1;
               sleep(Duration::from_secs(2_u64.pow(attempts))).await;
               continue;
           },
           Err(e) => return Err(e),
       }
   }
   ```

## Examples

### Complete Web Application

```rust
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::Json,
    routing::{get, post},
    Router,
};
use serde_json::json;
use sms_core::{InboundRegistry, SendRequest};
use sms_plivo::PlivoClient;
use sms_twilio::TwilioClient;
use sms_web_generic::WebhookProcessor;
use smskit::{
    config::AppConfig,
    rate_limiter::{RateLimiter, DefaultKeyGenerator, RateLimitResult},
};
use std::sync::Arc;
use tracing::{info, warn};

#[derive(Clone)]
struct AppState {
    plivo: Arc<PlivoClient>,
    twilio: Arc<TwilioClient>,
    processor: WebhookProcessor,
    rate_limiter: Arc<RateLimiter>,
    config: AppConfig,
}

async fn send_sms(
    Path(provider): Path<String>,
    State(state): State<AppState>,
    Json(payload): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    // Extract request data
    let to = payload.get("to").and_then(|v| v.as_str()).ok_or(StatusCode::BAD_REQUEST)?;
    let from = payload.get("from").and_then(|v| v.as_str()).ok_or(StatusCode::BAD_REQUEST)?;
    let text = payload.get("text").and_then(|v| v.as_str()).ok_or(StatusCode::BAD_REQUEST)?;

    // Check rate limit
    let rate_key = format!("{}:{}", provider, "client_ip"); // In real app, extract client IP
    match state.rate_limiter.check_rate_limit(&rate_key).await {
        RateLimitResult::Limited { retry_after } => {
            warn!("Rate limit exceeded for {}", rate_key);
            return Err(StatusCode::TOO_MANY_REQUESTS);
        },
        RateLimitResult::Allowed => {},
    }

    let request = SendRequest { to, from, text };

    let response = match provider.as_str() {
        "plivo" => state.plivo.send(request).await,
        "twilio" => state.twilio.send(request).await,
        _ => return Err(StatusCode::NOT_FOUND),
    };

    match response {
        Ok(resp) => {
            info!("SMS sent via {}: {}", provider, resp.id);
            Ok(Json(json!({
                "id": resp.id,
                "provider": resp.provider,
                "status": "sent"
            })))
        },
        Err(e) => {
            warn!("Failed to send SMS via {}: {}", provider, e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

async fn handle_webhook(
    Path(provider): Path<String>,
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    body: axum::body::Bytes,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let generic_headers: Vec<(String, String)> = headers
        .iter()
        .map(|(name, value)| (
            name.to_string(),
            value.to_str().unwrap_or("").to_string()
        ))
        .collect();

    let response = state.processor.process_webhook(&provider, generic_headers, &body);

    match response.status.as_u16() {
        200..=299 => {
            info!("Webhook processed for provider: {}", provider);
            let json: serde_json::Value = serde_json::from_str(&response.body)
                .unwrap_or_else(|_| json!({"status": "processed"}));
            Ok(Json(json))
        },
        404 => {
            warn!("Unknown provider: {}", provider);
            Err(StatusCode::NOT_FOUND)
        },
        401 => {
            warn!("Webhook signature verification failed for {}", provider);
            Err(StatusCode::UNAUTHORIZED)
        },
        _ => {
            warn!("Webhook processing error for {}: {}", provider, response.body);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

async fn health_check(State(state): State<AppState>) -> Json<serde_json::Value> {
    Json(json!({
        "status": "healthy",
        "version": env!("CARGO_PKG_VERSION"),
        "rate_limiting": state.config.rate_limit.enabled,
    }))
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing
    tracing_subscriber::fmt::init();

    // Load configuration
    let config = AppConfig::load().unwrap_or_default();

    // Initialize SMS clients
    let plivo = Arc::new(PlivoClient::new(
        config.providers.plivo.as_ref().unwrap().auth_id.clone(),
        config.providers.plivo.as_ref().unwrap().auth_token.clone(),
        config.providers.plivo.as_ref().unwrap().base_url.clone(),
    ));

    let twilio = Arc::new(TwilioClient::new(
        config.providers.twilio.as_ref().unwrap().account_sid.clone(),
        config.providers.twilio.as_ref().unwrap().auth_token.clone(),
        config.providers.twilio.as_ref().unwrap().base_url.clone(),
    ));

    // Setup webhook registry
    let registry = InboundRegistry::new()
        .with(plivo.clone())
        .with(twilio.clone());

    let processor = WebhookProcessor::new(registry);

    // Setup rate limiter
    let rate_limiter = Arc::new(RateLimiter::new(config.rate_limit.clone()));

    // Create application state
    let state = AppState {
        plivo,
        twilio,
        processor,
        rate_limiter,
        config: config.clone(),
    };

    // Build router
    let app = Router::new()
        .route("/health", get(health_check))
        .route("/send/:provider", post(send_sms))
        .route("/webhook/:provider", post(handle_webhook))
        .with_state(state);

    // Start server
    let addr = format!("{}:{}", config.server.host, config.server.port);
    info!("Starting SMS Kit server on {}", addr);

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
```

### Testing Providers

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use sms_core::{SendRequest, SmsError};

    #[tokio::test]
    async fn test_plivo_send() {
        let client = PlivoClient::new(
            "test_id".to_string(),
            "test_token".to_string(),
            None,
        );

        let request = SendRequest {
            to: "+1234567890",
            from: "+0987654321",
            text: "Test message",
        };

        // This would fail in tests without real credentials
        // In practice, you'd mock the HTTP client
        match client.send(request).await {
            Ok(response) => {
                assert!(!response.id.is_empty());
                assert_eq!(response.provider, "plivo");
            },
            Err(SmsError::Auth(_)) => {
                // Expected with test credentials
                println!("Auth error expected in tests");
            },
            Err(e) => panic!("Unexpected error: {}", e),
        }
    }
}
```

## Contributing

### Development Setup

1. Clone the repository:

   ```bash
   git clone https://github.com/your-org/smskit.git
   cd smskit
   ```

2. Install Rust toolchain:

   ```bash
   curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
   rustup component add rustfmt clippy
   ```

3. Run tests:

   ```bash
   cargo test --all-features
   cargo test --test '*'  # Integration tests
   ```

4. Check formatting and linting:

   ```bash
   cargo fmt -- --check
   cargo clippy --all-targets --all-features -- -D warnings
   ```

5. Run benchmarks:

   ```bash
   cargo bench
   ```

### Adding a New Provider

1. Create a new crate in `crates/sms-{provider}/`:

   ```
   crates/sms-newprovider/
   ├── Cargo.toml
   └── src/
       └── lib.rs
   ```

2. Implement the required traits:

   ```rust
   use async_trait::async_trait;
   use sms_core::{SmsClient, InboundWebhook, SendRequest, SendResponse, SmsError, InboundMessage, Headers};

   pub struct NewProviderClient {
       // Client configuration
   }

   #[async_trait]
   impl SmsClient for NewProviderClient {
       async fn send(&self, req: SendRequest<'_>) -> Result<SendResponse, SmsError> {
           // Implementation
       }
   }

   #[async_trait]
   impl InboundWebhook for NewProviderClient {
       fn provider(&self) -> &'static str {
           "newprovider"
       }

       fn parse_inbound(&self, headers: &Headers, body: &[u8]) -> Result<InboundMessage, SmsError> {
           // Implementation
       }

       fn verify(&self, headers: &Headers, body: &[u8]) -> Result<(), SmsError> {
           // Signature verification
       }
   }
   ```

3. Add tests and documentation

4. Submit a pull request

### Guidelines

- Follow Rust naming conventions
- Add comprehensive tests for all functionality
- Include documentation with examples
- Ensure all CI checks pass
- Update the main README if needed

## License

Licensed under the MIT License. See [LICENSE](LICENSE) for details.
