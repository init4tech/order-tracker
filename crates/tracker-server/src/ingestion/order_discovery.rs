use crate::handle_task_exit;
use alloy::primitives::B256;
use eyre::WrapErr;
use futures_util::TryStreamExt;
use signet_orders::OrderStreamExt;
use signet_tx_cache::TxCache;
use signet_types::SignedOrder;
use std::collections::HashSet;
use tokio::sync::{mpsc, watch};
use tokio_util::sync::CancellationToken;
use tracing::{debug, trace, warn};

/// Polls the tx-cache for new orders and forwards them to the state manager. Also maintains a
/// shared snapshot of which order hashes are currently in the cache.
pub(crate) async fn run(
    tx_cache: TxCache,
    order_sender: mpsc::Sender<SignedOrder>,
    all_order_hashes_sender: watch::Sender<HashSet<B256>>,
    poll_interval: std::time::Duration,
    cancellation_token: CancellationToken,
) -> eyre::Result<()> {
    let result = run_inner(
        &tx_cache,
        &order_sender,
        &all_order_hashes_sender,
        poll_interval,
        &cancellation_token,
    )
    .await;
    handle_task_exit("order_discovery", result, &cancellation_token)
}

async fn run_inner(
    tx_cache: &TxCache,
    order_sender: &mpsc::Sender<SignedOrder>,
    all_order_hashes_sender: &watch::Sender<HashSet<B256>>,
    poll_interval: std::time::Duration,
    cancellation_token: &CancellationToken,
) -> eyre::Result<()> {
    let mut seen_hashes: HashSet<B256> = HashSet::new();
    let mut interval = tokio::time::interval(poll_interval);
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    debug!(poll_interval_ms = poll_interval.as_millis(), "starting order discovery");

    loop {
        tokio::select! {
            biased;
            _ = cancellation_token.cancelled() => return Ok(()),
            _ = interval.tick() => {
                match poll_cache(tx_cache, &mut seen_hashes).await {
                    Ok((new_orders, current_hashes)) => {
                        all_order_hashes_sender.send_replace(current_hashes);
                        send_new_orders(order_sender, new_orders).await?;
                    }
                    Err(error) => warn!(%error, "failed to fetch orders from tx-cache"),
                }
            }
        }
    }
}

/// Fetch all orders from the tx-cache. Returns newly-seen orders and the full set of hashes
/// currently in the cache.
async fn poll_cache(
    tx_cache: &TxCache,
    seen_hashes: &mut HashSet<B256>,
) -> Result<(Vec<SignedOrder>, HashSet<B256>), signet_tx_cache::TxCacheError> {
    let mut current_hashes = HashSet::new();
    let new_orders: Vec<SignedOrder> = tx_cache
        .stream_orders()
        .filter_orders(|order| {
            let hash = *order.order_hash();
            current_hashes.insert(hash);
            seen_hashes.insert(hash)
        })
        .try_collect()
        .await?;

    // Prune hashes that are no longer in the tx-cache so the set stays bounded.
    seen_hashes.retain(|hash| current_hashes.contains(hash));

    if !new_orders.is_empty() {
        crate::metrics::record_orders_discovered(new_orders.len());
        debug!(count = new_orders.len(), "discovered new orders");
    }

    Ok((new_orders, current_hashes))
}

async fn send_new_orders(
    order_sender: &mpsc::Sender<SignedOrder>,
    orders: Vec<SignedOrder>,
) -> eyre::Result<()> {
    for order in orders {
        trace!(order_hash = %order.order_hash(), "discovered new order");
        order_sender.send(order).await.wrap_err("order channel closed")?;
    }
    Ok(())
}
