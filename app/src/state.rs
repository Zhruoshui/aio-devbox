// Shared application state. Built once at startup from the parsed service
// registry and shared across handlers via `axum::State`.

use std::sync::Arc;

use crate::config::Service;

#[derive(Clone)]
pub struct AppState {
    pub services: Arc<Vec<Service>>,
}

impl AppState {
    pub fn new(services: Vec<Service>) -> Self {
        Self {
            services: Arc::new(services),
        }
    }
}
