use crate::ingestion::block_watcher::BlockTip;
use alloy::primitives::B256;
use signet_tracker::Chain;
use signet_zenith::RollupOrders;
use std::collections::BTreeMap;

/// A `Filled` event observed on-chain.
#[derive(Debug, Clone)]
pub(crate) struct FilledEvent {
    /// Which chain the event was observed on.
    pub(crate) chain: Chain,
    /// The block containing the event (on the event's chain).
    pub(crate) block_number: u64,
    /// The transaction hash.
    pub(crate) tx_hash: B256,
    /// The outputs from the fill.
    pub(crate) outputs: Vec<RollupOrders::Output>,
}

/// An `Order` (initiation) event observed on the rollup.
#[derive(Debug, Clone, Copy)]
pub(crate) struct OrderEvent {
    /// The rollup block containing the event.
    pub(crate) block_number: u64,
    /// The transaction hash of the `initiatePermit2` call.
    pub(crate) tx_hash: B256,
    /// The order's deadline as a unix timestamp.
    pub(crate) deadline: u64,
}

/// Per-chain event storage with independent block tips and pruning.
#[derive(Debug)]
struct ChainEvents {
    filled: BTreeMap<u64, Vec<FilledEvent>>,
    tip: u64,
}

impl ChainEvents {
    const fn new() -> Self {
        Self { filled: BTreeMap::new(), tip: 0 }
    }

    fn insert_filled(&mut self, event: FilledEvent) {
        self.filled.entry(event.block_number).or_default().push(event);
    }

    fn prune(&mut self, retention_blocks: u64) {
        let cutoff = self.tip.saturating_sub(retention_blocks);
        self.filled = self.filled.split_off(&cutoff);
    }

    fn filled_count(&self) -> usize {
        self.filled.values().map(Vec::len).sum()
    }
}

/// In-memory store of recent chain events, pruned to a configurable retention window.
///
/// Filled events are stored per-chain since rollup and host have independent block numbering.
/// Order (initiation) events are rollup-only.
#[derive(Debug)]
pub(crate) struct EventStore {
    rollup: ChainEvents,
    host: ChainEvents,
    /// Order initiation events (rollup-only), keyed by rollup block number.
    order_events: BTreeMap<u64, Vec<OrderEvent>>,
    retention_blocks: u64,
}

impl EventStore {
    /// Create a new event store with the given retention window in blocks.
    pub(crate) const fn new(retention_blocks: u64) -> Self {
        Self {
            rollup: ChainEvents::new(),
            host: ChainEvents::new(),
            order_events: BTreeMap::new(),
            retention_blocks,
        }
    }

    /// Update the tips for both chains and prune old events.
    pub(crate) fn update_tips(&mut self, tip: BlockTip) {
        self.rollup.tip = tip.rollup;
        self.rollup.prune(self.retention_blocks);
        let cutoff = self.rollup.tip.saturating_sub(self.retention_blocks);
        self.order_events = self.order_events.split_off(&cutoff);

        self.host.tip = tip.host;
        self.host.prune(self.retention_blocks);
    }

    /// Insert a `Filled` event, routing to the correct chain's storage.
    pub(crate) fn insert_filled(&mut self, event: FilledEvent) {
        match event.chain {
            Chain::Rollup => self.rollup.insert_filled(event),
            Chain::Host => self.host.insert_filled(event),
        }
    }

    /// Insert an `Order` (initiation) event (always rollup).
    pub(crate) fn insert_order(&mut self, event: OrderEvent) {
        self.order_events.entry(event.block_number).or_default().push(event);
    }

    /// Find a `Filled` event whose outputs are a superset of the given expected outputs.
    ///
    /// Searches both chains, most recent block first.
    #[expect(dead_code, reason = "will be used for nonce-consumed-but-missed-event fallback")]
    pub(crate) fn find_matching_fill(
        &self,
        expected_outputs: &[RollupOrders::Output],
    ) -> Option<&FilledEvent> {
        // Search both chains — check all events from most recent blocks first.
        for chain_events in [&self.rollup, &self.host] {
            for events in chain_events.filled.values().rev() {
                for event in events {
                    if signet_tracker::fill_outputs_are_superset_of_order_outputs(
                        &event.outputs,
                        expected_outputs,
                    ) {
                        return Some(event);
                    }
                }
            }
        }
        None
    }

    /// Find the `Order` event matching the given deadline.
    pub(crate) fn find_order_by_deadline(&self, deadline: u64) -> Option<&OrderEvent> {
        for events in self.order_events.values().rev() {
            for event in events {
                if event.deadline == deadline {
                    return Some(event);
                }
            }
        }
        None
    }

    /// Total number of stored filled events across both chains.
    pub(crate) fn filled_count(&self) -> usize {
        self.rollup.filled_count() + self.host.filled_count()
    }

    /// Total number of stored order events.
    pub(crate) fn order_count(&self) -> usize {
        self.order_events.values().map(Vec::len).sum()
    }
}
