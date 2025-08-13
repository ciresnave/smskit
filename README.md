# smskit – Universal Multi‑Provider SMS Toolkit for Rust 🚀

**Status:** v0.2.0 (beta) – Complete framework-agnostic SMS abstraction with support for every major Rust web framework.

## 🎯 Why smskit?

Give your users the freedom to bring their own SMS provider while you code to a single, unified interface. Switch between Plivo, Twilio, AWS SNS, or any provider without changing your application code.

**🔥 NEW in v0.2.0:** Universal web framework support! Works with Axum, Warp, Actix-web, Rocket, Tide, Hyper, or ANY framework you can imagine.

## 📦 Architecture

```
┌─────────────────┐    ┌──────────────────┐    ┌─────────────────────┐
│   Your App      │    │   sms-core       │    │  SMS Providers      │
│                 │────│  (traits & types)│────│  • Plivo           │
│ • Business Logic│    │                  │    │  • Twilio (soon)   │
│ • Web Routes    │    └──────────────────┘    │  • AWS SNS (soon)  │
└─────────────────┘             │              └─────────────────────┘
         │                      │
         ▼                      ▼
┌─────────────────┐    ┌──────────────────┐
│ Web Framework   │    │ sms-web-generic  │
│ Adapters        │────│ (core logic)     │
│ • Axum          │    │                  │
│ • Warp          │    │ Framework-       │
│ • Actix-web     │    │ agnostic         │
│ • Rocket        │    │ processing       │
│ • Tide          │    │                  │
│ • Hyper         │    │                  │
│ • + ANY other   │    │                  │
└─────────────────┘    └──────────────────┘
```

## 🚀 Quick Start

### Option 1: Use Your Favorite Framework

<details>
<summary><strong>Axum</strong> (Click to expand)</summary>

```toml
[dependencies]
sms-core = "0.2"
sms-plivo = "0.2"
sms-web-axum = "0.2"
axum = "0.7"
tokio = { version = "1.0", features = ["full"] }
```

```rust
use std::sync::Arc;
use axum::{routing::post, Router};
use sms_core::InboundRegistry;
use sms_plivo::PlivoClient;
use sms_web_axum::{unified_webhook, AppState};

#[tokio::main]
async fn main() {
    let plivo = PlivoClient::new("your_auth_id", "your_auth_token");
    let registry = InboundRegistry::new().with(Arc::new(plivo));
    let state = AppState { registry };

    let app = Router::new()
        .route("/webhooks/:provider", post(unified_webhook))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
```

</details>

### Option 2: Framework-Agnostic Integration 🔥

**Works with ANY web framework** - just adapt the request/response types:

```rust
use sms_web_generic::WebhookProcessor;

// This works with ANY web framework!
async fn handle_sms_webhook(provider: String, headers: Vec<(String, String)>, body: &[u8]) -> YourFrameworkResponse {
    let processor = WebhookProcessor::new(your_registry);
    let response = processor.process_webhook(&provider, headers, body);

    // Convert to your framework's response type
    response.into()
}
```

## 📋 Supported Frameworks

| Framework | Crate | Status | Example |
|-----------|--------|--------|---------|
| **Axum** | `sms-web-axum` | ✅ Complete | [Example](examples/unified_webhook.rs) |
| **Warp** | `sms-web-warp` | ✅ Complete | [Example](examples/frameworks/warp_server.rs) |
| **Actix-web** | `sms-web-actix` | ✅ Complete | [Example](examples/frameworks/actix_server.rs) |
| **Rocket** | `sms-web-rocket` | ✅ Complete | [Example](examples/frameworks/rocket_server.rs) |
| **Tide** | `sms-web-tide` | ✅ Complete | [Example](examples/frameworks/tide_server.rs) |
| **Hyper** | `sms-web-hyper` | ✅ Complete | [Example](examples/frameworks/hyper_server.rs) |
| **Generic** | `sms-web-generic` | ✅ Complete | [DIY Integration](examples/frameworks/generic_integration.rs) |
| **Your Framework** | DIY | ✅ Supported | Use `sms-web-generic`! |

## 🔌 SMS Providers

| Provider | Crate | Send SMS | Webhooks | Status |
|----------|--------|----------|----------|--------|
| **Plivo** | `sms-plivo` | ✅ | ✅ | Complete |
| **Twilio** | `sms-twilio` | 🔄 | 🔄 | Coming Soon |
| **AWS SNS** | `sms-aws-sns` | 🔄 | 🔄 | Coming Soon |

## 💡 Usage Examples

### Sending SMS

```rust
use sms_core::{SendRequest, SmsClient};
use sms_plivo::PlivoClient;

let client = PlivoClient::new("auth_id", "auth_token");
let response = client.send(SendRequest {
    to: "+1234567890",
    from: "+0987654321",
    text: "Hello from smskit!"
}).await?;

println!("Message sent with ID: {}", response.id);
```

### Receiving SMS (Webhooks)

All frameworks receive the same normalized `InboundMessage`:

```rust
{
  "id": "abc-123",
  "from": "+1234567890",
  "to": "+0987654321",
  "text": "Hello World",
  "timestamp": "2024-12-30T12:34:56Z",
  "provider": "plivo",
  "raw": { /* original provider payload */ }
}
```

**Webhook URLs:**

- POST `http://yourserver.com/webhooks/plivo`
- POST `http://yourserver.com/webhooks/twilio`
- POST `http://yourserver.com/webhooks/{any_provider}`

## 🏃‍♂️ Running Examples

```bash
# Test the generic integration (works everywhere!)
cargo run --example generic_integration

# Try with specific frameworks
cargo run --example warp_server --features warp
cargo run --example actix_server --features actix-web
cargo run --example hyper_server --features hyper

# Original Axum example
cargo run --example unified_webhook
```
