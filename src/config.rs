use config::{Config, ConfigError, Environment, File};
use serde::{Deserialize, Serialize};
use std::env;

/// Application configuration
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct AppConfig {
    /// Server configuration
    pub server: ServerConfig,
    /// SMS providers configuration
    pub providers: ProvidersConfig,
    /// Security configuration
    pub security: SecurityConfig,
    /// Logging configuration
    pub logging: LoggingConfig,
    /// Rate limiting configuration
    pub rate_limit: RateLimitConfig,
}

/// Server configuration
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ServerConfig {
    /// Server host (default: 0.0.0.0)
    pub host: String,
    /// Server port (default: 3000)
    pub port: u16,
    /// Request timeout in seconds (default: 30)
    pub timeout_seconds: u64,
}

/// SMS providers configuration
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ProvidersConfig {
    /// Plivo configuration
    pub plivo: Option<PlivoConfig>,
    /// Twilio configuration
    pub twilio: Option<TwilioConfig>,
    /// AWS SNS configuration
    pub aws_sns: Option<AwsSnsConfig>,
}

/// Plivo provider configuration
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct PlivoConfig {
    /// Plivo Auth ID
    pub auth_id: String,
    /// Plivo Auth Token
    pub auth_token: String,
    /// Webhook signature validation (default: true)
    pub verify_signatures: bool,
}

/// Twilio provider configuration
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct TwilioConfig {
    /// Twilio Account SID
    pub account_sid: String,
    /// Twilio Auth Token
    pub auth_token: String,
    /// Webhook signature validation (default: true)
    pub verify_signatures: bool,
}

/// AWS SNS provider configuration
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct AwsSnsConfig {
    /// AWS Access Key ID
    pub access_key_id: String,
    /// AWS Secret Access Key
    pub secret_access_key: String,
    /// AWS Region
    pub region: String,
}

/// Security configuration
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct SecurityConfig {
    /// Enable signature verification (default: true)
    pub verify_signatures: bool,
    /// Maximum request body size in bytes (default: 1MB)
    pub max_body_size: usize,
    /// Request timeout in seconds (default: 30)
    pub request_timeout: u64,
}

/// Logging configuration
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct LoggingConfig {
    /// Log level (default: info)
    pub level: String,
    /// Log format: json or pretty (default: json)
    pub format: String,
}

/// Rate limiting configuration
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct RateLimitConfig {
    /// Enable rate limiting (default: true)
    pub enabled: bool,
    /// Requests per minute (default: 100)
    pub requests_per_minute: u32,
    /// Burst size (default: 10)
    pub burst_size: u32,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            host: "0.0.0.0".to_string(),
            port: 3000,
            timeout_seconds: 30,
        }
    }
}

impl Default for SecurityConfig {
    fn default() -> Self {
        Self {
            verify_signatures: true,
            max_body_size: 1024 * 1024, // 1MB
            request_timeout: 30,
        }
    }
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            level: "info".to_string(),
            format: "json".to_string(),
        }
    }
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            requests_per_minute: 100,
            burst_size: 10,
        }
    }
}

impl AppConfig {
    /// Load configuration from files and environment variables
    pub fn load() -> Result<Self, ConfigError> {
        let run_mode = env::var("RUN_MODE").unwrap_or_else(|_| "development".into());

        let s = Config::builder()
            // Start with default configuration
            .add_source(Config::try_from(&AppConfig::default())?)
            // Add configuration file based on environment
            .add_source(File::with_name("config/default").required(false))
            .add_source(File::with_name(&format!("config/{}", run_mode)).required(false))
            // Add local configuration file (gitignored)
            .add_source(File::with_name("config/local").required(false))
            // Add environment variables (prefixed with SMSKIT_)
            .add_source(Environment::with_prefix("SMSKIT").separator("__"))
            .build()?;

        s.try_deserialize()
    }
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            server: ServerConfig::default(),
            providers: ProvidersConfig {
                plivo: None,
                twilio: None,
                aws_sns: None,
            },
            security: SecurityConfig::default(),
            logging: LoggingConfig::default(),
            rate_limit: RateLimitConfig::default(),
        }
    }
}
