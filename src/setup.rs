use std::{sync::Arc, time::Duration};

use axum::{body::Body, extract::Request, http::Response};
use tower_http::classify::ServerErrorsFailureClass;
use tracing::Span;

use crate::{broadcaster::Broadcaster, store::AppStore};

pub struct Tracing;

impl Tracing {
    pub fn on_request(request: &Request<Body>, _: &Span) {
        tracing::info!(
            "HTTP request: {} {}",
            request.method(),
            request.uri().path()
        )
    }

    pub fn on_response(response: &Response<Body>, latency: Duration, _: &Span) {
        tracing::info!("HTTP response: {} {:?}", response.status(), latency)
    }

    pub fn on_failure(error: ServerErrorsFailureClass, latency: Duration, _: &Span) {
        tracing::error!("Request failed: {:?} after {:?}", error, latency)
    }
}

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
