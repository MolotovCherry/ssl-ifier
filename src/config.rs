use std::{
    env, fs,
    net::{Ipv4Addr, SocketAddr, SocketAddrV4},
};

use color_eyre::{eyre::eyre, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Config {
    pub addresses: Addresses,
    pub options: Options,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Addresses {
    pub host: String,
    // Backend host. In the following format
    //- eg: 127.0.0.1:8081, myaddr.com:8081
    pub backend: String,
    // Proxy address to listen on (and ports)
    // This DOES NOT serve content over http (use your regular service for that if you want that)
    // The purpose of this is to provide a permanent redirect to the https service
    //- eg: 127.0.0.1:80:443, myaddr.com:80:443
    pub proxy: String,
    // Whether to enable websocket proxying to backend, and if so, what path to use
    //- eg: /ws
    pub websocket_path: Option<String>,
    // must be PEM format
    pub ssl_cert: String,
    // must be PEM format
    pub ssl_key: String,
    // path to the health check on the backend
    // e.g. /api/health
    pub health_check: Option<String>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Options {
    // enable kavita support
    #[serde(default)]
    pub kavita: bool,
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

    pub fn proxy_addr(&self) -> Result<ProxyAddr> {
        let mut iter = self.addresses.proxy.split(":");

        let addr = iter.next().unwrap_or("0.0.0.0").parse::<Ipv4Addr>()?;
        let http_port = iter.next().unwrap_or("80").parse::<u16>()?;
        let ssl_port = iter.next().unwrap_or("443").parse::<u16>()?;

        let addr = ProxyAddr {
            addr,
            ssl_port,
            http_port,
        };

        Ok(addr)
    }
}

#[derive(Copy, Clone, Debug)]
pub struct ProxyAddr {
    pub addr: Ipv4Addr,
    pub ssl_port: u16,
    pub http_port: u16,
}

impl ProxyAddr {
    pub fn ssl_addr(&self) -> SocketAddr {
        let v4 = SocketAddrV4::new(self.addr, self.ssl_port);
        SocketAddr::V4(v4)
    }

    pub fn http_addr(&self) -> SocketAddr {
        let v4 = SocketAddrV4::new(self.addr, self.http_port);
        SocketAddr::V4(v4)
    }
}
