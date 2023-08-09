mod config;
mod error_pages;
mod resolver;

use std::{
    env,
    sync::{Arc, OnceLock},
};

use anyhow::anyhow;
use axum::{
    body::{Body, BoxBody, HttpBody},
    http::{header, Request},
    response::{IntoResponse, Response},
};
use axum_server::tls_rustls::RustlsConfig;
use reqwest::{Client, StatusCode};
use tower::make::Shared;

use crate::config::Config;

use self::error_pages::{format_error_page, E502};

static REQUEST_DATA: OnceLock<RequestData> = OnceLock::new();

#[derive(Debug)]
struct RequestData {
    client: Client,
    config: Arc<Config>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = Arc::new(config::Config::get_config()?);

    println!(
        "Starting ssl-ifier for service: {}",
        config.addresses.backend
    );
    println!("Listening on: {}", config.addresses.proxy);

    REQUEST_DATA
        .set(RequestData {
            client: Client::builder().build().unwrap(),
            config: config.clone(),
        })
        .unwrap();

    // make backend address
    let backend_addr = resolver::get_addresses(&config.addresses.proxy)?;

    let backend_addr = backend_addr.ipv4.ok_or(anyhow!("ipv4 address not found"))?;

    // get server config for rust
    let exe_path = env::current_exe()?;
    let exe_path = exe_path.parent().ok_or(anyhow!("Failed to get parent"))?;

    let config = RustlsConfig::from_pem_file(
        exe_path.join(&config.addresses.ssl_cert),
        exe_path.join(&config.addresses.ssl_key),
    )
    .await?;
    //

    let service = tower::service_fn(backend_ssl_proxy);

    axum_server::bind_rustls(backend_addr, config)
        .serve(Shared::new(service))
        .await?;

    Ok(())
}

async fn backend_ssl_proxy(req: Request<Body>) -> anyhow::Result<Response<BoxBody>> {
    let data = REQUEST_DATA.get().unwrap();

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
            return Ok((
                StatusCode::BAD_GATEWAY,
                [(header::CONTENT_TYPE, "text/html")],
                format_error_page(E502, e),
            )
                .into_response());
        }
    };

    let mut response = Response::builder();

    *response.headers_mut().unwrap() = reqwest.headers().clone();

    let response = response
        .status(reqwest.status())
        .body(Body::wrap_stream(reqwest.bytes_stream()))
        .unwrap()
        .map(|b| BoxBody::new(b.map_err(axum::Error::new)));

    Ok(response)
}
