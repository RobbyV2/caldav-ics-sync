use anyhow::{Result, bail};
use serde::Deserialize;

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
}

impl AppConfig {
    pub fn load() -> Result<Self> {
        let cfg = config::Config::builder()
            .set_default("server_host", "0.0.0.0")?
            .set_default("server_port", 6765_i64)?
            .set_default("port", 6766_i64)?
            .set_default("data_dir", "./data")?
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

    pub fn proxy_url(&self) -> String {
        match &self.server_proxy_url {
            Some(url) => url.clone(),
            None => format!("http://127.0.0.1:{}", self.port),
        }
    }
}
