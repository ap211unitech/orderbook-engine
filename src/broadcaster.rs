//! In-process WebSocket broadcaster.
//!
//! A background task subscribes to the Redis `fills` pub/sub channel.
//! When a fill arrives it broadcasts the JSON to every currently-connected
//! WebSocket client via `tokio::sync::broadcast`.
//!
//! Each API server instance runs its own copy of this task, so every
//! instance fans out to its own connected clients.  Because *every* instance
//! subscribes to the same Redis channel, a fill produced by any instance
//! reaches all clients on all instances.

use std::sync::Arc;
use tokio::sync::broadcast;
use tracing::{error, info};

use crate::store::AppStore;

/// Shared broadcaster handle — cheaply cloneable.
#[derive(Clone)]
pub struct Broadcaster {
    tx: broadcast::Sender<String>,
}

impl Broadcaster {
    pub fn new() -> Self {
        // 1024-message buffer.  Slow clients that fall too far behind will
        // receive `RecvError::Lagged` — acceptable for a feed.
        let (tx, _) = broadcast::channel(1_024);
        Self { tx }
    }

    /// Subscribe to the broadcast stream.
    pub fn subscribe(&self) -> broadcast::Receiver<String> {
        self.tx.subscribe()
    }

    /// Publish a raw JSON string to all subscribers.
    pub fn publish(&self, msg: String) {
        // Ignore errors — they just mean there are no current subscribers.
        let _ = self.tx.send(msg);
    }
}

/// Spawn a background task that listens on Redis pub/sub and fans out fills.
pub async fn spawn_redis_relay(store: Arc<AppStore>, broadcaster: Broadcaster) {
    tokio::spawn(async move {
        loop {
            match relay_loop(&store, &broadcaster).await {
                Ok(()) => info!("Redis relay exited cleanly, restarting"),
                Err(e) => error!("Redis relay error: {e}, restarting"),
            }
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        }
    });
}

async fn relay_loop(store: &AppStore, broadcaster: &Broadcaster) -> anyhow::Result<()> {
    use futures_util::StreamExt;

    let mut pubsub = store.subscribe_fills().await?;
    info!("Redis pub/sub relay connected");

    let mut stream = pubsub.on_message();
    while let Some(msg) = stream.next().await {
        let payload: String = msg.get_payload()?;
        broadcaster.publish(payload);
    }

    Ok(())
}
