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
    types::{LevelSnapshot, OrderBookSnapshot, SubmitOrder, SubmitResponse},
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

// GET /orderbook
pub async fn get_orderbook(State(state): State<AppState>) -> Response {
    match state.store.get_book().await {
        Ok(book) => {
            // Bids: highest price first.
            let bids = book
                .bids
                .iter()
                .rev()
                .map(|(price, level)| LevelSnapshot {
                    price: *price,
                    qty: level.orders.iter().map(|o| o.qty).sum(),
                })
                .collect();

            // Asks: lowest price first.
            let asks = book
                .asks
                .iter()
                .map(|(price, level)| LevelSnapshot {
                    price: *price,
                    qty: level.orders.iter().map(|o| o.qty).sum(),
                })
                .collect();

            Json(OrderBookSnapshot { bids, asks }).into_response()
        }
        Err(e) => {
            tracing::error!("get_book error: {e}");
            (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response()
        }
    }
}
