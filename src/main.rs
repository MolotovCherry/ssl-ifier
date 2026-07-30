mod config;
mod error_pages;
mod health;
mod middleware;
mod proxy;
mod redirect;
mod utils;
mod websocket;

use std::{
    env,
    net::SocketAddr,
    sync::{atomic::AtomicBool, Arc},
};

use axum::{middleware as amiddleware, routing::get, Router};
use axum_server::tls_rustls::RustlsConfig;
use color_eyre::Result;
use reqwest::Client;
use thiserror::Error;
use tokio::task;
use tracing::{error, info, level_filters::LevelFilter};
use tracing_subscriber::{fmt, prelude::*, EnvFilter};
use url::Url;

use self::{health::health_check, redirect::redirect_http};
use crate::config::{Config, ProxyAddr};

#[derive(Debug)]
pub struct StateData {
    client: Client,
    config: Config,
    health: AtomicBool,
    websocket_destination: Option<Url>,
}

#[derive(Error, Debug)]
enum AppError {
    #[error("failed to install crypto handler")]
    CryptoInstallFailure,
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("failed to get parent")]
    NoParent,
    #[error("could not parse: {0}")]
    ParseFailure(String),
    #[error("no current exe found")]
    NoCurrentExe,
}

fn setup() -> Result<()> {
    rustls::crypto::aws_lc_rs::default_provider()
        .install_default()
        .map_err(|_| AppError::CryptoInstallFailure)?;

    tracing_subscriber::registry()
        .with(fmt::layer().without_time().with_ansi_sanitization(false))
        .with(
            EnvFilter::builder()
                .with_default_directive(LevelFilter::INFO.into())
                .with_env_var("PROXY_LOG")
                .from_env_lossy(),
        )
        .init();

    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    setup()?;

    let config = config::Config::get_config()?;
    let data = Arc::new(StateData {
        client: Client::builder().build().unwrap(),
        websocket_destination: if let Some(path) = &config.addresses.websocket_path {
            let addr = format!("ws://{}{path}", config.addresses.backend);
            Some(Url::parse(&addr).map_err(|_| AppError::ParseFailure(addr))?)
        } else {
            None
        },
        config,
        health: AtomicBool::new(true),
    });

    let proxy_addr = data.config.proxy_addr()?;

    // get server config for rust
    let exe_path = env::current_exe().map_err(|_| AppError::NoCurrentExe)?;
    let exe_path = exe_path.parent().ok_or(AppError::NoParent)?;

    let ssl_config = RustlsConfig::from_pem_file(
        exe_path.join(&data.config.addresses.ssl_cert),
        exe_path.join(&data.config.addresses.ssl_key),
    )
    .await?;

    //

    info!(
        "Listening on http://{proxy_addr}:{proxy_port} and https://{proxy_addr}:{ssl_port} for service http://{backend}",
        backend = data.config.addresses.backend,
        proxy_addr = proxy_addr.addr,
        proxy_port = proxy_addr.http_port,
        ssl_port = proxy_addr.ssl_port
    );

    // run health checks against api to determine availability of service
    if data.config.addresses.health_check.is_some() {
        health_check(data.clone());
    }

    let router = make_route(proxy_addr, data.clone());

    // serve http endpoint which redirects to https
    task::spawn(async move {
        if let Err(e) = redirect_http(proxy_addr).await {
            error!("{e}");
        }
    });

    // ssl
    axum_server::bind_rustls(proxy_addr.ssl_addr(), ssl_config)
        .serve(router.into_make_service_with_connect_info::<SocketAddr>())
        .await?;

    Ok(())
}

fn make_route(addr: ProxyAddr, data: Arc<StateData>) -> Router {
    let mut router = Router::new().fallback(proxy::proxy);

    if let Some(path) = &data.config.addresses.websocket_path {
        info!(
            "Listening for websocket connections on wss://{proxy_addr}:{ssl_port}{path}",
            proxy_addr = addr.addr,
            ssl_port = addr.ssl_port
        );

        router = router.route(path, get(websocket::handler));
    }

    if data.config.options.kavita {
        router = router.layer(amiddleware::from_fn_with_state(
            data.clone(),
            middleware::kavita,
        ));
    }

    router.with_state(data)
}
