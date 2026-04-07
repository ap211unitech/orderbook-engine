//! Axum HTTP handlers and WebSocket upgrade handler.

use axum::{
    Json,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use tracing::info;

use crate::{
    config::AppState,
    types::{SubmitOrder, SubmitResponse},
};

// POST /orders
pub async fn post_order(State(state): State<AppState>, Json(req): Json<SubmitOrder>) -> Response {
    match state.store.submit_order(req).await {
        Ok((order_id, fills)) => {
            info!(order_id, fills = fills.len(), "order submitted");
            Json(SubmitResponse { order_id, fills }).into_response()
        }
        Err(e) => {
            tracing::error!("submit_order error: {e}");
            (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response()
        }
    }
}
