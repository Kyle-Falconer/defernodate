use std::time::Duration;

/// Configuration for the hybrid cache engine.
#[derive(Debug, Clone)]
pub struct CacheConfig {
    /// How far back from "now" to expand when filling cache on a miss.
    pub lookbehind: chrono::Duration,
    /// How far forward from "now" to expand when filling cache on a miss.
    pub lookahead: chrono::Duration,
    /// Optional TTL for cached instance keys.
    pub instance_ttl: Option<Duration>,
    /// Redis key prefix for namespacing in shared Redis instances.
    pub key_prefix: Option<String>,
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            lookbehind: chrono::Duration::days(30),
            lookahead: chrono::Duration::days(180),
            instance_ttl: None,
            key_prefix: None,
        }
    }
}
