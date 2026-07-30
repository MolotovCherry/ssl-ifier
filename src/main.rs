mod config;
mod error_pages;
mod middleware;
mod proxy;
mod redirect;
mod utils;
mod websocket;

use std::{env, net::SocketAddr, sync::Arc};

use axum::{Router, body::Body, middleware as amiddleware, routing::get};
use axum_server::tls_rustls::RustlsConfig;
use hyper_util::{
    client::legacy::{Client, connect::HttpConnector},
    rt::TokioExecutor,
};
use snafu::{OptionExt, ResultExt, Snafu};
use tokio::task;
use tracing::{error, info, level_filters::LevelFilter};
use tracing_subscriber::{EnvFilter, fmt, prelude::*};
use url::{ParseError, Url};

use crate::config::{Config, ConfigError, ProxyAddr};
use redirect::redirect_http;

#[derive(Debug)]
pub struct StateData {
    client: Client<HttpConnector, Body>,
    config: Config,
    websocket_destination: Option<Url>,
}

#[derive(Snafu, Debug)]
enum AppError {
    #[snafu(display("failed to install crypto handler"))]
    CryptoInstallFailure,
    #[snafu(display("{source}"))]
    Io { source: std::io::Error },
    #[snafu(display("failed to get parent"))]
    NoParent,
    #[snafu(display("could not parse: {source}"))]
    ParseFailure { source: ParseError },
    #[snafu(display("no current exe found"))]
    NoCurrentExe,
    #[snafu(display("{source}"))]
    Config { source: ConfigError },

    #[snafu(whatever, display("{message}"))]
    Whatever {
        message: String,
        #[snafu(source(from(Box<dyn std::error::Error>, Some)))]
        source: Option<Box<dyn std::error::Error>>,
    },
}

fn setup() -> Result<(), AppError> {
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
async fn main() -> Result<(), AppError> {
    setup()?;

    let client = Client::builder(TokioExecutor::new()).build_http();

    let config = config::Config::get_config().context(ConfigSnafu)?;
    let data = Arc::new(StateData {
        client,
        websocket_destination: if let Some(path) = &config.addresses.websocket_path {
            let addr = format!("ws://{}{path}", config.addresses.backend);
            Some(Url::parse(&addr).context(ParseFailureSnafu)?)
        } else {
            None
        },
        config,
    });

    let proxy_addr = data.config.proxy_addr().context(ConfigSnafu)?;

    // get server config for rust
    let exe_path = env::current_exe().map_err(|_| AppError::NoCurrentExe)?;
    let exe_path = exe_path.parent().context(NoParentSnafu)?;

    let ssl_config = RustlsConfig::from_pem_file(
        exe_path.join(&data.config.addresses.ssl_cert),
        exe_path.join(&data.config.addresses.ssl_key),
    )
    .await
    .context(IoSnafu)?;

    //

    info!(
        "Listening on http://{proxy_addr}:{proxy_port} and https://{proxy_addr}:{ssl_port} for service http://{backend}",
        backend = data.config.addresses.backend,
        proxy_addr = proxy_addr.addr,
        proxy_port = proxy_addr.http_port,
        ssl_port = proxy_addr.ssl_port
    );

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
        .await
        .context(IoSnafu)?;

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
