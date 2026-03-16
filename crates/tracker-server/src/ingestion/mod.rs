/// Subscribes to new block headers on both chains.
pub(crate) mod block_watcher;
/// Subscribes to `Filled` and `Order` log events on both chains.
pub(crate) mod event_watcher;
/// Polls the tx-cache for new orders.
pub(crate) mod order_discovery;
