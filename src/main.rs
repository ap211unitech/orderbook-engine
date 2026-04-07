mod broadcaster;
mod config;
mod handlers;
mod setup;
mod store;
mod types;

use std::{net::SocketAddr, sync::Arc};

use anyhow::Result;
use axum::{
    self, Router,
    body::Body,
    extract::Request,
    routing::{get, post},
};
use tower_http::{cors::CorsLayer, trace::TraceLayer};
use tracing_subscriber::FmtSubscriber;

use setup::Tracing;

use crate::{
    broadcaster::Broadcaster,
    config::{AppConfig, AppState},
    handlers::{get_orderbook, post_order, ws_handler},
    store::AppStore,
};

#[tokio::main]
async fn main() -> Result<()> {
    // Setting up tracing
    let subscriber = FmtSubscriber::builder()
        .with_max_level(tracing::Level::INFO) // error > warn > info > debug > trace
        .finish();

    tracing::subscriber::set_global_default(subscriber).expect("Failed to setup logging!");

    let app_config = AppConfig::load_config();

    tracing::info!("Connecting to Redis at {}", app_config.redis_url);
    let store = Arc::new(AppStore::new(&app_config.redis_url).await?);
    tracing::info!("Connected to Redis ✅");

    // Broadcaster: receives fills from Redis relay, fans out to WS clients.
    let broadcaster = Broadcaster::new();
    broadcaster::spawn_redis_relay(Arc::clone(&store), broadcaster.clone()).await;

    let state = AppState { store, broadcaster };

    // build our application with a single route
    let app = Router::new()
        .route("/health", get(|| async { "Server is healthy!" }))
        .route("/orders", post(post_order))
        .route("/orderbook", get(get_orderbook))
        .route("/ws", get(ws_handler))
        .layer(CorsLayer::permissive())
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(|_: &Request<Body>| tracing::info_span!("http"))
                .on_request(Tracing::on_request)
                .on_response(Tracing::on_response)
                .on_failure(Tracing::on_failure),
        )
        .with_state(state);

    let addr = SocketAddr::from(([127, 0, 0, 1], app_config.port));

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();

    tracing::info!("Server started on: {} 🚀", listener.local_addr().unwrap());

    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await?;

    Ok(())
}
