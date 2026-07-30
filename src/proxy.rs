use std::{convert::Infallible, sync::Arc};

use axum::{
    body::Body,
    extract::{Request, State},
    http::StatusCode,
    response::Response,
};
use tracing::info;

use crate::{StateData, error_pages::error_page, utils::format_req};

pub async fn proxy(
    State(state): State<Arc<StateData>>,
    req: Request<Body>,
) -> Result<Response<Body>, Infallible> {
    let (parts, body) = req.into_parts();

    let path = parts
        .uri
        .path_and_query()
        .map(|i| i.as_str())
        .unwrap_or("/");

    let uri = format!("http://{}{path}", state.config.addresses.backend);
    let mut builder = Request::builder().method(&parts.method).uri(uri);
    if let Some(hm) = builder.headers_mut() {
        *hm = parts.headers;
    }

    let request = match builder.body(body) {
        Ok(r) => r,
        Err(e) => return Ok(error_page(StatusCode::INTERNAL_SERVER_ERROR, e)),
    };

    match state.client.request(request).await {
        Ok(res) => {
            info!("{} {}", format_req(&parts.method, &parts.uri), res.status());
            let res = res.map(Body::new);
            Ok(res)
        }

        Err(err) => Ok(error_page(StatusCode::BAD_GATEWAY, err)),
    }
}
