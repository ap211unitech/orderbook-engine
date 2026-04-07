//! Axum HTTP handlers and WebSocket upgrade handler.

use axum::{
    Json,
    extract::{
        State, WebSocketUpgrade,
        ws::{Message, WebSocket},
    },
    http::StatusCode,
    response::{IntoResponse, Response},
};
use tracing::info;

use crate::{
    broadcaster::Broadcaster,
    setup::AppState,
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

// GET /ws  — WebSocket upgrade
pub async fn ws_handler(ws: WebSocketUpgrade, State(state): State<AppState>) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, state.broadcaster))
}

async fn handle_socket(mut socket: WebSocket, broadcaster: Broadcaster) {
    let mut rx = broadcaster.subscribe();
    tracing::info!("WebSocket client connected");

    loop {
        tokio::select! {
            // Incoming fill from Redis relay.
            result = rx.recv() => {
                match result {
                    Ok(msg) => {
                        tracing::debug!("sending fill to ws client");
                        if socket.send(Message::Text(msg.into())).await.is_err() {
                            break;
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        tracing::warn!("ws client lagged by {n} messages");
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }

            // Client disconnect or ping/pong.
            msg = socket.recv() => {
                match msg {
                    Some(Ok(Message::Close(_))) | None => {
                        info!("WebSocket client disconnected");
                        break;
                    }
                    Some(Ok(Message::Ping(data))) => {
                        let _ = socket.send(Message::Pong(data)).await;
                    }
                    _ => {}
                }
            }
        }
    }
}
