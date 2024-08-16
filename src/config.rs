use std::{env, fs};

use color_eyre::{eyre::eyre, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Config {
    pub addresses: Addresses,
    pub options: Options,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Addresses {
    // Backend host. In the following format
    //- eg: 127.0.0.1:8081, myaddr.com:8081
    pub backend: String,
    // Proxy address to listen on (and port)
    //- eg: 127.0.0.1:443, myaddr.com:443
    pub proxy: String,
    // Proxy address to listen on (and port) for http
    // This DOES NOT serve content over http (use your regular service for that if you want that)
    // The purpose of this is to provide a permanent redirect to the https service
    //- eg: 127.0.0.1:80, myaddr.com:80
    pub proxy_http: Option<String>,
    // Whether to enable websocket proxying to backend, and if so, what path to use
    //- eg: /ws
    pub websocket_path: Option<String>,
    // must be PEM format
    pub ssl_cert: Option<String>,
    // must be PEM format
    pub ssl_key: Option<String>,
    // path to the health check on the backend
    // e.g. /api/health
    pub health_check: Option<String>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Options {
    // If your service has legacy http urls, you can turn on this to allow compatibility
    // Sets the header: `Content-Security-Policy: upgrade-insecure-requests`
    pub http_support: bool,
    // Whether ssl is enabled
    pub ssl: bool,
}

impl Config {
    pub fn get_config() -> Result<Self> {
        let exe_path = env::current_exe()?;
        let parent_dir = exe_path
            .parent()
            .ok_or_else(|| eyre!("Failed to find parent dir"))?;

        let config_path = parent_dir.join("config.toml");

        if !config_path.exists() {
            fs::write(config_path, toml::to_string(&Self::default())?)?;
            return Err(eyre!("Please setup config.toml"));
        }

        let config = fs::read_to_string(config_path)?;

        Ok(toml::from_str::<Self>(&config)?)
    }
}
