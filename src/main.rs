mod config;
mod error_pages;
mod health;
mod proxy;
mod redirect;
mod resolver;
mod websocket;

use std::{
    env,
    sync::{atomic::AtomicBool, Arc},
};

use axum::{
    routing::{any_service, get},
    Extension, Router,
};
use axum_server::tls_rustls::RustlsConfig;
use color_eyre::{eyre::bail, Result};
use reqwest::Client;
use thiserror::Error;
use tokio::task;
use tower::ServiceBuilder;
use tower_http::add_extension::AddExtensionLayer;
use tracing::{error, info, level_filters::LevelFilter};
use tracing_subscriber::{
    fmt, prelude::__tracing_subscriber_SubscriberExt as _, util::SubscriberInitExt as _, EnvFilter,
};
use url::Url;

use self::{health::health_check, redirect::redirect_http};
use crate::config::Config;

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
    #[error("ipv4 address not found")]
    Ipv4NotFound,
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("failed to get parent")]
    NoParent,
    #[error("ssl cert or key was not configured")]
    SslMissing,
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
        .with(fmt::layer().without_time())
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

    // make backend address
    let backend_addr = resolver::get_addresses(&data.config.addresses.proxy)?;

    let backend_addr = backend_addr.ipv4.ok_or(AppError::Ipv4NotFound)?;

    // get server config for rust
    let exe_path = env::current_exe().map_err(|_| AppError::NoCurrentExe)?;
    let exe_path = exe_path.parent().ok_or(AppError::NoParent)?;

    let ssl_config = if data.config.options.ssl {
        let (Some(cert), Some(key)) = (
            &data.config.addresses.ssl_cert,
            &data.config.addresses.ssl_key,
        ) else {
            bail!(AppError::SslMissing);
        };

        let ssl = RustlsConfig::from_pem_file(exe_path.join(cert), exe_path.join(key)).await?;

        Some(ssl)
    } else {
        None
    };

    //

    if data.config.addresses.proxy_http.is_none() {
        info!(
            "Listening on https://{} for service http://{}",
            data.config.addresses.proxy, data.config.addresses.backend
        );
    } else if let Some(proxy_http) = &data.config.addresses.proxy_http {
        info!(
            "Listening on http://{proxy_http} and https://{} for service http://{}",
            data.config.addresses.proxy, data.config.addresses.backend
        );
    }

    // run health checks against api to determine availability of service
    if data.config.addresses.health_check.is_some() {
        health_check(data.clone());
    }

    let service = ServiceBuilder::new()
        .layer(AddExtensionLayer::new(data.clone()))
        .service(tower::service_fn(proxy::proxy));
    let mut router = Router::new();

    if let Some(path) = &data.config.addresses.websocket_path {
        info!(
            "Listening for websocket connections on wss://{}{path}",
            data.config.addresses.proxy
        );

        router = router.route(path, get(websocket::handler));
    }

    // you cannot have two routes with the same path or panic, so we will let websocket override it
    if !data
        .config
        .addresses
        .websocket_path
        .as_ref()
        .is_some_and(|p| p == "/")
    {
        router = router.route("/", any_service(service.clone()));
    }

    // everything else goes to the service
    router = router
        .route("/*path", any_service(service))
        .layer(Extension(data.clone()));

    if let Some(ssl_config) = ssl_config {
        // whether to serve http endpoint which redirects to https
        if data.config.addresses.proxy_http.is_some() {
            let data = data.clone();
            task::spawn(async move {
                if let Err(e) = redirect_http(data).await {
                    error!("{e}");
                }
            });
        }

        // ssl
        axum_server::bind_rustls(backend_addr, ssl_config)
            .serve(router.into_make_service())
            .await?;
    } else {
        // http
        axum_server::bind(backend_addr)
            .serve(router.into_make_service())
            .await?;
    }

    Ok(())
}
