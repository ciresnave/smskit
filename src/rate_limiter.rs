use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::time::sleep;
use tracing::{debug, warn};

/// Configuration for rate limiting
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimitConfig {
    /// Maximum number of requests per window
    pub max_requests: u32,
    /// Window duration in seconds
    pub window_seconds: u64,
    /// Whether to enable rate limiting
    pub enabled: bool,
    /// Per-provider rate limits (overrides global settings)
    pub per_provider: HashMap<String, ProviderRateLimit>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderRateLimit {
    pub max_requests: u32,
    pub window_seconds: u64,
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            max_requests: 100,
            window_seconds: 60,
            enabled: true,
            per_provider: HashMap::new(),
        }
    }
}

/// Rate limiter implementation using token bucket algorithm
#[derive(Debug)]
pub struct RateLimiter {
    config: RateLimitConfig,
    buckets: Arc<Mutex<HashMap<String, TokenBucket>>>,
}

#[derive(Debug)]
struct TokenBucket {
    tokens: u32,
    last_refill: Instant,
    max_tokens: u32,
    refill_rate: f64, // tokens per second
}

impl TokenBucket {
    fn new(max_tokens: u32, window_seconds: u64) -> Self {
        let refill_rate = max_tokens as f64 / window_seconds as f64;
        Self {
            tokens: max_tokens,
            last_refill: Instant::now(),
            max_tokens,
            refill_rate,
        }
    }

    fn try_consume(&mut self) -> bool {
        self.refill();

        if self.tokens > 0 {
            self.tokens -= 1;
            true
        } else {
            false
        }
    }

    fn refill(&mut self) {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_refill).as_secs_f64();

        if elapsed > 0.0 {
            let tokens_to_add = (elapsed * self.refill_rate).floor() as u32;
            self.tokens = (self.tokens + tokens_to_add).min(self.max_tokens);
            self.last_refill = now;
        }
    }
}

impl RateLimiter {
    pub fn new(config: RateLimitConfig) -> Self {
        Self {
            config,
            buckets: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Check if a request should be rate limited
    pub async fn check_rate_limit(&self, key: &str) -> RateLimitResult {
        if !self.config.enabled {
            return RateLimitResult::Allowed;
        }

        // Determine rate limit settings for this key
        let (max_requests, window_seconds) =
            if let Some(provider_limit) = self.get_provider_limit(key) {
                (provider_limit.max_requests, provider_limit.window_seconds)
            } else {
                (self.config.max_requests, self.config.window_seconds)
            };

        let mut buckets = self.buckets.lock().unwrap();
        let bucket = buckets
            .entry(key.to_string())
            .or_insert_with(|| TokenBucket::new(max_requests, window_seconds));

        if bucket.try_consume() {
            debug!("Rate limit check passed for key: {}", key);
            RateLimitResult::Allowed
        } else {
            warn!("Rate limit exceeded for key: {}", key);
            let retry_after = self.calculate_retry_after(bucket);
            RateLimitResult::Limited { retry_after }
        }
    }

    fn get_provider_limit(&self, key: &str) -> Option<&ProviderRateLimit> {
        // Extract provider name from key (assuming format like "provider:identifier")
        if let Some(provider) = key.split(':').next() {
            self.config.per_provider.get(provider)
        } else {
            None
        }
    }

    fn calculate_retry_after(&self, bucket: &TokenBucket) -> Duration {
        let tokens_needed = 1;
        let seconds_to_wait = tokens_needed as f64 / bucket.refill_rate;
        Duration::from_secs_f64(seconds_to_wait.ceil())
    }

    /// Clean up old buckets to prevent memory leaks
    pub async fn cleanup_old_buckets(&self) {
        let cleanup_interval = Duration::from_secs(300); // 5 minutes
        let max_idle_time = Duration::from_secs(3600); // 1 hour

        loop {
            sleep(cleanup_interval).await;

            let mut buckets = self.buckets.lock().unwrap();
            let now = Instant::now();

            buckets.retain(|key, bucket| {
                let idle_time = now.duration_since(bucket.last_refill);
                if idle_time > max_idle_time {
                    debug!("Cleaning up old rate limit bucket for key: {}", key);
                    false
                } else {
                    true
                }
            });
        }
    }
}

/// Result of rate limit check
#[derive(Debug)]
pub enum RateLimitResult {
    Allowed,
    Limited { retry_after: Duration },
}

/// Rate limiting middleware for different HTTP frameworks
pub trait RateLimitMiddleware {
    type Request;
    type Response;
    type Error;

    async fn apply_rate_limit(
        &self,
        request: Self::Request,
        limiter: &RateLimiter,
    ) -> Result<Self::Request, Self::Response>;
}

/// Generic rate limit key generator
pub trait KeyGenerator {
    fn generate_key(&self, provider: &str, identifier: &str) -> String {
        format!("{}:{}", provider, identifier)
    }

    fn extract_client_ip(&self, headers: &sms_core::Headers) -> Option<String> {
        // Look for common IP headers
        for (name, value) in headers {
            match name.to_lowercase().as_str() {
                "x-forwarded-for" => return Some(value.split(',').next()?.trim().to_string()),
                "x-real-ip" => return Some(value.clone()),
                "cf-connecting-ip" => return Some(value.clone()),
                _ => continue,
            }
        }
        None
    }
}

/// Default key generator implementation
pub struct DefaultKeyGenerator;

impl KeyGenerator for DefaultKeyGenerator {}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::time::{sleep, Duration};

    #[tokio::test]
    async fn test_rate_limiter_allows_requests_within_limit() {
        let config = RateLimitConfig {
            max_requests: 2,
            window_seconds: 1,
            enabled: true,
            per_provider: HashMap::new(),
        };

        let limiter = RateLimiter::new(config);

        // First two requests should be allowed
        match limiter.check_rate_limit("test-key").await {
            RateLimitResult::Allowed => {}
            _ => panic!("First request should be allowed"),
        }

        match limiter.check_rate_limit("test-key").await {
            RateLimitResult::Allowed => {}
            _ => panic!("Second request should be allowed"),
        }

        // Third request should be limited
        match limiter.check_rate_limit("test-key").await {
            RateLimitResult::Limited { .. } => {}
            RateLimitResult::Allowed => panic!("Third request should be limited"),
        }
    }

    #[tokio::test]
    async fn test_rate_limiter_refills_tokens() {
        let config = RateLimitConfig {
            max_requests: 1,
            window_seconds: 1,
            enabled: true,
            per_provider: HashMap::new(),
        };

        let limiter = RateLimiter::new(config);

        // Consume the token
        match limiter.check_rate_limit("test-key").await {
            RateLimitResult::Allowed => {}
            _ => panic!("First request should be allowed"),
        }

        // Next request should be limited
        match limiter.check_rate_limit("test-key").await {
            RateLimitResult::Limited { .. } => {}
            RateLimitResult::Allowed => panic!("Second request should be limited"),
        }

        // Wait for refill
        sleep(Duration::from_millis(1100)).await;

        // Should be allowed again
        match limiter.check_rate_limit("test-key").await {
            RateLimitResult::Allowed => {}
            RateLimitResult::Limited { .. } => panic!("Request after refill should be allowed"),
        }
    }

    #[tokio::test]
    async fn test_disabled_rate_limiter() {
        let config = RateLimitConfig {
            max_requests: 1,
            window_seconds: 1,
            enabled: false,
            per_provider: HashMap::new(),
        };

        let limiter = RateLimiter::new(config);

        // All requests should be allowed when disabled
        for _ in 0..10 {
            match limiter.check_rate_limit("test-key").await {
                RateLimitResult::Allowed => {}
                RateLimitResult::Limited { .. } => {
                    panic!("Requests should be allowed when rate limiting is disabled")
                }
            }
        }
    }

    #[tokio::test]
    async fn test_per_provider_rate_limits() {
        let mut per_provider = HashMap::new();
        per_provider.insert(
            "twilio".to_string(),
            ProviderRateLimit {
                max_requests: 10,
                window_seconds: 60,
            },
        );

        let config = RateLimitConfig {
            max_requests: 5,
            window_seconds: 60,
            enabled: true,
            per_provider,
        };

        let limiter = RateLimiter::new(config);

        // Twilio should use its specific limit (10)
        for i in 1..=6 {
            match limiter.check_rate_limit("twilio:test").await {
                RateLimitResult::Allowed => {}
                RateLimitResult::Limited { .. } => {
                    panic!("Twilio request {} should be allowed (limit is 10)", i)
                }
            }
        }

        // Other provider should use global limit (5)
        for i in 1..=5 {
            match limiter.check_rate_limit("plivo:test").await {
                RateLimitResult::Allowed => {}
                RateLimitResult::Limited { .. } => {
                    panic!("Plivo request {} should be allowed (global limit is 5)", i)
                }
            }
        }

        // 6th request for plivo should be limited
        match limiter.check_rate_limit("plivo:test").await {
            RateLimitResult::Limited { .. } => {}
            RateLimitResult::Allowed => panic!("6th Plivo request should be limited"),
        }
    }
}
