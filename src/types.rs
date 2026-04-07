use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Side {
    Buy,
    Sell,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Order {
    pub id: u64,
    pub side: Side,
    pub price: u64,
    pub qty: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Fill {
    pub maker_order_id: u64,
    pub taker_order_id: u64,
    pub price: u64,
    pub qty: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PriceLevel {
    pub orders: Vec<Order>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OrderBook {
    pub bids: BTreeMap<u64, PriceLevel>,
    pub asks: BTreeMap<u64, PriceLevel>,
    pub next_id: u64,
}

/// Wire format for POST /orders request body.
#[derive(Debug, Serialize, Deserialize)]
pub struct SubmitOrder {
    pub side: Side,
    pub price: u64,
    pub qty: u64,
}

/// Response for POST /orders.
#[derive(Debug, Serialize)]
pub struct SubmitResponse {
    pub order_id: u64,
    pub fills: Vec<Fill>,
}

/// Response for GET /orderbook.
#[derive(Debug, Serialize)]
pub struct OrderBookSnapshot {
    /// Sorted best-to-worst: highest price first.
    pub bids: Vec<LevelSnapshot>,
    /// Sorted best-to-worst: lowest price first.
    pub asks: Vec<LevelSnapshot>,
}

#[derive(Debug, Serialize)]
pub struct LevelSnapshot {
    pub price: u64,
    pub qty: u64,
}
