use crate::{
    config::Tracker,
    metrics,
    state::state_manager::{SnapshotRequest, TrackOrderRequest},
    ws::handlers,
};
use alloy::primitives::B256;
use axum::{
    Json, Router,
    extract::{Path, State},
    http::{StatusCode, Uri},
    response::{IntoResponse, Response},
    routing::get,
};
use core::str::FromStr;
use eyre::{Result, WrapErr};
use serde::Serialize;
use signet_tracker::OrderStatus;
use signet_tx_cache::TxCache;
use std::{net::SocketAddr, sync::Arc};
use tokio::{
    net::TcpListener,
    sync::{broadcast, mpsc},
    time::Instant,
};
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, instrument};

/// Shared application state available to all HTTP and WS handlers.
pub(crate) struct AppState {
    /// The order tracker (used by the GET endpoint for full on-demand diagnostics).
    pub(crate) tracker: Tracker,
    /// Channel to register orders with the state manager (used by WS handlers).
    pub(crate) track_request_sender: mpsc::Sender<TrackOrderRequest>,
    /// Channel to request a snapshot of all tracked order statuses from the state manager.
    pub(crate) snapshot_request_sender: mpsc::Sender<SnapshotRequest>,
    /// Broadcast sender for order status updates (WS handlers subscribe to this).
    pub(crate) update_sender: broadcast::Sender<OrderStatus>,
    /// Tx-cache client for order lookup in WS handlers.
    pub(crate) tx_cache: TxCache,
}

/// JSON error response body.
#[derive(Serialize)]
struct ErrorResponse {
    error: String,
}

async fn healthcheck() -> Response {
    Json(serde_json::json!({"status": "ok"})).into_response()
}

async fn route_not_found(uri: Uri) -> Response {
    debug!(%uri, "no matching route");
    (StatusCode::NOT_FOUND, Json(ErrorResponse { error: format!("No route for {uri}") }))
        .into_response()
}

/// Parse an order hash from the path, logging and returning a 400 on failure.
#[expect(clippy::result_large_err, reason = "callers return the Response immediately")]
pub(crate) fn parse_order_hash(raw: &str) -> Result<B256, Response> {
    B256::from_str(raw).map_err(|error| {
        let msg = format!("malformed order hash: {error}");
        debug!(msg);
        (StatusCode::BAD_REQUEST, Json(ErrorResponse { error: msg })).into_response()
    })
}

/// Query the status of a single order by hash.
#[instrument(skip_all, fields(order_hash = %raw_hash))]
async fn order_status(
    State(state): State<Arc<AppState>>,
    Path(raw_hash): Path<String>,
) -> Response {
    let order_hash = match parse_order_hash(&raw_hash) {
        Ok(hash) => hash,
        Err(response) => return response,
    };
    let start = Instant::now();
    let response = match state.tracker.status(order_hash).await {
        Ok(report) => {
            metrics::record_request(metrics::RequestResult::Success);
            Json(report).into_response()
        }
        Err(signet_tracker::Error::OrderNotFound(_)) => {
            debug!("order not found in tx-cache");
            metrics::record_request(metrics::RequestResult::NotFound);
            let error = Json(ErrorResponse { error: "order not found in tx-cache".into() });
            (StatusCode::NOT_FOUND, error).into_response()
        }
        Err(err) => {
            error!(%err, "failed to query order status");
            metrics::record_request(metrics::RequestResult::Error);
            let error = Json(ErrorResponse { error: err.to_string() });
            (StatusCode::INTERNAL_SERVER_ERROR, error).into_response()
        }
    };
    metrics::record_request_duration(start.elapsed());
    response
}

/// Serve the tracker HTTP and WebSocket API until cancelled or failure.
///
/// Returns `Ok(())` on graceful cancellation or an error if the server exits unexpectedly.
pub(crate) async fn serve_tracker(
    app_state: Arc<AppState>,
    port: u16,
    cancellation_token: CancellationToken,
) -> Result<()> {
    std::sync::LazyLock::force(&metrics::DESCRIPTIONS);
    let shutdown_token = cancellation_token.clone();

    let result = async {
        let router = Router::new()
            .route("/healthcheck", get(healthcheck))
            .route("/orders/{order_hash}", get(order_status))
            .route("/orders/{order_hash}/ws", get(handlers::single_order_ws))
            .route("/orders/ws", get(handlers::all_orders_ws))
            .fallback(route_not_found)
            .with_state(app_state);
        let addr = SocketAddr::from(([0, 0, 0, 0], port));
        let listener = TcpListener::bind(addr)
            .await
            .wrap_err_with(|| format!("failed to bind tracker server on port {port}"))?;
        axum::serve(listener, router)
            .with_graceful_shutdown(shutdown_token.cancelled_owned())
            .await
            .wrap_err("failed serving tracker")
    }
    .await;

    crate::handle_task_exit("server", result, &cancellation_token)
}
