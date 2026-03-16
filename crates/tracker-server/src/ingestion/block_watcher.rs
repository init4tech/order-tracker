use crate::handle_task_exit;
use alloy::providers::Provider;
use eyre::{Context, bail};
use signet_tracker::Chain;
use std::time::Duration;
use tokio::time::MissedTickBehavior;
use tokio::{
    sync::{broadcast::error::RecvError, watch},
    task::JoinHandle,
};
use tokio_util::sync::CancellationToken;
use tracing::{debug, trace, warn};

/// The latest block numbers observed on both chains.
#[derive(Debug, Clone, Copy)]
pub(crate) struct BlockTip {
    /// The latest rollup block number.
    pub(crate) rollup: u64,
    /// The latest host block number.
    pub(crate) host: u64,
}

/// Subscribes to new block headers on the rollup chain via WebSocket, then polls the host chain
/// until it too has advanced, ensuring both tips are up to date before notifying consumers.
#[derive(Debug)]
pub(crate) struct BlockWatcher<RuP, HostP> {
    rollup_provider: RuP,
    host_provider: HostP,
    sender: watch::Sender<BlockTip>,
    cancellation_token: CancellationToken,
}

impl<RuP, HostP> BlockWatcher<RuP, HostP>
where
    RuP: Provider + Clone + 'static,
    HostP: Provider + Clone + 'static,
{
    /// Create a new block watcher, fetching the current block number from each chain.
    pub(crate) async fn new(
        rollup_provider: RuP,
        host_provider: HostP,
        cancellation_token: CancellationToken,
    ) -> eyre::Result<Self> {
        let (rollup_tip, host_tip) =
            tokio::try_join!(rollup_provider.get_block_number(), host_provider.get_block_number(),)
                .wrap_err("failed to fetch initial block numbers")?;

        Ok(Self {
            rollup_provider,
            host_provider,
            sender: watch::channel(BlockTip { rollup: rollup_tip, host: host_tip }).0,
            cancellation_token,
        })
    }

    /// Subscribe to block tip updates.
    fn subscribe(&self) -> BlockNumbers {
        BlockNumbers { receiver: self.sender.subscribe() }
    }

    /// Spawn the block watcher task.
    pub(crate) fn spawn(self) -> SpawnResult {
        let block_numbers = self.subscribe();
        let join_handle = tokio::spawn(watch_blocks(
            self.rollup_provider,
            self.host_provider,
            self.sender,
            self.cancellation_token,
        ));
        SpawnResult { block_numbers, join_handle }
    }
}

/// Result of spawning the block watcher task.
pub(crate) struct SpawnResult {
    pub(crate) block_numbers: BlockNumbers,
    pub(crate) join_handle: JoinHandle<eyre::Result<()>>,
}

/// Block number receiver for both chains.
#[derive(Debug, Clone)]
pub(crate) struct BlockNumbers {
    receiver: watch::Receiver<BlockTip>,
}

impl BlockNumbers {
    /// Get the current rollup block number.
    pub(crate) fn rollup(&self) -> u64 {
        self.receiver.borrow().rollup
    }

    /// Get the current host block number.
    pub(crate) fn host(&self) -> u64 {
        self.receiver.borrow().host
    }

    /// Wait for both chains to advance. Returns the new block tip once both the rollup (via WS
    /// subscription) and host (via polling) have produced new blocks.
    pub(crate) async fn changed(&mut self) -> Result<BlockTip, watch::error::RecvError> {
        self.receiver.changed().await?;
        Ok(*self.receiver.borrow_and_update())
    }
}

async fn watch_blocks<RuP: Provider, HostP: Provider>(
    rollup_provider: RuP,
    host_provider: HostP,
    sender: watch::Sender<BlockTip>,
    cancellation_token: CancellationToken,
) -> eyre::Result<()> {
    let result = async {
        let mut sub = rollup_provider
            .subscribe_blocks()
            .await
            .wrap_err("failed to subscribe to rollup blocks")?;

        debug!("subscribed to rollup block headers");

        loop {
            tokio::select! {
                biased;
                _ = cancellation_token.cancelled() => return Ok(()),
                result = sub.recv() => {
                    match result {
                        Ok(header) => {
                            let rollup_block = header.number;
                            crate::metrics::record_block_received(Chain::Rollup);
                            trace!(rollup_block, "new rollup block");

                            let previous_host = sender.borrow().host;
                            let host_block = poll_host_tip(
                                &host_provider,
                                previous_host,
                                &cancellation_token,
                            )
                            .await?;
                            crate::metrics::record_block_received(Chain::Host);
                            trace!(host_block, "host tip updated");

                            sender.send_replace(BlockTip { rollup: rollup_block, host: host_block });
                        }
                        Err(RecvError::Lagged(missed)) => {
                            warn!(%missed, "rollup block subscription receiver lagged");
                        }
                        Err(RecvError::Closed) => bail!("rollup block subscription closed"),
                    }
                }
            }
        }
    }
    .await;

    handle_task_exit("block_watcher", result, &cancellation_token)
}

/// Poll the host chain's block number until it advances past `previous_tip`.
async fn poll_host_tip<P: Provider>(
    provider: &P,
    previous_tip: u64,
    cancellation_token: &CancellationToken,
) -> eyre::Result<u64> {
    let mut interval = tokio::time::interval(Duration::from_millis(250));
    interval.set_missed_tick_behavior(MissedTickBehavior::Skip);

    loop {
        tokio::select! {
            _ = cancellation_token.cancelled() => return Ok(previous_tip),
            _ = interval.tick() => {
                let tip = provider
                    .get_block_number()
                    .await
                    .wrap_err("failed to poll host block number")?;
                if tip > previous_tip {
                    return Ok(tip);
                }
            }
        }
    }
}
