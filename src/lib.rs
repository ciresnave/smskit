//! # SMS Kit
//!
//! A comprehensive, production-ready multi-provider SMS abstraction library for Rust.
//!
//! ## Features
//!
//! - **Multi-provider support**: Plivo, Twilio, AWS SNS
//! - **Unified dispatch**: [`SmsRouter`](sms_core::SmsRouter) routes sends to named providers
//! - **Fallback chaining**: [`FallbackClient`](sms_core::FallbackClient) tries providers in order
//! - **Owned requests**: [`OwnedSendRequest`](sms_core::OwnedSendRequest) for async-friendly data
//! - **`from_env()` constructors**: Read credentials from environment variables
//! - **Framework agnostic**: Works with Axum, Warp, Actix, Rocket, Poem, Hyper, Tide
//! - **Webhook processing**: Unified inbound webhook handling with signature verification
//! - **Rate limiting**: Built-in per-provider rate limiting
//! - **Configuration**: Layered TOML + env var configuration
//!
//! ## Quick Start
//!
//! ```rust,ignore
//! use smskit::prelude::*;
//! use sms_plivo::PlivoClient;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     // Create from explicit credentials...
//!     let client = PlivoClient::new("your_auth_id", "your_auth_token");
//!     // ...or from environment variables (PLIVO_AUTH_ID, PLIVO_AUTH_TOKEN)
//!     let client = PlivoClient::from_env()?;
//!
//!     let response = client.send(SendRequest {
//!         to: "+1234567890",
//!         from: "+0987654321",
//!         text: "Hello from SMS Kit!",
//!     }).await?;
//!
//!     println!("Message sent with ID: {}", response.id);
//!     Ok(())
//! }
//! ```
//!
//! ## Unified Dispatch
//!
//! Route sends to named providers — callers don't need provider crate imports:
//!
//! ```rust,ignore
//! use smskit::prelude::*;
//!
//! let router = SmsRouter::new()
//!     .with("plivo", plivo_client)
//!     .with("twilio", twilio_client)
//!     .default_provider("plivo");
//!
//! // Explicit dispatch:
//! router.send_via("twilio", request).await?;
//! // Or use the default:
//! router.send(request).await?;
//! ```
//!
//! ## Fallback Chaining
//!
//! Try providers in order — returns the first success:
//!
//! ```rust,ignore
//! use smskit::prelude::*;
//! use std::sync::Arc;
//!
//! let client = FallbackClient::new(vec![
//!     Arc::new(primary_client),
//!     Arc::new(backup_client),
//! ]);
//! let response = client.send(request).await?;
//! ```
//!
//! ## Owned Requests for Async Contexts
//!
//! [`OwnedSendRequest`](sms_core::OwnedSendRequest) avoids lifetime friction
//! when holding requests across `.await` points:
//!
//! ```rust,ignore
//! let req = OwnedSendRequest::new("+1234567890", "+0987654321", "Hello!");
//! let response = client.send(req.as_ref()).await?;
//! ```
//!
//! ## Configuration
//!
//! Layered configuration from TOML files and `SMSKIT_`-prefixed environment variables:
//!
//! ```rust,ignore
//! use smskit::config::AppConfig;
//!
//! let config = AppConfig::load()?;
//! ```

pub mod config;
pub mod rate_limiter;

pub use config::*;

/// Common imports for SMS Kit usage.
///
/// Pulls in everything from `sms_core` (traits, request/response types, errors)
/// plus the configuration and rate-limiting types from this crate.
pub mod prelude {
    pub use crate::config::{
        AppConfig, LoggingConfig, ProvidersConfig, SecurityConfig, ServerConfig,
    };
    pub use crate::rate_limiter::{
        DefaultKeyGenerator, KeyGenerator, RateLimitMiddleware, RateLimitResult, RateLimiter,
    };
    // Re-export everything from sms-core, which now includes:
    //   SmsClient, SendRequest, OwnedSendRequest, SendResponse,
    //   SmsRouter, FallbackClient, InboundWebhook, InboundRegistry, etc.
    pub use sms_core::*;
}
