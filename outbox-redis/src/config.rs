use std::time::Duration;

pub struct RedisTokenConfig {
    pub ttl: Duration,
    pub key_prefix: String,
    #[cfg(feature = "moka")]
    pub local_cache_capacity: u64,
}

impl Default for RedisTokenConfig {
    fn default() -> Self {
        Self {
            ttl: Duration::from_secs(24 * 3600),
            key_prefix: "i_token".to_owned(),
            #[cfg(feature = "moka")]
            local_cache_capacity: 10_000,
        }
    }
}
