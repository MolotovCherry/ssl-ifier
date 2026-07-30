use std::{
    env, fs, io,
    net::{Ipv4Addr, SocketAddr, SocketAddrV4},
};

use serde::{Deserialize, Serialize};
use snafu::{OptionExt, ResultExt, Snafu, whatever};

#[derive(Debug, Snafu)]
pub enum ConfigError {
    #[snafu(display("Exe path not found"))]
    ExePathNotFound { source: io::Error },
    #[snafu(display("Exe path's parent dir not found"))]
    ParentDirNotFound,
    #[snafu(display("toml serialize error: {source}"))]
    TomlSer { source: toml::ser::Error },
    #[snafu(display("toml deserialize error: {source}"))]
    TomlDe { source: toml::de::Error },
    #[snafu(display("io error: {source}"))]
    Io { source: io::Error },

    #[snafu(whatever, display("{message}"))]
    Whatever {
        message: String,
        #[snafu(source(from(Box<dyn std::error::Error>, Some)))]
        source: Option<Box<dyn std::error::Error>>,
    },
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Config {
    pub addresses: Addresses,
    pub options: Options,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
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
}

#[derive(Debug, Copy, Clone, Default, Serialize, Deserialize)]
pub struct Options {
    // enable kavita support
    #[serde(default)]
    pub kavita: bool,
}

impl Config {
    pub fn get_config() -> Result<Self, ConfigError> {
        let exe_path = env::current_exe().context(ExePathNotFoundSnafu)?;
        let parent_dir = exe_path.parent().context(ParentDirNotFoundSnafu)?;

        let config_path = parent_dir.join("config.toml");

        if !config_path.exists() {
            fs::write(
                config_path,
                toml::to_string(&Self::default()).context(TomlSerSnafu)?,
            )
            .context(IoSnafu)?;

            whatever!("Please setup config.toml");
        }

        let config = fs::read_to_string(config_path).whatever_context("")?;

        toml::from_str::<Self>(&config).context(TomlDeSnafu)
    }

    pub fn proxy_addr(&self) -> Result<ProxyAddr, ConfigError> {
        let mut iter = self.addresses.proxy.split(":");

        let addr = iter
            .next()
            .unwrap_or("0.0.0.0")
            .parse::<Ipv4Addr>()
            .whatever_context("failed to pase proxy addr")?;

        let http_port = iter
            .next()
            .unwrap_or("80")
            .parse::<u16>()
            .whatever_context("failed to pase proxy http port")?;

        let ssl_port = iter
            .next()
            .unwrap_or("443")
            .parse::<u16>()
            .whatever_context("failed to pase proxy ssl port")?;

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
