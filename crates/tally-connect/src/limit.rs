//! In-memory fixed-window rate limiting.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Entries kept before expired windows are swept out, so a flood of distinct
/// keys can't grow memory without bound.
const PRUNE_ABOVE: usize = 10_000;

pub struct Limiter {
    limit: u32,
    window: Duration,
    counts: Mutex<HashMap<String, (Instant, u32)>>,
}

impl Limiter {
    pub fn new(limit: u32, window: Duration) -> Limiter {
        Limiter { limit, window, counts: Mutex::new(HashMap::new()) }
    }

    /// Counts one event for `key`; false once the window's limit is passed.
    pub fn hit(&self, key: &str) -> bool {
        let mut map = self.counts.lock().unwrap_or_else(|e| e.into_inner());
        let now = Instant::now();
        if map.len() > PRUNE_ABOVE {
            map.retain(|_, (start, _)| now.duration_since(*start) < self.window);
        }
        let entry = map.entry(key.to_string()).or_insert((now, 0));
        if now.duration_since(entry.0) >= self.window {
            *entry = (now, 0);
        }
        entry.1 = entry.1.saturating_add(1);
        entry.1 <= self.limit
    }

    /// Whether `key` has already used its window's allowance (counts nothing).
    pub fn blocked(&self, key: &str) -> bool {
        let map = self.counts.lock().unwrap_or_else(|e| e.into_inner());
        map.get(key).is_some_and(|(start, n)| start.elapsed() < self.window && *n >= self.limit)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allows_up_to_the_limit_per_key() {
        let l = Limiter::new(2, Duration::from_secs(60));
        assert!(l.hit("a") && l.hit("a"));
        assert!(!l.hit("a"));
        assert!(l.hit("b"));
        assert!(l.blocked("a") && !l.blocked("c"));
    }

    #[test]
    fn window_resets() {
        let l = Limiter::new(1, Duration::from_millis(20));
        assert!(l.hit("a"));
        assert!(!l.hit("a"));
        std::thread::sleep(Duration::from_millis(30));
        assert!(l.hit("a"));
    }
}
