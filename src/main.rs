use std::net::SocketAddr;

use axum::{self, Router, body::Body, extract::Request, routing::get};
use tower_http::trace::TraceLayer;
use tracing_subscriber::FmtSubscriber;

use setup::Tracing;

use crate::config::AppConfig;

mod config;
mod setup;
mod types;

#[tokio::main]
async fn main() {
    // Setting up tracing
    let subscriber = FmtSubscriber::builder()
        .with_max_level(tracing::Level::INFO) // error > warn > info > debug > trace
        .finish();

    tracing::subscriber::set_global_default(subscriber).expect("Failed to setup logging!");

    let app_config = AppConfig::load_config();

    // build our application with a single route
    let app = Router::new()
        .route("/", get(|| async { "Hello, World!" }))
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(|_: &Request<Body>| tracing::info_span!("http"))
                .on_request(Tracing::on_request)
                .on_response(Tracing::on_response)
                .on_failure(Tracing::on_failure),
        );

    let addr = SocketAddr::from(([127, 0, 0, 1], app_config.port));

    // run our app with hyper, listening globally on port 3000
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();

    tracing::info!("Server started on: {} 🚀", listener.local_addr().unwrap());

    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await
    .unwrap();
}
