use crate::{
    metrics::WsEndpoint,
    service::AppState,
    state::state_manager::TrackOrderRequest,
    ws::messages::{OrderFilter, StatusFilter},
};
use alloy::primitives::B256;
use axum::extract::{
    Path, State,
    ws::{Message, WebSocket},
};
use axum::response::Response;
use core::pin::pin;
use futures_util::stream::SplitSink;
use futures_util::{SinkExt, StreamExt, TryStreamExt};
use signet_tracker::OrderStatus;
use std::sync::Arc;
use tokio::sync::{broadcast::error::RecvError, oneshot};
use tracing::{debug, error, instrument, warn};

/// How long to wait for a WebSocket send before considering the client unresponsive.
const WS_SEND_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(2);

/// WebSocket handler for subscribing to a single order's status.
///
/// Sends the initial report, then status updates on each change. Closes when the order reaches a
/// terminal state (filled or expired).
#[instrument(skip(state, ws), fields(%order_hash))]
pub(crate) async fn single_order_ws(
    State(state): State<Arc<AppState>>,
    Path(order_hash): Path<B256>,
    ws: axum::extract::ws::WebSocketUpgrade,
) -> Response {
    ws.on_upgrade(move |socket| handle_single_order(socket, state, order_hash))
}

async fn handle_single_order(socket: WebSocket, state: Arc<AppState>, order_hash: B256) {
    let _guard = WsMetricsGuard::new(WsEndpoint::Single);
    let (mut ws_sender, mut ws_receiver) = socket.split();

    debug!("single-order subscription started");

    // Look up the order in the tx-cache.
    let order = {
        let mut stream = pin!(state.tx_cache.stream_orders());
        let mut found = None;
        while let Ok(Some(order)) = stream.try_next().await {
            if *order.order_hash() == order_hash {
                found = Some(order);
                break;
            }
        }
        found
    };

    let Some(order) = order else {
        let error = serde_json::json!({"error": "order not found in tx-cache"});
        timed_send(&mut ws_sender, Message::text(error.to_string())).await;
        close_connection(&mut ws_sender, "single-order").await;
        return;
    };

    // Register with state manager and get initial report.
    let (initial_sender, initial_receiver) = oneshot::channel();
    let request = TrackOrderRequest { order, initial_report_sender: Some(initial_sender) };

    if state.track_request_sender.send(request).await.is_err() {
        warn!("state manager channel closed");
        close_connection(&mut ws_sender, "single-order").await;
        return;
    }

    let Ok(initial_report) = initial_receiver.await else {
        warn!("failed to receive initial report from state manager");
        close_connection(&mut ws_sender, "single-order").await;
        return;
    };

    // Send initial snapshot.
    if !send_status(WsEndpoint::Single, &mut ws_sender, &initial_report).await {
        debug!("single-order client disconnected");
        return;
    }

    // If already terminal, close immediately.
    if initial_report.is_terminal() {
        debug!("order already in terminal state, closing");
        close_connection(&mut ws_sender, "single-order").await;
        return;
    }

    // Subscribe to broadcast updates and filter for this order.
    let mut update_receiver = state.update_sender.subscribe();

    loop {
        tokio::select! {
            result = update_receiver.recv() => {
                match result {
                    Ok(status) if status.order_hash() == order_hash => {
                        let terminal = status.is_terminal();
                        if !send_status(WsEndpoint::Single, &mut ws_sender, &status).await {
                            debug!("single-order client disconnected");
                            return;
                        }
                        if terminal {
                            debug!("order reached terminal state, closing");
                            close_connection(&mut ws_sender, "single-order").await;
                            return;
                        }
                    }
                    Ok(_) => continue,
                    Err(RecvError::Lagged(missed)) => {
                        warn!(%missed, "single-order websocket broadcast receiver lagged");
                    }
                    Err(RecvError::Closed) => {
                        debug!("broadcast channel closed, closing single-order connection");
                        close_connection(&mut ws_sender, "single-order").await;
                        return;
                    }
                }
            }
            msg = ws_receiver.next() => {
                match msg {
                    Some(Ok(Message::Close(_))) | None => {
                        debug!("single-order client disconnected");
                        return;
                    }
                    _ => {}
                }
            }
        }
    }
}

/// WebSocket handler for subscribing to all orders.
///
/// The client may send a JSON [`OrderFilter`] at any time to change which updates are forwarded.
/// Sends updates for all matching orders until the client disconnects.
#[instrument(skip(state, ws))]
pub(crate) async fn all_orders_ws(
    State(state): State<Arc<AppState>>,
    ws: axum::extract::ws::WebSocketUpgrade,
) -> Response {
    ws.on_upgrade(move |socket| handle_all_orders(socket, state))
}

async fn handle_all_orders(socket: WebSocket, state: Arc<AppState>) {
    let _guard = WsMetricsGuard::new(WsEndpoint::All);
    let (mut ws_sender, mut ws_receiver) = socket.split();

    let mut filter = OrderFilter::default();

    debug!("all-orders subscription started");

    let mut update_receiver = state.update_sender.subscribe();

    loop {
        tokio::select! {
            result = update_receiver.recv() => {
                match result {
                    Ok(status) if matches_filter(&status, &filter) => {
                        if !send_status(WsEndpoint::All, &mut ws_sender, &status).await {
                            debug!("all-orders client disconnected");
                            return;
                        }
                    }
                    Ok(_) => continue,
                    Err(RecvError::Lagged(missed)) => {
                        warn!(%missed, "all-orders websocket broadcast receiver lagged");
                    }
                    Err(RecvError::Closed) => {
                        debug!("broadcast channel closed, closing all-orders connection");
                        close_connection(&mut ws_sender, "all-orders").await;
                        return;
                    }
                }
            }
            msg = ws_receiver.next() => {
                match msg {
                    Some(Ok(Message::Text(text))) => {
                        match serde_json::from_str::<OrderFilter>(&text) {
                            Ok(new_filter) => {
                                debug!(?new_filter, "filter updated");
                                filter = new_filter;
                            }
                            Err(error) => {
                                warn!(%error, "invalid filter message, ignoring");
                            }
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => {
                        debug!("all-orders client disconnected");
                        return;
                    }
                    _ => {}
                }
            }
        }
    }
}

/// Send a JSON-serialized status over the WebSocket. Returns `false` if sending fails or times out.
async fn send_status(
    endpoint: WsEndpoint,
    sender: &mut SplitSink<WebSocket, Message>,
    status: &OrderStatus,
) -> bool {
    let json = match serde_json::to_string(status) {
        Ok(json) => json,
        Err(error) => {
            error!(order_hash = %status.order_hash(), %error, "failed to serialize status");
            return false;
        }
    };
    if !timed_send(sender, Message::text(json)).await {
        return false;
    }
    crate::metrics::ws_message_sent(endpoint);
    true
}

/// Send a Close frame and shut down the sink so the TCP connection is properly torn down.
async fn close_connection(sender: &mut SplitSink<WebSocket, Message>, context: &str) {
    timed_send(sender, Message::Close(None)).await;
    let _ = sender.close().await;
    debug!("{context} client disconnected");
}

/// Send a message with a timeout. Returns `false` if the send fails or times out.
async fn timed_send(sender: &mut SplitSink<WebSocket, Message>, message: Message) -> bool {
    match tokio::time::timeout(WS_SEND_TIMEOUT, sender.send(message)).await {
        Ok(Ok(())) => true,
        Ok(Err(error)) => {
            debug!(%error, "websocket send failed");
            false
        }
        Err(_) => {
            warn!(
                timeout = WS_SEND_TIMEOUT.as_secs_f32(),
                "websocket send timed out, dropping unresponsive client"
            );
            false
        }
    }
}

/// Drop guard that tracks WS connection open/close in metrics.
struct WsMetricsGuard {
    endpoint: WsEndpoint,
}

impl WsMetricsGuard {
    fn new(endpoint: WsEndpoint) -> Self {
        crate::metrics::ws_connection_opened(endpoint);
        Self { endpoint }
    }
}

impl Drop for WsMetricsGuard {
    fn drop(&mut self) {
        crate::metrics::ws_connection_closed(self.endpoint);
    }
}

/// Check if a status matches the given filter.
fn matches_filter(status: &OrderStatus, filter: &OrderFilter) -> bool {
    if let Some(statuses) = &filter.statuses {
        let status_matches = statuses.iter().any(|filter_status| {
            matches!(
                (filter_status, status),
                (StatusFilter::Pending, OrderStatus::Pending { .. })
                    | (StatusFilter::Filled, OrderStatus::Filled { .. })
                    | (StatusFilter::Expired, OrderStatus::Expired { .. })
            )
        });
        if !status_matches {
            return false;
        }
    }

    // TODO: owner filtering requires adding owner to OrderStatus or filtering at the state manager.

    true
}
