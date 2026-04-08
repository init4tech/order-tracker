use crate::{
    config::Tracker,
    handle_task_exit,
    ingestion::block_watcher::BlockTip,
    ingestion::{block_watcher::BlockNumbers, event_watcher::ChainEvent},
    state::{
        event_store::{EventStore, FilledEvent},
        tracked_order::TrackedOrder,
    },
};
use alloy::primitives::B256;
use eyre::eyre;
use signet_tracker::{FillInfo, FillOutput, OrderStatus};
use signet_types::SignedOrder;
use std::collections::{HashMap, HashSet};
use tokio::sync::{broadcast, mpsc, oneshot, watch};
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, instrument, warn};

/// Request to register an order for tracking. Optionally returns an initial report via a oneshot
/// channel.
#[derive(Debug)]
pub(crate) struct TrackOrderRequest {
    /// The signed order to track.
    pub(crate) order: SignedOrder,
    /// If set, the initial [`OrderStatus`] is sent back on this channel.
    pub(crate) initial_report_sender: Option<oneshot::Sender<OrderStatus>>,
}

/// A oneshot channel for requesting a snapshot of all currently tracked order statuses.
pub(crate) type SnapshotRequest = oneshot::Sender<Vec<OrderStatus>>;

/// The central state manager. Processes chain events, tracks order lifecycle, and broadcasts
/// status updates.
///
/// Owns all mutable state and runs as a single tokio task.
pub(crate) struct StateManager {
    event_store: EventStore,
    tracked_orders: HashMap<B256, TrackedOrder>,
    tracker: Tracker,
    block_numbers: BlockNumbers,
    event_receiver: mpsc::Receiver<ChainEvent>,
    order_receiver: mpsc::Receiver<SignedOrder>,
    all_order_hashes_receiver: watch::Receiver<HashSet<B256>>,
    track_request_receiver: mpsc::Receiver<TrackOrderRequest>,
    snapshot_request_receiver: mpsc::Receiver<SnapshotRequest>,
    update_sender: broadcast::Sender<OrderStatus>,
    cancellation_token: CancellationToken,
}

impl StateManager {
    /// Create a new state manager.
    #[expect(clippy::too_many_arguments, reason = "startup wiring requires all channel handles")]
    pub(crate) fn new(
        tracker: Tracker,
        block_numbers: BlockNumbers,
        event_receiver: mpsc::Receiver<ChainEvent>,
        order_receiver: mpsc::Receiver<SignedOrder>,
        all_order_hashes_receiver: watch::Receiver<HashSet<B256>>,
        track_request_receiver: mpsc::Receiver<TrackOrderRequest>,
        snapshot_request_receiver: mpsc::Receiver<SnapshotRequest>,
        update_sender: broadcast::Sender<OrderStatus>,
        retention_blocks: u64,
        cancellation_token: CancellationToken,
    ) -> Self {
        Self {
            event_store: EventStore::new(retention_blocks),
            tracked_orders: HashMap::new(),
            tracker,
            block_numbers,
            event_receiver,
            order_receiver,
            all_order_hashes_receiver,
            track_request_receiver,
            snapshot_request_receiver,
            update_sender,
            cancellation_token,
        }
    }

    /// Run the state manager loop until cancelled or an input channel closes.
    pub(crate) async fn run(mut self) -> eyre::Result<()> {
        let cancellation_token = self.cancellation_token.clone();
        let result = self.run_inner().await;
        handle_task_exit("state_manager", result, &cancellation_token)
    }

    async fn run_inner(&mut self) -> eyre::Result<()> {
        debug!(
            rollup_tip = self.block_numbers.rollup(),
            host_tip = self.block_numbers.host(),
            "state manager started"
        );

        loop {
            tokio::select! {
                biased;
                _ = self.cancellation_token.cancelled() => return Ok(()),
                result = self.block_numbers.changed() => {
                    let tip = result.map_err(|_| eyre!("block number channel closed"))?;
                    self.handle_new_blocks(tip).await;
                }
                event = self.event_receiver.recv() => {
                    let event = event.ok_or_else(|| eyre!("event channel closed"))?;
                    self.handle_chain_event(event).await;
                }
                order = self.order_receiver.recv() => {
                    let order = order.ok_or_else(|| eyre!("order discovery channel closed"))?;
                    self.register_order(order, None).await;
                }
                request = self.track_request_receiver.recv() => {
                    let request = request.ok_or_else(|| eyre!("track request channel closed"))?;
                    self.register_order(request.order, request.initial_report_sender).await;
                }
                request = self.snapshot_request_receiver.recv() => {
                    let request = request.ok_or_else(|| eyre!("snapshot request channel closed"))?;
                    let statuses: Vec<OrderStatus> = self.tracked_orders.values().map(|tracked| tracked.status().clone()).collect();
                    debug!(count = statuses.len(), "sending snapshot to all-orders client");
                    let _ = request.send(statuses);
                }
            }
        }
    }

    /// Handle new blocks on both chains. Re-runs diagnostics for all pending orders, detecting
    /// deadline expiry and nonce consumption (missed fill events). Evicts terminal orders afterward.
    async fn handle_new_blocks(&mut self, tip: BlockTip) {
        self.event_store.update_tips(tip);

        let pending_hashes: Vec<B256> = self
            .tracked_orders
            .iter()
            .filter(|(_, tracked)| !tracked.is_terminal())
            .map(|(hash, _)| *hash)
            .collect();

        let all_order_hashes = self.all_order_hashes_receiver.borrow().clone();

        for hash in pending_hashes {
            let Some(tracked) = self.tracked_orders.get(&hash) else { continue };
            let order = tracked.order().clone();
            let is_in_cache = all_order_hashes.contains(&hash);

            let new_status = self.tracker.status_for_order(&order, is_in_cache).await;

            let status_changed =
                std::mem::discriminant(tracked.status()) != std::mem::discriminant(&new_status);

            if status_changed {
                info!(order_hash = %hash, status = %status_label(&new_status), "order status changed");
            }

            if let Some(tracked) = self.tracked_orders.get_mut(&hash) {
                tracked.set_status(new_status.clone());
            }

            // Broadcast on status change, or always for pending (diagnostics may have changed).
            if status_changed || matches!(new_status, OrderStatus::Pending { .. }) {
                let _ = self.update_sender.send(new_status);
            }
        }

        // Evict terminal orders - their final status has already been broadcast.
        let before = self.tracked_orders.len();
        self.tracked_orders.retain(|_, tracked| !tracked.is_terminal());
        let evicted = before - self.tracked_orders.len();
        if evicted > 0 {
            debug!(evicted, remaining = self.tracked_orders.len(), "evicted terminal orders");
        }
    }

    /// Handle a chain event (Filled or Order).
    async fn handle_chain_event(&mut self, event: ChainEvent) {
        match event {
            ChainEvent::Filled(filled) => {
                crate::metrics::record_chain_event(crate::metrics::ChainEventKind::Filled);
                self.event_store.insert_filled(filled.clone());
                self.check_fills_against_pending(&filled).await;
            }
            ChainEvent::Order(order_event) => {
                crate::metrics::record_chain_event(crate::metrics::ChainEventKind::Order);
                self.event_store.insert_order(order_event);
            }
        }
        self.update_store_metrics();
    }

    /// Check a newly received Filled event against all pending orders. Verifies the Permit2 nonce
    /// is consumed before confirming the fill — if the nonce is not consumed, the event matched a
    /// different order with identical outputs.
    async fn check_fills_against_pending(&mut self, filled: &FilledEvent) {
        let candidate_hashes: Vec<B256> = self
            .tracked_orders
            .iter()
            .filter(|(_, tracked)| !tracked.is_terminal())
            .filter(|(_, tracked)| {
                signet_tracker::fill_outputs_are_superset_of_order_outputs(
                    &filled.outputs,
                    tracked.order().outputs(),
                )
            })
            .map(|(hash, _)| *hash)
            .collect();

        for hash in candidate_hashes {
            let Some(tracked) = self.tracked_orders.get(&hash) else { continue };

            // Verify the nonce is actually consumed — if not, the fill matched a different order.
            match self.tracker.is_nonce_consumed(tracked.order()).await {
                Ok(true) => {
                    info!(order_hash = %hash, "order filled (via event)");
                    let fill_info = build_fill_info(&self.event_store, filled, tracked);
                    let status =
                        OrderStatus::Filled { order_hash: hash, fill_info: Some(fill_info) };
                    if let Some(tracked) = self.tracked_orders.get_mut(&hash) {
                        tracked.set_status(status.clone());
                    }
                    let _ = self.update_sender.send(status);
                }
                Ok(false) => {
                    debug!(
                        order_hash = %hash,
                        "Filled event output match but nonce not consumed — different order"
                    );
                }
                Err(error) => {
                    warn!(
                        order_hash = %hash,
                        "failed to verify nonce after Filled event match: {error:#}"
                    );
                }
            }
        }
    }

    /// Register a new order for tracking. Runs full diagnostics via the tracker to produce the
    /// initial status, then tracks the order for event-driven updates.
    #[instrument(skip_all, fields(order_hash = %order.order_hash()))]
    async fn register_order(
        &mut self,
        order: SignedOrder,
        initial_status_sender: Option<oneshot::Sender<OrderStatus>>,
    ) {
        let order_hash = *order.order_hash();

        // Skip if already tracked.
        if self.tracked_orders.contains_key(&order_hash) {
            if let Some(sender) = initial_status_sender
                && let Some(tracked) = self.tracked_orders.get(&order_hash)
            {
                let _ = sender.send(tracked.status().clone());
            }
            return;
        }

        // Run full diagnostics for the initial status.
        let status = self.tracker.status_for_order(&order, true).await;
        info!(order_hash = %order_hash, status = %status_label(&status), "tracking new order");
        let tracked = TrackedOrder::new(order, status.clone());
        self.tracked_orders.insert(order_hash, tracked);

        if let Some(sender) = initial_status_sender {
            let _ = sender.send(status.clone());
        }

        let _ = self.update_sender.send(status);
        self.update_store_metrics();
    }

    fn update_store_metrics(&self) {
        crate::metrics::set_tracked_orders(self.tracked_orders.len());
        crate::metrics::set_event_store_filled(self.event_store.filled_count());
        crate::metrics::set_event_store_order(self.event_store.order_count());
    }
}

const fn status_label(status: &OrderStatus) -> &'static str {
    match status {
        OrderStatus::Pending { .. } => "pending",
        OrderStatus::Filled { .. } => "filled",
        OrderStatus::Expired { .. } => "expired",
    }
}

/// Build a [`FillInfo`] from a stored event and tracked order.
fn build_fill_info(
    event_store: &EventStore,
    filled: &FilledEvent,
    tracked: &TrackedOrder,
) -> FillInfo {
    let deadline: u64 = tracked.order().permit().permit.deadline.to();
    let rollup_initiation_tx = event_store.find_order_tx(deadline, tracked.order().outputs());

    FillInfo {
        block_number: filled.block_number,
        rollup_initiation_tx,
        fill_tx: Some(signet_tracker::ChainTransaction {
            chain: filled.chain,
            tx_hash: filled.tx_hash,
        }),
        outputs: filled
            .outputs
            .iter()
            .map(|output| FillOutput {
                token_contract: output.token,
                token_symbol: String::new(),
                amount: output.amount.into(),
                recipient: output.recipient,
                chain: filled.chain,
            })
            .collect(),
    }
}
