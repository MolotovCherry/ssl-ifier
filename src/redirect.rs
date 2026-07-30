use std::io;

use axum::{
    handler::HandlerWithoutStateExt as _,
    http::{
        Method, StatusCode, Uri,
        uri::{self, Authority, InvalidUri, InvalidUriParts},
    },
    response::{IntoResponse as _, Redirect},
};
use axum_extra::{TypedHeader, headers::Host};
use snafu::{ResultExt, Snafu};
use tracing::info;

use crate::{config::ProxyAddr, error_pages::error_page, utils::format_req};

#[derive(Debug, Snafu)]
pub enum RedirectError {
    #[snafu(display("{source}"))]
    UriParts { source: InvalidUriParts },
    #[snafu(display("{source}"))]
    Uri { source: InvalidUri },
    #[snafu(display("{source}"))]
    Io { source: io::Error },
}

pub async fn redirect_http(addr: ProxyAddr) -> Result<(), RedirectError> {
    let make_https = move |host: &str, uri: Uri| -> Result<Uri, RedirectError> {
        let mut parts = uri.into_parts();

        parts.scheme = Some(uri::Scheme::HTTPS);

        if parts.path_and_query.is_none() {
            parts.path_and_query = Some("/".parse().unwrap());
        }

        let https_host = host.replace(&addr.http_port.to_string(), &addr.ssl_port.to_string());
        let authority = https_host.parse::<Authority>().context(UriSnafu)?;

        parts.authority = Some(authority);

        Uri::from_parts(parts).context(UriPartsSnafu)
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
        .await
        .context(IoSnafu)?;

    Ok(())
}
