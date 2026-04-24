use std::time::Duration;

/// Configuration for a cache-backed caller. Contains pure values only
/// — no Redis prefixes, no I/O settings. Defaults chosen for a typical
/// personal calendar (30 days back, 180 days forward).
#[derive(Debug, Clone)]
pub struct CacheConfig {
    /// How far back from "now" to expand when filling cache on a miss.
    pub lookbehind: chrono::Duration,
    /// How far forward from "now" to expand when filling cache on a miss.
    pub lookahead: chrono::Duration,
    /// Optional TTL for cached instance keys.
    pub instance_ttl: Option<Duration>,
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            lookbehind: chrono::Duration::days(30),
            lookahead: chrono::Duration::days(180),
            instance_ttl: None,
        }
    }
}
