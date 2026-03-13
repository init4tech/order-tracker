use crate::{config::Tracker, metrics};
use alloy::primitives::B256;
use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
};
use eyre::{Report, Result, WrapErr, bail};
use serde::Serialize;
use std::{net::SocketAddr, sync::Arc};
use tokio::{net::TcpListener, task::JoinHandle, time::Instant};
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, instrument};

/// JSON error response body.
#[derive(Serialize)]
struct ErrorResponse {
    error: String,
}

async fn healthcheck() -> Response {
    Json(serde_json::json!({"status": "ok"})).into_response()
}

async fn not_found() -> Response {
    (StatusCode::NOT_FOUND, Json(ErrorResponse { error: "not found".into() })).into_response()
}

/// Query the status of a single order by hash.
#[instrument(skip(tracker), fields(%order_hash))]
async fn order_status(
    State(tracker): State<Arc<Tracker>>,
    Path(order_hash): Path<B256>,
) -> Response {
    let start = Instant::now();
    let response = match tracker.status(order_hash).await {
        Ok(report) => {
            metrics::record_request("success");
            Json(report).into_response()
        }
        Err(signet_tracker::Error::OrderNotFound(_)) => {
            metrics::record_request("not-found");
            metrics::record_request_error("not-found");
            (
                StatusCode::NOT_FOUND,
                Json(ErrorResponse { error: "order not found in tx-cache".into() }),
            )
                .into_response()
        }
        Err(err) => {
            error!(%err, "failed to query order status");
            metrics::record_request("error");
            metrics::record_request_error("internal");
            (StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse { error: err.to_string() }))
                .into_response()
        }
    };
    metrics::record_request_duration(start.elapsed());
    response
}

/// Serve the tracker HTTP API until cancelled or failure.
///
/// Returns `Ok(())` on graceful cancellation or an error if the server exits unexpectedly.
pub async fn serve_tracker(
    tracker: Tracker,
    port: u16,
    cancellation_token: CancellationToken,
) -> Result<()> {
    std::sync::LazyLock::force(&metrics::DESCRIPTIONS);
    let handle = do_serve(tracker, port, cancellation_token.clone());
    let result = handle.await;
    if cancellation_token.is_cancelled() {
        return Ok(());
    }
    cancellation_token.cancel();
    match result {
        Ok(Ok(())) => bail!("tracker server exited without cancellation"),
        Ok(error) => error,
        Err(error) if error.is_panic() => {
            Err(Report::new(error).wrap_err("panic in tracker server"))
        }
        Err(_) => bail!("tracker server task cancelled unexpectedly"),
    }
}

fn do_serve(
    tracker: Tracker,
    port: u16,
    cancel_token: CancellationToken,
) -> JoinHandle<Result<()>> {
    let shared_tracker = Arc::new(tracker);
    let router = Router::new()
        .route("/healthcheck", get(healthcheck))
        .route("/orders/{order_hash}", get(order_status))
        .fallback(not_found)
        .with_state(shared_tracker);
    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    tokio::spawn(async move {
        let listener = TcpListener::bind(addr)
            .await
            .wrap_err_with(|| format!("failed to bind tracker server on port {port}"))?;
        axum::serve(listener, router)
            .with_graceful_shutdown(async move {
                cancel_token.cancelled().await;
                debug!("tracker server cancelled");
            })
            .await
            .wrap_err("failed serving tracker")
    })
}
