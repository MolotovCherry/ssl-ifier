use std::{
    sync::{atomic::Ordering, Arc},
    time::Duration,
};

use tokio::{task, time::sleep};

use crate::StateData;

pub fn health_check(data: Arc<StateData>) {
    let data = data.clone();

    task::spawn(async move {
        let backend = &data.config.addresses.backend;
        let health_api = &data.config.addresses.health_check.as_ref().unwrap();
        let url = format!("http://{backend}{health_api}");

        loop {
            match data.client.get(&url).send().await {
                Ok(_) => data.health.store(true, Ordering::Release),
                Err(_) => data.health.store(false, Ordering::Release),
            }

            sleep(Duration::from_secs(5)).await;
        }
    });
}
