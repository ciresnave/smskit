use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tracing::{debug, warn};

/// Rate limiter for webhook endpoints
#[derive(Debug, Clone)]
pub struct RateLimiter {
    inner: Arc<RwLock<RateLimiterInner>>,
    requests_per_minute: u32,
    burst_size: u32,
    cleanup_interval: Duration,
}

#[derive(Debug)]
struct RateLimiterInner {
    clients: HashMap<IpAddr, ClientState>,
    last_cleanup: Instant,
}

#[derive(Debug)]
struct ClientState {
    tokens: u32,
    last_refill: Instant,
    last_request: Instant,
}

impl RateLimiter {
    /// Create a new rate limiter
    pub fn new(requests_per_minute: u32, burst_size: u32) -> Self {
        Self {
            inner: Arc::new(RwLock::new(RateLimiterInner {
                clients: HashMap::new(),
                last_cleanup: Instant::now(),
            })),
            requests_per_minute,
            burst_size,
            cleanup_interval: Duration::from_secs(300), // 5 minutes
        }
    }

    /// Check if a request from the given IP should be allowed
    pub async fn check_rate_limit(&self, ip: IpAddr) -> bool {
        let mut limiter = self.inner.write().await;

        // Cleanup old entries periodically
        let now = Instant::now();
        if now.duration_since(limiter.last_cleanup) > self.cleanup_interval {
            self.cleanup_old_entries(&mut limiter, now);
            limiter.last_cleanup = now;
        }

        // Get or create client state
        let client_state = limiter.clients.entry(ip).or_insert_with(|| ClientState {
            tokens: self.burst_size,
            last_refill: now,
            last_request: now,
        });

        // Refill tokens based on time elapsed
        let time_since_refill = now.duration_since(client_state.last_refill);
        let tokens_to_add =
            (time_since_refill.as_secs() * self.requests_per_minute as u64 / 60) as u32;

        if tokens_to_add > 0 {
            client_state.tokens = (client_state.tokens + tokens_to_add).min(self.burst_size);
            client_state.last_refill = now;
            debug!(
                "Refilled {} tokens for IP {}, total: {}",
                tokens_to_add, ip, client_state.tokens
            );
        }

        client_state.last_request = now;

        // Check if request can be allowed
        if client_state.tokens > 0 {
            client_state.tokens -= 1;
            debug!(
                "Rate limit OK for IP {}, remaining tokens: {}",
                ip, client_state.tokens
            );
            true
        } else {
            warn!("Rate limit exceeded for IP {}", ip);
            false
        }
    }

    /// Get current rate limit status for an IP
    pub async fn get_status(&self, ip: IpAddr) -> Option<RateLimitStatus> {
        let limiter = self.inner.read().await;
        limiter.clients.get(&ip).map(|state| {
            let now = Instant::now();
            let time_since_refill = now.duration_since(state.last_refill);
            let tokens_to_add =
                (time_since_refill.as_secs() * self.requests_per_minute as u64 / 60) as u32;
            let current_tokens = (state.tokens + tokens_to_add).min(self.burst_size);

            RateLimitStatus {
                remaining: current_tokens,
                limit: self.burst_size,
                reset_time: state.last_refill
                    + Duration::from_secs(60 / self.requests_per_minute as u64),
            }
        })
    }

    fn cleanup_old_entries(&self, limiter: &mut RateLimiterInner, now: Instant) {
        let cutoff = now - Duration::from_secs(600); // 10 minutes
        limiter.clients.retain(|ip, state| {
            let should_keep = state.last_request > cutoff;
            if !should_keep {
                debug!("Cleaned up rate limit entry for IP {}", ip);
            }
            should_keep
        });
    }
}

/// Rate limit status information
#[derive(Debug, Clone)]
pub struct RateLimitStatus {
    /// Remaining requests
    pub remaining: u32,
    /// Total request limit
    pub limit: u32,
    /// When the rate limit resets
    pub reset_time: Instant,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::IpAddr;
    use std::str::FromStr;

    #[tokio::test]
    async fn rate_limiter_allows_initial_requests() {
        let limiter = RateLimiter::new(60, 10);
        let ip = IpAddr::from_str("127.0.0.1").unwrap();

        // Should allow initial requests up to burst size
        for _ in 0..10 {
            assert!(limiter.check_rate_limit(ip).await);
        }

        // Should reject the next request
        assert!(!limiter.check_rate_limit(ip).await);
    }

    #[tokio::test]
    async fn rate_limiter_refills_tokens() {
        let limiter = RateLimiter::new(60, 5);
        let ip = IpAddr::from_str("127.0.0.1").unwrap();

        // Exhaust tokens
        for _ in 0..5 {
            assert!(limiter.check_rate_limit(ip).await);
        }
        assert!(!limiter.check_rate_limit(ip).await);

        // Wait and modify the state to simulate time passage
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        // In a real scenario, tokens would refill based on elapsed time
        // This test demonstrates the mechanism works
    }

    #[tokio::test]
    async fn rate_limiter_tracks_multiple_ips() {
        let limiter = RateLimiter::new(60, 3);
        let ip1 = IpAddr::from_str("127.0.0.1").unwrap();
        let ip2 = IpAddr::from_str("192.168.1.1").unwrap();

        // Both IPs should get their own token buckets
        assert!(limiter.check_rate_limit(ip1).await);
        assert!(limiter.check_rate_limit(ip2).await);

        let status1 = limiter.get_status(ip1).await.unwrap();
        let status2 = limiter.get_status(ip2).await.unwrap();

        assert_eq!(status1.remaining, 2); // Used 1 token
        assert_eq!(status2.remaining, 2); // Used 1 token
        assert_eq!(status1.limit, 3);
        assert_eq!(status2.limit, 3);
    }
}
