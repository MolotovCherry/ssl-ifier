use std::{net::SocketAddr, sync::Arc};

use axum::{
    extract::{ConnectInfo, Request, State},
    http::{HeaderValue, header::HOST},
    middleware::Next,
    response::Response,
};

use crate::StateData;

pub async fn kavita(
    conn: ConnectInfo<SocketAddr>,
    State(data): State<Arc<StateData>>,
    mut req: Request,
    next: Next,
) -> Response {
    // https://wiki.kavitareader.com/installation/remote-access/nginx-example/

    let ip = conn.ip().to_string();

    let headers = req.headers_mut();

    if let Ok(val) = HeaderValue::from_str(&data.config.addresses.host) {
        headers.insert(HOST, val);
    }

    if let Ok(val) = HeaderValue::from_str(&ip) {
        headers.insert("X-Real-IP", val);
    }

    //
    // X-Forwarded-For
    //

    let xff = match headers.get("X-Forwarded-For") {
        Some(existing) => {
            let list = existing.to_str().unwrap_or("");

            if list.is_empty() {
                ip
            } else {
                format!("{list}, {ip}")
            }
        }

        None => ip,
    };

    if let Ok(val) = HeaderValue::from_str(&xff) {
        headers.insert("X-Forwarded-For", val);
    }

    //
    // X-Forwarded-Proto
    //

    headers.insert("X-Forwarded-Proto", HeaderValue::from_static("https"));

    //
    // Request
    //

    next.run(req).await
}
