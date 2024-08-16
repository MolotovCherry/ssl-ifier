use std::sync::Arc;

use axum::{
    extract::Host,
    handler::HandlerWithoutStateExt as _,
    http::{
        uri::{self, Authority, InvalidUri, InvalidUriParts},
        Uri,
    },
    response::{IntoResponse as _, Redirect},
};
use color_eyre::Result;
use reqwest::{Method, StatusCode};
use tracing::info;

use crate::{error_pages::error_page, resolver, utils::format_req, StateData};

#[derive(Debug, thiserror::Error)]
pub enum RedirectError {
    #[error("ipv4 address not found")]
    Ipv4NotFound,
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    InvalidUriParts(#[from] InvalidUriParts),
    #[error("{0}")]
    InvalidUri(#[from] InvalidUri),
    #[error("address proxy_http config section needs to be configured")]
    MissingProxyHttp,
}

pub async fn redirect_http(data: Arc<StateData>) -> Result<()> {
    let http_port = resolver::get_port(data.config.addresses.proxy_http.as_ref().unwrap())
        .unwrap_or("80")
        .to_string();
    let https_port = resolver::get_port(&data.config.addresses.proxy)
        .unwrap_or("443")
        .to_string();

    let make_https = move |host: String, uri: Uri| -> Result<Uri> {
        let mut parts = uri.into_parts();

        parts.scheme = Some(uri::Scheme::HTTPS);

        if parts.path_and_query.is_none() {
            parts.path_and_query = Some("/".parse().unwrap());
        }

        let https_host = host.replace(&http_port, &https_port);
        let authority: std::result::Result<Authority, InvalidUri> = https_host.parse::<Authority>();
        let authority = authority?;
        parts.authority = Some(authority);

        Ok(Uri::from_parts(parts)?)
    };

    let redirect = move |method: Method, Host(host): Host, uri: Uri| async move {
        info!("{}", format_req(&method, &uri));

        match make_https(host, uri) {
            Ok(uri) => Redirect::permanent(&uri.to_string()).into_response(),
            Err(error) => error_page(StatusCode::BAD_REQUEST, error).into_response(),
        }
    };

    let http_proxy = resolver::get_addresses(
        data.config
            .addresses
            .proxy_http
            .as_ref()
            .ok_or(RedirectError::MissingProxyHttp)?,
    )?;

    let http_proxy = http_proxy.ipv4.ok_or(RedirectError::Ipv4NotFound)?;

    axum_server::bind(http_proxy)
        .serve(redirect.into_make_service())
        .await?;

    Ok(())
}
