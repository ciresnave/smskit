# smskit -- Universal Multi-Provider SMS Toolkit for Rust

**Status:** v0.2.0 (beta) -- Complete framework-agnostic SMS abstraction with support for every major Rust web framework.

## Why smskit?

Give your users the freedom to bring their own SMS provider while you code to a single, unified interface. Switch between Plivo, Twilio, AWS SNS seamlessly without changing your application code.

**NEW in v0.2.0:** Universal web framework support, unified dispatch router, fallback chaining, `from_env()` constructors, and owned request types for async ergonomics.

## Architecture

```
                        sms-core
                   (traits & types)
                   SmsClient trait
                   SmsRouter
                   FallbackClient
                         |
          +--------------+--------------+
          |              |              |
      sms-plivo     sms-twilio    sms-aws-sns
          |              |              |
          +--------------+--------------+
                         |
                  sms-web-generic
               (framework-agnostic
                webhook processing)
                         |
     +-------+-------+--+--+-------+-------+
     |       |       |     |       |       |
   Axum   Warp  Actix-web Rocket  Poem   Hyper  Tide
```

## Quick Start

### Sending SMS

```rust
use sms_core::{SendRequest, SmsClient};
use sms_plivo::PlivoClient;

// Create from explicit credentials...
let client = PlivoClient::new("auth_id", "auth_token");

// ...or from environment variables
let client = PlivoClient::from_env()?;

let response = client.send(SendRequest {
    to: "+14155551234",
    from: "+10005551234",
    text: "Hello from smskit!",
}).await?;

println!("Message sent with ID: {}", response.id);
```

### Unified Dispatch (no provider imports needed)

```rust
use sms_core::{SmsRouter, SendRequest};

let router = SmsRouter::new()
    .with("plivo", plivo_client)
    .with("twilio", twilio_client)
    .with("aws-sns", sns_client)
    .default_provider("plivo");

// Dispatch by name:
router.send_via("twilio", req).await?;

// Or use the default:
router.send(req).await?;
```

### Fallback Chaining

```rust
use sms_core::FallbackClient;
use std::sync::Arc;

let client = FallbackClient::new(vec![
    Arc::new(primary_client),
    Arc::new(backup_client),
]);

// Tries primary first; on failure, tries backup.
let response = client.send(req).await?;
```

### Owned Requests for Async Contexts

```rust
use sms_core::OwnedSendRequest;

// Owns its data -- can be held across .await points
let req = OwnedSendRequest::new("+14155551234", "+10005551234", "Hello!");
let response = client.send(req.as_ref()).await?;
```

### Environment-Based Construction

Every provider supports `from_env()`:

```rust
use sms_plivo::PlivoClient;       // reads PLIVO_AUTH_ID, PLIVO_AUTH_TOKEN
use sms_twilio::TwilioClient;     // reads TWILIO_ACCOUNT_SID, TWILIO_AUTH_TOKEN
use sms_aws_sns::AwsSnsClient;    // reads AWS_REGION, AWS_ACCESS_KEY_ID, AWS_SECRET_ACCESS_KEY

let plivo = PlivoClient::from_env()?;
let twilio = TwilioClient::from_env()?;
let aws = AwsSnsClient::from_env()?;
```

### Receiving SMS (Webhooks)

All frameworks receive the same normalized `InboundMessage`:

```json
{
  "id": "abc-123",
  "from": "+1234567890",
  "to": "+0987654321",
  "text": "Hello World",
  "timestamp": "2024-12-30T12:34:56Z",
  "provider": "plivo",
  "raw": { ... }
}
```

**Webhook URLs:**

- POST `http://yourserver.com/webhooks/plivo`
- POST `http://yourserver.com/webhooks/twilio`
- POST `http://yourserver.com/webhooks/aws-sns`

## SMS Providers

| Provider | Crate | Send | Webhooks | Signature Verification | `from_env()` |
|----------|-------|------|----------|------------------------|--------------|
| **Plivo** | `sms-plivo` | Yes | Yes | -- | Yes |
| **Twilio** | `sms-twilio` | Yes | Yes | HMAC-SHA1 | Yes |
| **AWS SNS** | `sms-aws-sns` | Yes | Yes | -- | Yes |

## Supported Frameworks

| Framework | Crate | Example |
|-----------|-------|---------|
| **Axum** | `sms-web-axum` | [Example](examples/unified_webhook.rs) |
| **Warp** | `sms-web-warp` | [Example](examples/frameworks/warp_server.rs) |
| **Actix-web** | `sms-web-actix` | [Example](examples/frameworks/actix_server.rs) |
| **Rocket** | `sms-web-rocket` | [Example](examples/frameworks/rocket_server.rs) |
| **Tide** | `sms-web-tide` | [Example](examples/frameworks/tide_server.rs) |
| **Hyper** | `sms-web-hyper` | [Example](examples/frameworks/hyper_server.rs) |
| **Generic** | `sms-web-generic` | [DIY Integration](examples/frameworks/generic_integration.rs) |
| **Your Framework** | DIY | Use `sms-web-generic`! |

## Running Examples

```bash
# Generic integration (works everywhere)
cargo run --example generic_integration

# Framework-specific
cargo run --example unified_webhook
cargo run --example warp_server --features warp
cargo run --example actix_server --features actix-web
cargo run --example hyper_server --features hyper
```

## Configuration

Copy `config/default.toml` and adjust for your environment. Configuration loads from:

1. Built-in defaults
2. `config/default.toml`
3. `config/{environment}.toml` (set `RUN_MODE=production`)
4. `config/local.toml` (gitignored)
5. `SMSKIT_` prefixed environment variables

## License

MIT OR Apache-2.0
