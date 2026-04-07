use std::sync::Arc;

use crate::{broadcaster::Broadcaster, store::AppStore};

pub struct AppConfig {
    pub redis_url: String,
    pub port: u16,
}

impl AppConfig {
    pub fn load_config() -> Self {
        let app_config = AppConfig {
            redis_url: std::env::var("REDIS_URL")
                .unwrap_or_else(|_| "redis://127.0.0.1:6379".to_owned()),
            port: std::env::var("PORT")
                .ok()
                .and_then(|p| p.parse().ok())
                .unwrap_or(8000),
        };

        app_config
    }
}

/// Shared application state injected into every handler.
#[derive(Clone)]
pub struct AppState {
    pub store: Arc<AppStore>,
    pub broadcaster: Broadcaster,
}
