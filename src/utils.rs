use std::borrow::Cow;

use axum::http::{Method, Uri};
use owo_colors::OwoColorize;
use url::form_urlencoded;

pub fn format_req(method: &Method, uri: &Uri) -> String {
    let path = uri.path();
    let query = format_query(uri.query().unwrap_or(""));

    let method = method.green();
    let path = format!("{path}{query}");
    let path = path.cyan();

    format!("{method} {path}")
}

pub fn format_query(uri: &str) -> String {
    let mut query = Vec::new();

    let parsed = form_urlencoded::parse(uri.as_bytes());
    for (key, value) in parsed {
        if ["apiKey", "access_token"].contains(&key.as_ref()) {
            let value = (key, Cow::Borrowed("******REDACTED******"));

            query.push(value);
        } else {
            query.push((key, value));
        }
    }

    let mut query = form_urlencoded::Serializer::new(String::new())
        .extend_pairs(query)
        .finish();

    if !query.is_empty() {
        query.insert(0, '?');
    }

    query.to_string()
}
