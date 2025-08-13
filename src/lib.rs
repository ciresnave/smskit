//! # SMS Kit
//!
//! A comprehensive, production-ready multi-provider SMS abstraction library for Rust.
//!
//! ## Features
//!
//! - **Multi-provider support**: Plivo, Twilio, AWS SNS, and more
//! - **Framework agnostic**: Works with Axum, Warp, Actix, or any HTTP framework
//! - **Webhook processing**: Unified webhook handling for inbound SMS
//! - **Type safety**: Strongly typed SMS operations and responses
//! - **Rate limiting**: Built-in rate limiting with per-provider configuration
//! - **Comprehensive configuration**: Environment-based configuration management
//! - **Observability**: Structured logging and tracing support
//! - **Production ready**: Security, error handling, and reliability features
//!
//! ## Quick Start
//!
//! ```rust,ignore
//! use smskit::prelude::*;
//! use sms_plivo::PlivoClient;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let client = PlivoClient::new(
//!         "your_auth_id".to_string(),
//!         "your_auth_token".to_string(),
//!         None,
//!     );
//!
//!     let response = client.send(SendRequest {
//!         to: "+1234567890",
//!         from: "+0987654321",
//!         text: "Hello from SMS Kit!"
//!     }).await?;
//!
//!     println!("Message sent with ID: {}", response.id);
//!     Ok(())
//! }
//! ```
//!
//! ## Configuration
//!
//! SMS Kit uses a comprehensive configuration system that supports environment variables:
//!
//! ```rust,ignore
//! use smskit::config::AppConfig;
//!
//! let config = AppConfig::from_env()?;
//! println!("Rate limit: {} requests per {}s",
//!          config.rate_limit.max_requests,
//!          config.rate_limit.window_seconds);
//! ```

pub mod config;
pub mod rate_limiter;

pub use config::*;

/// Common imports for SMS Kit usage
pub mod prelude {
    pub use crate::config::{
        AppConfig, LoggingConfig, ProvidersConfig, SecurityConfig, ServerConfig,
    };
    pub use crate::rate_limiter::{
        DefaultKeyGenerator, KeyGenerator, RateLimitMiddleware, RateLimitResult, RateLimiter,
    };
    pub use sms_core::*;
}
