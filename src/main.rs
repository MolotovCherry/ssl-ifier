mod config;
mod error_pages;
mod resolver;
mod websocket;

use std::{
    convert::Infallible,
    env,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};

use anyhow::anyhow;
use axum::{
    body::{Body, BoxBody, HttpBody},
    extract::Host,
    handler::HandlerWithoutStateExt,
    http::{uri, Request, Uri},
    response::{IntoResponse, Redirect, Response},
    routing::{any_service, get},
    Extension, Router,
};
use axum_server::tls_rustls::RustlsConfig;
use reqwest::{Client, StatusCode};
use tokio::{task, time::sleep};
use tower::ServiceBuilder;
use tower_http::add_extension::AddExtensionLayer;
use url::Url;

use crate::config::Config;

use self::error_pages::error_page;

#[derive(Debug)]
pub struct StateData {
    client: Client,
    config: Config,
    health: AtomicBool,
    websocket_destination: Option<Url>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = config::Config::get_config()?;
    let req_data = Arc::new(StateData {
        client: Client::builder().build().unwrap(),
        websocket_destination: if let Some(path) = &config.addresses.websocket_path {
            Some(Url::parse(&format!("ws://{}{path}", config.addresses.backend)).unwrap())
        } else {
            None
        },
        config,
        health: AtomicBool::new(true),
    });

    // make backend address
    let backend_addr = resolver::get_addresses(&req_data.config.addresses.proxy)?;

    let backend_addr = backend_addr.ipv4.ok_or(anyhow!("ipv4 address not found"))?;

    // get server config for rust
    let exe_path = env::current_exe()?;
    let exe_path = exe_path.parent().ok_or(anyhow!("Failed to get parent"))?;

    let ssl_config = RustlsConfig::from_pem_file(
        exe_path.join(&req_data.config.addresses.ssl_cert),
        exe_path.join(&req_data.config.addresses.ssl_key),
    )
    .await?;
    //

    if req_data.config.addresses.proxy_http.is_none() {
        println!(
            "Listening on https://{} for service http://{}",
            req_data.config.addresses.proxy, req_data.config.addresses.backend
        );
    } else if let Some(proxy_http) = &req_data.config.addresses.proxy_http {
        println!(
            "Listening on http://{proxy_http} and https://{} for service http://{}",
            req_data.config.addresses.proxy, req_data.config.addresses.backend
        );
    }

    // whether to serve http endpoint which redirects to https
    if req_data.config.addresses.proxy_http.is_some() {
        task::spawn(redirect_http_to_https(req_data.clone()));
    }

    let check_health = req_data.config.addresses.health_check.is_some();

    // run health checks against api to determine availability of service
    if check_health {
        let data = req_data.clone();
        task::spawn(async move {
            loop {
                health_check(&data).await;
                sleep(Duration::from_secs(5)).await;
            }
        });
    }

    let service = ServiceBuilder::new()
        .layer(AddExtensionLayer::new(req_data.clone()))
        .service(tower::service_fn(backend_ssl_proxy));
    let mut router = Router::new();

    if let Some(path) = &req_data.config.addresses.websocket_path {
        println!(
            "Listening for websocket connections on wss://{}{path}",
            req_data.config.addresses.proxy
        );
        router = router.route(path, get(websocket::handler));
    }

    // you cannot have two routes with the same path or panic, so we will let websocket override it
    if !req_data
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
        .layer(Extension(req_data.clone()));

    axum_server::bind_rustls(backend_addr, ssl_config)
        .serve(router.into_make_service())
        .await?;

    Ok(())
}

async fn redirect_http_to_https(data: Arc<StateData>) -> anyhow::Result<()> {
    let http_port = resolver::get_port(data.config.addresses.proxy_http.as_ref().unwrap())
        .unwrap_or("80")
        .to_string();
    let https_port = resolver::get_port(&data.config.addresses.proxy)
        .unwrap_or("443")
        .to_string();

    let make_https = move |host: String, uri: Uri| -> anyhow::Result<Uri> {
        let mut parts = uri.into_parts();

        parts.scheme = Some(uri::Scheme::HTTPS);

        if parts.path_and_query.is_none() {
            parts.path_and_query = Some("/".parse().unwrap());
        }

        let https_host = host.replace(&http_port, &https_port);
        parts.authority = Some(https_host.parse()?);

        Ok(Uri::from_parts(parts)?)
    };

    let redirect = move |Host(host): Host, uri: Uri| async move {
        match make_https(host, uri) {
            Ok(uri) => Redirect::permanent(&uri.to_string()).into_response(),
            Err(error) => error_page(StatusCode::BAD_REQUEST, error).into_response(),
        }
    };

    let http_proxy = resolver::get_addresses(data.config.addresses.proxy_http.as_ref().unwrap())?;
    let http_proxy = http_proxy.ipv4.ok_or(anyhow!("ipv4 address not found"))?;

    axum_server::bind(http_proxy)
        .serve(redirect.into_make_service())
        .await?;

    Ok(())
}

async fn health_check(data: &StateData) {
    let url = format!(
        "http://{}{}",
        data.config.addresses.backend,
        data.config.addresses.health_check.as_ref().unwrap()
    );

    match data.client.get(url).send().await {
        Ok(_) => data.health.store(true, Ordering::Relaxed),
        Err(_) => data.health.store(false, Ordering::Relaxed),
    }
}

async fn backend_ssl_proxy(req: Request<Body>) -> Result<Response<BoxBody>, Infallible> {
    let data = req.extensions().get::<Arc<StateData>>().unwrap().clone();

    let health = data.health.load(Ordering::Relaxed);
    if !health {
        return Ok(error_page(
            StatusCode::BAD_GATEWAY,
            format!(
                "Health check failed for {}, service is down",
                data.config.addresses.backend
            ),
        ));
    }

    let (parts, body) = req.into_parts();

    let path = parts
        .uri
        .path_and_query()
        .map(|i| i.as_str())
        .unwrap_or("/");
    let method = parts.method;
    let headers = parts.headers;

    let url = format!("http://{}{path}", data.config.addresses.backend);

    let reqwest = match data
        .client
        .request(method, url)
        .headers(headers)
        .body(body)
        .send()
        .await
    {
        Ok(res) => res,
        Err(e) => {
            return Ok(error_page(StatusCode::BAD_GATEWAY, e));
        }
    };

    let mut response = Response::builder();

    *response.headers_mut().unwrap() = reqwest.headers().clone();

    if data.config.options.http_support {
        response = response.header("Content-Security-Policy", "upgrade-insecure-requests");
    }

    let response = response
        .status(reqwest.status())
        .body(Body::wrap_stream(reqwest.bytes_stream()))
        .unwrap()
        .map(|b| BoxBody::new(b.map_err(axum::Error::new)));

    Ok(response)
}
