//! Token-bucket rate limiter for the enrollment endpoint.

use std::sync::Mutex;
use std::time::Instant;

/// Simple token-bucket rate limiter.
pub struct RateLimiter {
    inner: Mutex<RateLimiterInner>,
}

struct RateLimiterInner {
    tokens: f64,
    max_tokens: f64,
    refill_rate: f64, // tokens per second
    last_refill: Instant,
}

impl RateLimiter {
    /// Create a rate limiter that allows `max_per_minute` requests per minute.
    pub fn new(max_per_minute: u32) -> Self {
        let max = f64::from(max_per_minute);
        Self {
            inner: Mutex::new(RateLimiterInner {
                tokens: max,
                max_tokens: max,
                refill_rate: max / 60.0,
                last_refill: Instant::now(),
            }),
        }
    }

    /// Try to consume one token. Returns `true` if allowed, `false` if rate-limited.
    pub fn try_acquire(&self) -> bool {
        let mut inner = self.inner.lock().unwrap();
        let now = Instant::now();
        let elapsed = now.duration_since(inner.last_refill).as_secs_f64();
        inner.tokens = (inner.tokens + elapsed * inner.refill_rate).min(inner.max_tokens);
        inner.last_refill = now;

        if inner.tokens >= 1.0 {
            inner.tokens -= 1.0;
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allows_within_limit() {
        let limiter = RateLimiter::new(100);
        for _ in 0..100 {
            assert!(limiter.try_acquire());
        }
    }

    #[test]
    fn rejects_over_limit() {
        let limiter = RateLimiter::new(10);
        for _ in 0..10 {
            assert!(limiter.try_acquire());
        }
        assert!(!limiter.try_acquire());
    }
}
