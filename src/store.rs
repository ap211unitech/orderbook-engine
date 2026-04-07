//! Redis-backed store.
//!
//! # Multi-instance correctness
//!
//! All order-book mutations run inside a single Lua script executed atomically
//! on the Redis server.  Redis processes Lua scripts single-threaded, so the
//! script acts as a global distributed mutex:
//!
//!   1. INCR order:seq        — allocate a monotonic order ID
//!   2. GET  book             — read the current book (JSON)
//!   3. Match in Lua          — pure price-time priority logic
//!   4. SET  book <new>       — write back the updated book
//!   5. PUBLISH fills <json>  — fan-out fills to every subscribed server
//!
//! No two API servers can interleave their book mutations.
//! There is no double-matching.
//!
//! Every server instance subscribes to the `fills` Redis Pub/Sub channel
//! and forwards arriving events to its local WebSocket clients.

use anyhow::Result;
use redis::{AsyncCommands, Client, Script, aio::ConnectionManager};

use crate::types::{Fill, OrderBook, SubmitOrder};

const BOOK_KEY: &str = "book";
const FILL_CHANNEL: &str = "fills";
const SEQ_KEY: &str = "order:seq";

pub struct AppStore {
    pub conn: ConnectionManager,
    redis_url: String,
    match_script: Script,
}

impl AppStore {
    pub async fn new(redis_url: &str) -> Result<Self> {
        let client = Client::open(redis_url)?;
        let conn = ConnectionManager::new(client).await?;
        Ok(Self {
            conn,
            redis_url: redis_url.to_owned(),
            match_script: Script::new(MATCH_LUA),
        })
    }

    /// Submit an order atomically.  Returns (order_id, fills).
    pub async fn submit_order(&self, req: SubmitOrder) -> Result<(u64, Vec<Fill>)> {
        let order_json = serde_json::to_string(&req)?;

        let result: String = self
            .match_script
            .clone()
            .key(BOOK_KEY)
            .key(SEQ_KEY)
            .key(FILL_CHANNEL)
            .arg(order_json)
            .invoke_async(&mut self.conn.clone())
            .await?;

        let parsed: serde_json::Value = serde_json::from_str(&result)?;
        let order_id = parsed["order_id"].as_u64().unwrap();

        // fills may be a JSON array or an empty object {} (cjson encodes empty
        // Lua tables as arrays, but a populated one as an object — we normalise).
        let fills: Vec<Fill> = match &parsed["fills"] {
            serde_json::Value::Array(arr) => arr
                .iter()
                .map(|v| serde_json::from_value(v.clone()).map_err(anyhow::Error::from))
                .collect::<Result<Vec<_>>>()?,
            // Empty object {} from cjson — no fills.
            serde_json::Value::Object(_) => vec![],
            _ => vec![],
        };

        Ok((order_id, fills))
    }

    /// Return a snapshot of the current order book.
    pub async fn get_book(&self) -> Result<OrderBook> {
        let raw: Option<String> = self.conn.clone().get(BOOK_KEY).await?;
        match raw {
            Some(s) => Ok(serde_json::from_str(&s)?),
            None => Ok(OrderBook::default()),
        }
    }
}

// Lua script — executes atomically inside Redis.
//
// KEYS[1] = book key
// KEYS[2] = sequence key
// KEYS[3] = fills pub/sub channel
// ARGV[1] = order JSON {side, price, qty}
//
// cjson encodes an empty Lua table as [] (array). To force object encoding
// for bids/asks we use the cjson.empty_array sentinel trick or simply ensure
// we never pass empty sub-tables to cjson. We handle this by checking
// fills length before encoding and by initialising bids/asks carefully.
const MATCH_LUA: &str = r#"
local book_key    = KEYS[1]
local seq_key     = KEYS[2]
local channel_key = KEYS[3]
local order_json  = ARGV[1]

-- Allocate order id
local order_id = tonumber(redis.call('INCR', seq_key))

-- Load or init book
local book_raw = redis.call('GET', book_key)
local book
if book_raw then
    book = cjson.decode(book_raw)
    -- cjson decodes an empty JSON object as an empty Lua table (array-like).
    -- Ensure bids and asks are always treated as hash maps.
    if type(book.bids) ~= 'table' then book.bids = {} end
    if type(book.asks) ~= 'table' then book.asks = {} end
else
    book = { bids = {}, asks = {} }
end

local req   = cjson.decode(order_json)
local side  = req.side
local price = tonumber(req.price)
local qty   = tonumber(req.qty)

local fills = {}

-- Fill taker qty against a resting price level (array of orders, FIFO).
local function fill_against(level, taker_qty, taker_id, exec_price)
    local new_fills = {}
    local i = 1
    while taker_qty > 0 and i <= #level do
        local maker  = level[i]
        local maker_qty = tonumber(maker.qty)
        local fqty   = math.min(taker_qty, maker_qty)
        table.insert(new_fills, {
            maker_order_id = tonumber(maker.id),
            taker_order_id = taker_id,
            price          = exec_price,
            qty            = fqty,
        })
        maker.qty  = maker_qty - fqty
        taker_qty  = taker_qty - fqty
        if maker.qty == 0 then
            table.remove(level, i)
        else
            i = i + 1
        end
    end
    return level, taker_qty, new_fills
end

if side == 'buy' then
    local ask_prices = {}
    for p, _ in pairs(book.asks) do
        table.insert(ask_prices, tonumber(p))
    end
    table.sort(ask_prices)

    for _, ap in ipairs(ask_prices) do
        if qty == 0 then break end
        if ap > price then break end
        local key   = tostring(ap)
        local level = book.asks[key]
        local new_fills
        level, qty, new_fills = fill_against(level, qty, order_id, ap)
        if #level > 0 then
            book.asks[key] = level
        else
            book.asks[key] = nil
        end
        for _, f in ipairs(new_fills) do table.insert(fills, f) end
    end

    if qty > 0 then
        local key = tostring(price)
        if not book.bids[key] then book.bids[key] = {} end
        table.insert(book.bids[key], { id = order_id, price = price, qty = qty })
    end

else  -- sell
    local bid_prices = {}
    for p, _ in pairs(book.bids) do
        table.insert(bid_prices, tonumber(p))
    end
    table.sort(bid_prices, function(a, b) return a > b end)

    for _, bp in ipairs(bid_prices) do
        if qty == 0 then break end
        if bp < price then break end
        local key   = tostring(bp)
        local level = book.bids[key]
        local new_fills
        level, qty, new_fills = fill_against(level, qty, order_id, bp)
        if #level > 0 then
            book.bids[key] = level
        else
            book.bids[key] = nil
        end
        for _, f in ipairs(new_fills) do table.insert(fills, f) end
    end

    if qty > 0 then
        local key = tostring(price)
        if not book.asks[key] then book.asks[key] = {} end
        table.insert(book.asks[key], { id = order_id, price = price, qty = qty })
    end
end

-- Persist updated book.
redis.call('SET', book_key, cjson.encode(book))

-- Build result. We return fills as an array always.
-- Encode fills separately to avoid cjson empty-table ambiguity.
local result
if #fills > 0 then
    local fills_json = cjson.encode(fills)
    redis.call('PUBLISH', channel_key, cjson.encode({ fills = fills }))
    return '{"order_id":' .. order_id .. ',"fills":' .. fills_json .. '}'
else
    return '{"order_id":' .. order_id .. ',"fills":[]}'
end
"#;
