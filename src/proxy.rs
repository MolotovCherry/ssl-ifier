use std::{convert::Infallible, sync::Arc};

use axum::{
    body::Body,
    extract::{Request, State},
    http::{HeaderMap, HeaderValue, Method, StatusCode, Uri},
    response::Response,
};
use tracing::{error, info};

use crate::{StateData, error_pages::error_page, utils::format_req};

pub async fn proxy(
    State(state): State<Arc<StateData>>,
    uri: Uri,
    method: Method,
    headers: HeaderMap<HeaderValue>,
    body: Body,
) -> Result<Response<Body>, Infallible> {
    let path = uri.path_and_query().map(|i| i.as_str()).unwrap_or("/");

    let url = format!("http://{}{path}", state.config.addresses.backend);
    let mut builder = Request::builder().method(&method).uri(url);
    match builder.headers_mut() {
        Some(h) => *h = headers,
        None => {
            let error = "Failed to add headers to request";
            error!("Internal Server Error: {error}");
            let page = error_page(StatusCode::INTERNAL_SERVER_ERROR, error);
            return Ok(page);
        }
    }

    let req = match builder.body(body) {
        Ok(r) => r,
        Err(e) => {
            error!("Internal Server Error: {e}");
            return Ok(error_page(StatusCode::INTERNAL_SERVER_ERROR, e));
        }
    };

    match state.client.request(req).await {
        Ok(res) => {
            info!("{} {}", format_req(&method, &uri), res.status());
            let res = res.map(Body::new);
            Ok(res)
        }

        Err(err) => {
            error!("Bad Gateway: {err}");
            let page = error_page(StatusCode::BAD_GATEWAY, err);
            Ok(page)
        }
    }
}
