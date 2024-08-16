use std::{
    convert::Infallible,
    sync::{atomic::Ordering, Arc},
};

use axum::{body::Body, extract::Request, response::Response};
use reqwest::{Body as ReqwestBody, StatusCode};
use tracing::info;

use crate::{error_pages::error_page, utils::format_req, StateData};

pub async fn proxy(req: Request<Body>) -> Result<Response<Body>, Infallible> {
    let data = req.extensions().get::<Arc<StateData>>().unwrap().clone();

    let health = data.health.load(Ordering::Acquire);
    if !health {
        let page = error_page(
            StatusCode::BAD_GATEWAY,
            format!(
                "Health check failed for {}, service is down",
                data.config.addresses.backend
            ),
        );

        return Ok(page);
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

    let stream = body.into_data_stream();
    let req_body = ReqwestBody::wrap_stream(stream);

    let reqwest = match data
        .client
        .request(method.clone(), url)
        .headers(headers)
        .body(req_body)
        .send()
        .await
    {
        Ok(res) => res,
        Err(e) => {
            return Ok(error_page(StatusCode::BAD_GATEWAY, e));
        }
    };

    info!("{} {}", format_req(&method, &parts.uri), reqwest.status());

    let mut response = Response::builder();

    if let Some(map) = response.headers_mut() {
        *map = reqwest.headers().clone();
    }

    if data.config.options.http_support {
        response = response.header("Content-Security-Policy", "upgrade-insecure-requests");
    }

    let response = response
        .status(reqwest.status())
        .body(Body::from_stream(reqwest.bytes_stream()))
        .unwrap_or_else(|_| {
            error_page(
                StatusCode::INTERNAL_SERVER_ERROR,
                "failed to get reqwest byte stream",
            )
        });

    Ok(response)
}
