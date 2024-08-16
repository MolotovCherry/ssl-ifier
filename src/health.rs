use std::sync::atomic::Ordering;

use crate::StateData;

pub async fn health_check(data: &StateData, backend: &str, health_api: &str) {
    let url = format!("http://{backend}{health_api}");

    match data.client.get(url).send().await {
        Ok(_) => data.health.store(true, Ordering::Release),
        Err(_) => data.health.store(false, Ordering::Release),
    }
}
