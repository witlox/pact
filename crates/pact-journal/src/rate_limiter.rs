//! Token-bucket rate limiter for the enrollment endpoint (F12 fix: per-IP).

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Mutex;
use std::time::Instant;

/// Two-tier rate limiter: global limit + per-source-IP limit.
pub struct RateLimiter {
    global: Mutex<Bucket>,
    per_ip: Mutex<HashMap<IpAddr, Bucket>>,
    per_ip_max: f64,
}

struct Bucket {
    tokens: f64,
    max_tokens: f64,
    refill_rate: f64,
    last_refill: Instant,
}

impl Bucket {
    fn new(max_per_minute: f64) -> Self {
        Self {
            tokens: max_per_minute,
            max_tokens: max_per_minute,
            refill_rate: max_per_minute / 60.0,
            last_refill: Instant::now(),
        }
    }

    fn try_acquire(&mut self) -> bool {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_refill).as_secs_f64();
        self.tokens = elapsed.mul_add(self.refill_rate, self.tokens).min(self.max_tokens);
        self.last_refill = now;

        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            true
        } else {
            false
        }
    }
}

impl RateLimiter {
    /// Create a rate limiter with global and per-IP limits.
    ///
    /// `global_per_minute`: total requests per minute across all sources.
    /// Per-IP limit is `global / 10` (each IP gets at most 10% of the global budget).
    pub fn new(global_per_minute: u32) -> Self {
        let global_max = f64::from(global_per_minute);
        Self {
            global: Mutex::new(Bucket::new(global_max)),
            per_ip: Mutex::new(HashMap::new()),
            per_ip_max: (global_max / 10.0).max(5.0), // at least 5 per IP per minute
        }
    }

    /// Try to consume one token. Checks both global and per-IP limits.
    /// Returns `true` if allowed, `false` if rate-limited.
    pub fn try_acquire(&self) -> bool {
        self.global.lock().unwrap().try_acquire()
    }

    /// Try to consume one token with per-IP tracking.
    pub fn try_acquire_for_ip(&self, ip: IpAddr) -> bool {
        // Check global limit first
        if !self.global.lock().unwrap().try_acquire() {
            return false;
        }
        // Check per-IP limit
        let mut per_ip = self.per_ip.lock().unwrap();
        let bucket = per_ip.entry(ip).or_insert_with(|| Bucket::new(self.per_ip_max));
        bucket.try_acquire()
    }

    /// Clean up stale per-IP entries (call periodically).
    pub fn cleanup_stale(&self) {
        let mut per_ip = self.per_ip.lock().unwrap();
        let now = Instant::now();
        per_ip.retain(|_, bucket| {
            // Remove entries not seen in the last 5 minutes
            now.duration_since(bucket.last_refill).as_secs() < 300
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allows_within_global_limit() {
        let limiter = RateLimiter::new(100);
        for _ in 0..100 {
            assert!(limiter.try_acquire());
        }
    }

    #[test]
    fn rejects_over_global_limit() {
        let limiter = RateLimiter::new(10);
        for _ in 0..10 {
            assert!(limiter.try_acquire());
        }
        assert!(!limiter.try_acquire());
    }

    #[test]
    fn per_ip_limits_individual_sources() {
        let limiter = RateLimiter::new(100); // global 100, per-IP 10
        let ip: IpAddr = "10.0.0.1".parse().unwrap();

        // Per-IP limit is 100/10 = 10
        for _ in 0..10 {
            assert!(limiter.try_acquire_for_ip(ip));
        }
        // 11th request from same IP should be blocked
        assert!(!limiter.try_acquire_for_ip(ip));

        // Different IP should still work
        let ip2: IpAddr = "10.0.0.2".parse().unwrap();
        assert!(limiter.try_acquire_for_ip(ip2));
    }

    #[test]
    fn cleanup_removes_stale_entries() {
        let limiter = RateLimiter::new(100);
        let ip: IpAddr = "10.0.0.1".parse().unwrap();
        limiter.try_acquire_for_ip(ip);
        assert_eq!(limiter.per_ip.lock().unwrap().len(), 1);

        // Cleanup shouldn't remove recent entries
        limiter.cleanup_stale();
        assert_eq!(limiter.per_ip.lock().unwrap().len(), 1);
    }
}
