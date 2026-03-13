//! HTTP/WebSocket API server for Signet order tracking.
//!
//! Provides endpoints for subscribing to individual order status updates (via WebSocket or SSE) and
//! for following all orders (or a filtered subset) via a persistent WebSocket stream.

#![warn(
    missing_copy_implementations,
    missing_debug_implementations,
    missing_docs,
    unreachable_pub,
    clippy::missing_const_for_fn,
    rustdoc::all
)]
#![cfg_attr(not(test), warn(unused_crate_dependencies))]
#![deny(unused_must_use, rust_2018_idioms)]
#![cfg_attr(docsrs, feature(doc_cfg, doc_auto_cfg))]

use eyre::{Result, WrapErr};
use init4_bin_base::deps::tracing::{debug, info};
use tokio::{
    select,
    signal::unix::{SignalKind, signal},
};
use tokio_util::sync::CancellationToken;

/// Configuration for the tracker server.
pub mod config;

pub(crate) mod metrics;

/// HTTP server and route handlers.
pub mod service;

// Suppress unused crate dependency warnings for crates used only by the binary.
use git_version as _;

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
