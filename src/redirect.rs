use axum::{
    handler::HandlerWithoutStateExt as _,
    http::{
        uri::{self, Authority, InvalidUri},
        Uri,
    },
    response::{IntoResponse as _, Redirect},
};
use axum_extra::{headers::Host, TypedHeader};
use color_eyre::Result;
use reqwest::{Method, StatusCode};
use tracing::info;

use crate::{config::ProxyAddr, error_pages::error_page, utils::format_req};

pub async fn redirect_http(addr: ProxyAddr) -> Result<()> {
    let make_https = move |host: &str, uri: Uri| -> Result<Uri> {
        let mut parts = uri.into_parts();

        parts.scheme = Some(uri::Scheme::HTTPS);

        if parts.path_and_query.is_none() {
            parts.path_and_query = Some("/".parse().unwrap());
        }

        let https_host = host.replace(&addr.http_port.to_string(), &addr.ssl_port.to_string());
        let authority: std::result::Result<Authority, InvalidUri> = https_host.parse::<Authority>();
        let authority = authority?;
        parts.authority = Some(authority);

        Ok(Uri::from_parts(parts)?)
    };

    let redirect = move |method: Method, TypedHeader(host): TypedHeader<Host>, uri: Uri| async move {
        let path = format_req(&method, &uri);

        match make_https(host.hostname(), uri) {
            Ok(uri) => {
                info!("{path} 308 Permanent Redirect");
                Redirect::permanent(&uri.to_string()).into_response()
            }

            Err(error) => {
                info!("{path} 400 Bad Request");
                error_page(StatusCode::BAD_REQUEST, error).into_response()
            }
        }
    };

    axum_server::bind(addr.http_addr())
        .serve(redirect.into_make_service())
        .await?;

    Ok(())
}
