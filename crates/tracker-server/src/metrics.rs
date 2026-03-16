use core::time::Duration;
use init4_bin_base::deps::metrics::{
    counter, describe_counter, describe_gauge, describe_histogram, gauge, histogram,
};
use signet_tracker::Chain;
use std::sync::LazyLock;

// HTTP request metrics.
const REQUESTS: &str = "signet.tracker.requests";
const REQUEST_DURATION_SECONDS: &str = "signet.tracker.request_duration_seconds";

// WebSocket metrics.
const WS_CONNECTIONS: &str = "signet.tracker.ws_connections";
const WS_MESSAGES_SENT: &str = "signet.tracker.ws_messages_sent";

// State metrics.
const TRACKED_ORDERS: &str = "signet.tracker.tracked_orders";
const EVENT_STORE_FILLED: &str = "signet.tracker.event_store_filled_events";
const EVENT_STORE_ORDER: &str = "signet.tracker.event_store_order_events";

// Ingestion metrics.
const BLOCKS_RECEIVED: &str = "signet.tracker.blocks_received";
const CHAIN_EVENTS_RECEIVED: &str = "signet.tracker.chain_events_received";
const ORDERS_DISCOVERED: &str = "signet.tracker.orders_discovered";

/// Force evaluation to register all metric descriptions with the exporter.
pub(crate) static DESCRIPTIONS: LazyLock<()> = LazyLock::new(|| {
    describe_counter!(REQUESTS, "Order status HTTP requests (label: result)");
    describe_histogram!(REQUEST_DURATION_SECONDS, "Duration of order status HTTP requests");
    describe_gauge!(WS_CONNECTIONS, "Active WebSocket connections (label: endpoint)");
    describe_counter!(WS_MESSAGES_SENT, "WebSocket messages sent (label: endpoint)");
    describe_gauge!(TRACKED_ORDERS, "Number of orders being tracked by the state manager");
    describe_gauge!(EVENT_STORE_FILLED, "Filled events in the in-memory event store");
    describe_gauge!(EVENT_STORE_ORDER, "Order events in the in-memory event store");
    describe_counter!(BLOCKS_RECEIVED, "New blocks received from subscriptions (label: chain)");
    describe_counter!(
        CHAIN_EVENTS_RECEIVED,
        "Chain events received from subscriptions (label: kind)"
    );
    describe_counter!(ORDERS_DISCOVERED, "New orders discovered from tx-cache polling");
});

// --- Label enums ---

/// Label values for the `result` label on HTTP request metrics.
#[derive(Clone, Copy)]
pub(crate) enum RequestResult {
    Success,
    NotFound,
    Error,
}

impl RequestResult {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Success => "success",
            Self::NotFound => "not_found",
            Self::Error => "error",
        }
    }
}

/// Label values for the `endpoint` label on WebSocket metrics.
#[derive(Clone, Copy)]
pub(crate) enum WsEndpoint {
    Single,
    All,
}

impl WsEndpoint {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Single => "single_order",
            Self::All => "all_orders",
        }
    }
}

/// Label values for the `kind` label on chain event metrics.
#[derive(Clone, Copy)]
pub(crate) enum ChainEventKind {
    Filled,
    Order,
}

impl ChainEventKind {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Filled => "filled",
            Self::Order => "order",
        }
    }
}

// --- HTTP request metrics ---

pub(crate) fn record_request(result: RequestResult) {
    counter!(REQUESTS, "result" => result.as_str()).increment(1);
}

pub(crate) fn record_request_duration(elapsed: Duration) {
    histogram!(REQUEST_DURATION_SECONDS).record(elapsed.as_secs_f64());
}

// --- WebSocket metrics ---

pub(crate) fn ws_connection_opened(endpoint: WsEndpoint) {
    gauge!(WS_CONNECTIONS, "endpoint" => endpoint.as_str()).increment(1.0);
}

pub(crate) fn ws_connection_closed(endpoint: WsEndpoint) {
    gauge!(WS_CONNECTIONS, "endpoint" => endpoint.as_str()).decrement(1.0);
}

pub(crate) fn ws_message_sent(endpoint: WsEndpoint) {
    counter!(WS_MESSAGES_SENT, "endpoint" => endpoint.as_str()).increment(1);
}

// --- State metrics ---

pub(crate) fn set_tracked_orders(count: usize) {
    gauge!(TRACKED_ORDERS).set(count as f64);
}

pub(crate) fn set_event_store_filled(count: usize) {
    gauge!(EVENT_STORE_FILLED).set(count as f64);
}

pub(crate) fn set_event_store_order(count: usize) {
    gauge!(EVENT_STORE_ORDER).set(count as f64);
}

// --- Ingestion metrics ---

pub(crate) fn record_block_received(chain: Chain) {
    counter!(BLOCKS_RECEIVED, "chain" => chain.as_str()).increment(1);
}

pub(crate) fn record_chain_event(kind: ChainEventKind) {
    counter!(CHAIN_EVENTS_RECEIVED, "kind" => kind.as_str()).increment(1);
}

pub(crate) fn record_orders_discovered(count: usize) {
    counter!(ORDERS_DISCOVERED).increment(count as u64);
}
