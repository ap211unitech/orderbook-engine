# Order Matching Engine

A order matching engine for a prediction market, built in Rust with Axum. Supports multiple concurrent API server instances, price-time priority matching, and real-time fill broadcasts over WebSocket.

Watch demo: https://drive.google.com/file/d/1RHSigSAgP2m1LudzqP5lWdPACtRBPDx-/view?usp=sharing

---

## Quick Start

### Prerequisites

- Rust 1.90+ (or any recent stable)
- Redis 6+

### Run with Docker

```bash
docker compose up
```

This command spawns two server instances, available at:

- http://0.0.0.0:8080
- http://0.0.0.0:8081

Both services will be up and running once the containers have successfully started.

---

## Design Decisions

### 1. How does the system handle multiple API server instances without double-matching?

Redis atomic Lua scripts. Every order mutation is a single `EVAL` call that Redis executes single-threaded, making it a global distributed mutex.

When an order arrives at any API server instance, the server does _no local matching_. Instead it fires a Lua script at Redis via `EVAL`. Redis guarantees Lua scripts are atomic — no other command or script can interleave while one is executing. The script:

1. `INCR order:seq` — allocates a globally unique, monotonically increasing order ID.
2. `GET book` — reads the current book state (JSON blob).
3. Runs price-time priority matching entirely inside Lua.
4. `SET book <updated>` — writes back the new book state.
5. `PUBLISH fills <json>` — broadcasts fills to all subscribers.

Because step 2–4 is a single atomic unit, two concurrent `POST /orders` requests hitting different server instances will never see the same stale book state. One script wins the serialisation and commits first; the second runs against the already-updated book.

This is the classical "Redis as a serialisation point" pattern. It trades throughput (one Redis round-trip per order) for simplicity and correctness.

**Why not optimistic locking / WATCH + MULTI/EXEC?** It would work, but requires retry logic on the client side. Lua is simpler and has equivalent atomicity guarantees.

### 2. What data structure did you use for the order book and why?

The in-memory book (which mirrored by the Lua script) is:

```
BTreeMap<price: u64, PriceLevel { orders: Vec<Order> }>
```

One `BTreeMap` for bids, one for asks.

**Why `BTreeMap`:**

- O(log n) insert and removal by price.
- O(1) best-price access: `.iter().next_back()` for bids (highest first), `.iter().next()` for asks (lowest first). No scanning.
- Naturally sorted — no manual heap maintenance.
- Simple to reason about and test.

**Why `Vec<Order>` within a price level:**

- Arrival order is preserved automatically (FIFO = time priority within a price level).
- O(1) push at the back (new resting order).
- O(1) removal from the front (filled maker order) — or O(k) if partial fills leave a partial order at position 0, which is fine at this scale.

The Redis-side "book" is stored as a JSON blob keyed by price string. This matches the `BTreeMap` structure and is sufficient for the this scale. At higher volume I might require a more compact binary format (e.g. FlatBuffers).

### 3. What breaks first under real production load?

Redis is the bottleneck and the single point of failure.

Every `POST /orders` blocks on a single Redis round-trip + Lua execution. Redis is fast (100k+ ops/sec on commodity hardware), but:

- Lua scripts are not parallelisable on a single Redis instance. All order submissions are serialised.
- A large order that sweeps many price levels means more Lua execution time, which blocks other orders longer.
- If Redis goes down, the entire system stops accepting orders.

Second: **WebSocket fan-out is per-instance and in-memory**. If a single instance has 100k connected clients and a flood of fills arrives, the broadcast loop will become a bottleneck. There is no backpressure — slow clients get `Lagged` errors and drop messages.

To scale past the Redis bottleneck, the book can be sharded by market — each market gets its own Redis key and Lua script, so submissions to different markets no longer contend. For WebSocket fan-out, we can replace the in-process broadcast with a dedicated pub/sub layer like Redis Streams or Kafka so fill delivery scales independently from order matching and slow clients can be drained without affecting others.

### 4. What would you build next with another 4 hours?

I can have these in order:

- Add an order cancel endpoint (`DELETE /orders/:id`).

- Redis Sentinel or Cluster for HA. The Lua approach works transparently with a single-primary setup.

- Replace the JSON book blob with a more compact representation (store bids and asks as Redis sorted sets keyed by price, with order IDs in lists per price level). This eliminates the full read/write of the entire book on every order and scales to large books.

- Prometheus metrics: order submission latency (p50/p99), fill rate, book depth, WebSocket client count.

---

## Architecture Diagram

```
  Client A          Client B          Client C (WS)
     │                 │                   │
     ▼                 ▼                   │
┌─────────┐      ┌─────────┐         ┌─────────┐
│ Server  │      │ Server  │         │ Server  │
│ :8080   │      │ :8081   │         │  :8082  │
└────┬────┘      └────┬────┘         └────┬────┘
     │                │                   │
     │  EVAL (Lua)    │     EVAL (Lua)    │ SUBSCRIBE
     │◄──────────────►│ ◄───────────────► │
     ▼                ▼                   ▼
┌──────────────────────────────────────────────┐
│                    Redis                     │
│  book (JSON blob)   order:seq   fills (PS)   │
└──────────────────────────────────────────────┘
                          │
              PUBLISH fills│
                          │
          ┌───────────────┼───────────────┐
          ▼               ▼               ▼
      Server :8080    Server :8081    Server :8082
      (relay task)    (relay task)    (relay task)
          │               │               │
     WS clients      WS clients      WS clients
```

Each server instance runs a background relay task that subscribes to the Redis `fills` channel and fans events out to its local WebSocket clients via a `tokio::sync::broadcast` channel.

---

## API

### `POST /orders`

Submit a new order.

**Request body:**

```json
{ "side": "buy", "price": 100, "qty": 10 }
```

**Response:**

```json
{
  "order_id": 42,
  "fills": [
    { "maker_order_id": 7, "taker_order_id": 42, "price": 100, "qty": 10 }
  ]
}
```

### `GET /orderbook`

Returns the current order book snapshot.

```json
{
  "bids": [
    { "price": 100, "qty": 15 },
    { "price": 99, "qty": 8 }
  ],
  "asks": [
    { "price": 101, "qty": 5 },
    { "price": 102, "qty": 20 }
  ]
}
```

### `GET /ws`

WebSocket endpoint. Connect and receive fill events in real time.

Each message is a JSON object:

```json
{
  "fills": [
    { "maker_order_id": 7, "taker_order_id": 42, "price": 100, "qty": 10 }
  ]
}
```

---

## Project Structure

```
src/
  main.rs        — wires everything together, starts server
  setup.rs       — Set up Tracing, Config, App State
  types.rs       — Order, Fill, Side, OrderBook, wire types
  store.rs       — Redis store: atomic Lua matching + pub/sub
  broadcaster.rs — Redis->WebSocket relay, tokio broadcast fan-out
  handlers.rs    — Axum HTTP and WebSocket handlers
```
