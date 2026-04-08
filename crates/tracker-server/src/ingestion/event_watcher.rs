use crate::{
    handle_task_exit,
    state::event_store::{FilledEvent, OrderEvent},
};
use alloy::{
    primitives::{Address, Log},
    providers::Provider,
    rpc::types::Filter,
    sol_types::SolEvent,
};
use eyre::{Context, bail};
use signet_tracker::Chain;
use signet_zenith::RollupOrders;
use tokio::{
    sync::{broadcast::error::RecvError, mpsc},
    task::JoinHandle,
};
use tokio_util::sync::CancellationToken;
use tracing::{debug, trace, warn};

/// An event received from the rollup chain.
#[derive(Debug, Clone)]
pub(crate) enum ChainEvent {
    /// A `Filled` event from the rollup.
    Filled(FilledEvent),
    /// An `Order` (initiation) event from the rollup.
    Order(OrderEvent),
}

/// Subscribes to `Filled` and `Order` log events on the rollup chain and forwards them via an mpsc
/// channel.
#[derive(Debug)]
pub(crate) struct EventWatcher<P> {
    provider: P,
    orders_contract: Address,
    event_sender: mpsc::Sender<ChainEvent>,
    cancellation_token: CancellationToken,
}

impl<P> EventWatcher<P>
where
    P: Provider + Clone + 'static,
{
    /// Create a new event watcher for the rollup chain.
    pub(crate) const fn new(
        provider: P,
        orders_contract: Address,
        event_sender: mpsc::Sender<ChainEvent>,
        cancellation_token: CancellationToken,
    ) -> Self {
        Self { provider, orders_contract, event_sender, cancellation_token }
    }

    /// Spawn the event watcher task.
    pub(crate) fn spawn(self) -> JoinHandle<eyre::Result<()>> {
        let filter = Filter::new()
            .address(self.orders_contract)
            .events([RollupOrders::Filled::SIGNATURE_HASH, RollupOrders::Order::SIGNATURE_HASH]);

        tokio::spawn(watch_events(
            self.provider,
            filter,
            self.event_sender,
            self.cancellation_token,
        ))
    }
}

async fn watch_events<P: Provider>(
    provider: P,
    filter: Filter,
    event_sender: mpsc::Sender<ChainEvent>,
    cancellation_token: CancellationToken,
) -> eyre::Result<()> {
    let result = async {
        let mut sub = provider
            .subscribe_logs(&filter)
            .await
            .wrap_err("failed to subscribe to rollup log events")?;

        debug!("subscribed to rollup log events");

        loop {
            tokio::select! {
                biased;
                _ = cancellation_token.cancelled() => return Ok(()),
                result = sub.recv() => {
                    match result {
                        Ok(log) => {
                            let Some(event) = decode_log(&log) else {
                                continue;
                            };
                            trace!(?event, "received rollup event");
                            event_sender
                                .send(event)
                                .await
                                .wrap_err("rollup event channel closed")?;
                        }
                        Err(RecvError::Lagged(missed)) => {
                            warn!(%missed, "rollup log subscription receiver lagged");
                        }
                        Err(RecvError::Closed) => bail!("rollup log subscription closed"),
                    }
                }
            }
        }
    }
    .await;

    handle_task_exit("rollup_event_watcher", result, &cancellation_token)
}

/// Decode a raw log into a [`ChainEvent`], if it matches a known event signature.
fn decode_log(log: &alloy::rpc::types::Log) -> Option<ChainEvent> {
    let tx_hash = log.transaction_hash?;
    let block_number = log.block_number?;

    let inner = Log::new_unchecked(log.address(), log.topics().to_vec(), log.data().data.clone());

    if let Ok(decoded) = RollupOrders::Filled::decode_log(&inner) {
        return Some(ChainEvent::Filled(FilledEvent {
            chain: Chain::Rollup,
            block_number,
            tx_hash,
            outputs: decoded.data.outputs,
        }));
    }

    match RollupOrders::Order::decode_log(&inner) {
        Ok(decoded) => Some(ChainEvent::Order(OrderEvent::new(
            block_number,
            tx_hash,
            decoded.deadline.to(),
            &decoded.data.outputs,
        ))),
        Err(error) => {
            warn!(%tx_hash, block_number, %error, "failed to decode rollup log event");
            None
        }
    }
}
