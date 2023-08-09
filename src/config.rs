use std::{env, fs};

use anyhow::anyhow;
use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Config {
    pub addresses: Addresses,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Addresses {
    // Backend host. In the following format
    //- eg: 127.0.0.1:8081, myaddr.com:8081
    pub backend: String,
    // Proxy address to listen on (and port)
    //- eg: 127.0.0.1:443, myaddr.com:443
    pub proxy: String,
    // must be PEM format
    pub ssl_cert: String,
    // must be PEM format
    pub ssl_key: String,
}

impl Config {
    pub fn get_config() -> anyhow::Result<Self> {
        let exe_path = env::current_exe()?;
        let parent_dir = exe_path
            .parent()
            .ok_or_else(|| anyhow!("Failed to find parent dir"))?;

        let config_path = parent_dir.join("config.toml");

        if !config_path.exists() {
            fs::write(config_path, toml::to_string(&Self::default())?)?;
            return Err(anyhow!("Please setup config.toml"));
        }

        let config = fs::read_to_string(config_path)?;

        Ok(toml::from_str::<Self>(&config)?)
    }
}
