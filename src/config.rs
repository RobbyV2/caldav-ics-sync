use anyhow::{Result, bail};
use serde::Deserialize;

/// Request rate limiting for the `/api` and `/ics` routes.
///
/// Opt-in: only active when `RATE_LIMIT_PER_SECOND` is set.
#[derive(Debug, Clone, Copy)]
pub struct RateLimit {
    pub per_second: u64,
    pub burst: u32,
    /// Key clients by the `X-Forwarded-For`/`X-Real-IP` header instead of the peer
    /// address. Required when running behind a reverse proxy, where every request
    /// otherwise shares the proxy's IP and therefore a single bucket.
    ///
    /// Only enable when a trusted proxy sits in front: clients can forge these headers.
    pub trust_proxy: bool,
}

#[derive(Debug, Deserialize)]
pub struct AppConfig {
    pub server_host: String,
    pub server_port: u16,
    pub port: u16,
    pub server_proxy_url: Option<String>,
    pub data_dir: String,
    pub db_path: Option<String>,
    pub auth_username: Option<String>,
    pub auth_password: Option<String>,
    pub auth_password_hash: Option<String>,
    pub rate_limit_per_second: Option<u64>,
    pub rate_limit_burst: u32,
    pub rate_limit_trust_proxy: bool,
}

impl AppConfig {
    pub fn load() -> Result<Self> {
        let cfg = config::Config::builder()
            .set_default("server_host", "0.0.0.0")?
            .set_default("server_port", 6765_i64)?
            .set_default("port", 6766_i64)?
            .set_default("data_dir", "./data")?
            .set_default("rate_limit_burst", 60_i64)?
            .set_default("rate_limit_trust_proxy", false)?
            .add_source(config::Environment::default())
            .build()?
            .try_deserialize::<Self>()?;

        if cfg.auth_password.is_some() && cfg.auth_password_hash.is_some() {
            bail!("AUTH_PASSWORD and AUTH_PASSWORD_HASH are mutually exclusive; set only one");
        }

        Ok(cfg)
    }

    pub fn db_path(&self) -> String {
        match &self.db_path {
            Some(path) => path.clone(),
            None => format!("{}/caldav-sync.db", self.data_dir),
        }
    }

    pub fn rate_limit(&self) -> Option<RateLimit> {
        self.rate_limit_per_second.map(|per_second| RateLimit {
            per_second,
            burst: self.rate_limit_burst,
            trust_proxy: self.rate_limit_trust_proxy,
        })
    }

    pub fn proxy_url(&self) -> String {
        match &self.server_proxy_url {
            Some(url) => url.clone(),
            None => format!("http://127.0.0.1:{}", self.port),
        }
    }
}
