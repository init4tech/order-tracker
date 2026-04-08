//! HTTP/WebSocket API server for Signet order tracking.
//!
//! Provides endpoints for subscribing to individual order status updates (via WebSocket or SSE) and
//! for following all orders (or a filtered subset) via a persistent WebSocket stream.

#![cfg_attr(not(test), warn(unused_crate_dependencies))]
#![cfg_attr(docsrs, feature(doc_cfg, doc_auto_cfg))]

/// Configuration for the tracker server.
pub mod config;

pub(crate) mod metrics;

/// Background chain event ingestion.
pub(crate) mod ingestion;

/// HTTP server and route handlers.
pub mod service;

/// In-memory order state and event tracking.
pub(crate) mod state;

/// WebSocket subscription types.
pub(crate) mod ws;

// Suppress unused crate dependency warnings for crates used only by the binary.
use git_version as _;

use crate::{
    ingestion::{block_watcher::BlockWatcher, event_watcher::EventWatcher, order_discovery},
    service::AppState,
    state::state_manager::StateManager,
};
use eyre::{Result, WrapErr, bail, eyre};
use init4_bin_base::deps::tracing::{debug, info};
use std::{collections::HashSet, pin::Pin, sync::Arc, time::Duration};
use tokio::{
    select,
    signal::unix::{SignalKind, signal},
    sync::{broadcast, mpsc, watch},
    task::{JoinError, JoinHandle},
};
use tokio_util::sync::CancellationToken;
use tracing::{error, warn};

/// Handles for all spawned background tasks. Awaiting this completes when any task exits,
/// returning `Ok(())` for graceful shutdown or `Err` if a task failed.
#[expect(missing_debug_implementations, reason = "contains JoinHandles")]
pub struct TasksJoinHandles {
    server: JoinHandle<Result<()>>,
    block_watcher: JoinHandle<Result<()>>,
    event_watcher: JoinHandle<Result<()>>,
    order_discovery: JoinHandle<Result<()>>,
    state_manager: JoinHandle<Result<()>>,
}

impl IntoFuture for TasksJoinHandles {
    type Output = Result<()>;
    type IntoFuture = Pin<Box<dyn Future<Output = Result<()>> + Send>>;

    fn into_future(self) -> Self::IntoFuture {
        Box::pin(async move {
            let results = [
                flatten_task_result("server", self.server.await),
                flatten_task_result("block_watcher", self.block_watcher.await),
                flatten_task_result("event_watcher", self.event_watcher.await),
                flatten_task_result("order_discovery", self.order_discovery.await),
                flatten_task_result("state_manager", self.state_manager.await),
            ];
            results.into_iter().find(Result::is_err).unwrap_or(Ok(()))
        })
    }
}

fn flatten_task_result(task_name: &str, result: Result<Result<()>, JoinError>) -> Result<()> {
    let inner_result = result.map_err(|join_error| {
        error!(task = task_name, %join_error, "task panicked");
        eyre!("{task_name} panicked: {join_error}")
    })?;
    match &inner_result {
        Ok(()) => info!(task = task_name, "task exited cleanly"),
        Err(error) => warn!(task = task_name, %error, "task failed"),
    }
    inner_result
}

/// Load the tracker configuration from the environment and initialize tracing and metrics.
pub fn config_from_env() -> Result<init4_bin_base::ConfigAndGuard<config::Config>> {
    init4_bin_base::init::<config::Config>()
        .map_err(|error| eyre::eyre!("{error}"))
        .wrap_err("failed to load tracker config (run with '--help' to see required env vars)")
}

/// Register signal handlers for graceful shutdown, returning a [`CancellationToken`] that is
/// cancelled on SIGINT or SIGTERM.
pub fn handle_signals() -> Result<CancellationToken> {
    let cancellation_token = CancellationToken::new();

    let mut sigint =
        signal(SignalKind::interrupt()).wrap_err("failed to register SIGINT handler")?;
    let mut sigterm =
        signal(SignalKind::terminate()).wrap_err("failed to register SIGTERM handler")?;

    tokio::spawn({
        let cancel_token = cancellation_token.clone();
        async move {
            select! {
                _ = sigint.recv() => {
                    info!("received SIGINT, shutting down");
                }
                _ = sigterm.recv() => {
                    info!("received SIGTERM, shutting down");
                }
            }
            cancel_token.cancel();
        }
    });

    debug!("ready to handle SIGINT or SIGTERM");
    Ok(cancellation_token)
}

/// Connect all providers, spawn all background tasks (including the HTTP/WS server), and return
/// handles that can be awaited for completion.
pub async fn run(
    config: &config::Config,
    cancellation_token: CancellationToken,
) -> Result<Option<TasksJoinHandles>> {
    // Connect all providers and caches concurrently, but bail immediately on shutdown so that
    // SIGINT/SIGTERM during startup (e.g. while retrying provider connections) kills the process.
    let (tracker, ru_ws_provider, host_provider, tx_cache, get_tracker) = select! {
        result = async {
            tokio::try_join!(
                config.connect_tracker(),
                config.connect_ru_provider(),
                config.connect_host_provider(),
                config.connect_tx_cache(),
                config.connect_tracker(),
            )
        } => result?,
        _ = cancellation_token.cancelled() => return Ok(None),
    };
    let constants = config.constants().clone();

    // Create channels.
    //
    // Chain events (Filled/Order) from both event watchers. 1024 gives the state manager headroom
    // to absorb bursts when a block contains many events without back-pressuring the watchers.
    let (event_sender, event_receiver) = mpsc::channel(1024);
    // Newly discovered orders from tx-cache polling. Each poll cycle may surface several orders,
    // but is bounded by the cache size and the 1s poll interval; 256 absorbs a full cycle.
    let (order_sender, order_receiver) = mpsc::channel(256);
    // Track requests from WS single-order handlers. Each connection sends exactly one request, so
    // this only needs to cover concurrent WS upgrades; 64 is ample.
    let (track_request_sender, track_request_receiver) = mpsc::channel(64);
    // Snapshot requests from WS all-orders handlers. Each connection sends one request on connect.
    let (snapshot_request_sender, snapshot_request_receiver) = mpsc::channel(64);
    // Status updates broadcast to all WS subscribers. Sized to match the event channel since each
    // event produces at most one update; slow consumers that fall behind will see a Lagged error.
    let (update_sender, _initial_receiver) = broadcast::channel(1024);
    let (all_order_hashes_sender, all_order_hashes_receiver) = watch::channel(HashSet::new());

    // Spawn the block watcher. Subscribes to rollup blocks via WS, polls host via HTTP.
    let block_watcher = BlockWatcher::new(
        ru_ws_provider.clone(),
        host_provider.clone(),
        cancellation_token.clone(),
    )
    .await
    .wrap_err("failed to start block watcher")?;
    let block_watcher_spawned = block_watcher.spawn();

    // Spawn rollup event watcher.
    let event_watcher = EventWatcher::new(
        ru_ws_provider,
        constants.rollup().orders(),
        event_sender,
        cancellation_token.clone(),
    );
    let event_watcher_join_handle = event_watcher.spawn();

    // Spawn order discovery.
    let order_discovery_join_handle = tokio::spawn(order_discovery::run(
        tx_cache.clone(),
        order_sender,
        all_order_hashes_sender,
        Duration::from_secs(1),
        cancellation_token.clone(),
    ));

    // Spawn state manager.
    let state_manager_task = StateManager::new(
        tracker,
        block_watcher_spawned.block_numbers,
        event_receiver,
        order_receiver,
        all_order_hashes_receiver,
        track_request_receiver,
        snapshot_request_receiver,
        update_sender.clone(),
        300, // ~1 hour at 12s blocks
        cancellation_token.clone(),
    );
    let state_manager_join_handle = tokio::spawn(state_manager_task.run());

    let app_state = Arc::new(AppState {
        tracker: get_tracker,
        track_request_sender,
        snapshot_request_sender,
        update_sender,
        tx_cache,
    });

    // Spawn the HTTP/WS server.
    let port = config.port();
    let server_cancel = cancellation_token.clone();
    let server =
        tokio::spawn(async move { service::serve_tracker(app_state, port, server_cancel).await });

    Ok(Some(TasksJoinHandles {
        server,
        block_watcher: block_watcher_spawned.join_handle,
        event_watcher: event_watcher_join_handle,
        order_discovery: order_discovery_join_handle,
        state_manager: state_manager_join_handle,
    }))
}

/// Normalize a background task's exit into a consistent `Result`.
///
/// - If the cancellation token is cancelled, the task is considered to have exited normally
///   regardless of the inner result → returns `Ok(())`.
/// - If not cancelled and the inner result is `Err`, the token is cancelled and the error is
///   propagated.
/// - If not cancelled and the inner result is `Ok`, that indicates a bug (the task exited without
///   cancellation or error). The token is cancelled and an error is returned.
pub(crate) fn handle_task_exit(
    task_name: &str,
    result: Result<()>,
    cancellation_token: &CancellationToken,
) -> Result<()> {
    if cancellation_token.is_cancelled() {
        return Ok(());
    }
    cancellation_token.cancel();
    let Err(error) = result else {
        error!(task = task_name, "task exited unexpectedly without error or cancellation");
        bail!("{task_name} exited unexpectedly");
    };
    warn!(task = task_name, %error, "task failed");
    Err(error)
}
