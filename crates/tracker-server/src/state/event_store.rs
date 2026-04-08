use crate::ingestion::block_watcher::BlockTip;
use alloy::primitives::{B256, keccak256};
use alloy::sol_types::SolValue;
use signet_tracker::Chain;
use signet_zenith::RollupOrders;
use std::collections::{BTreeMap, HashMap};

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
    /// Hash of deadline + outputs, uniquely identifying this order.
    key: B256,
    /// The rollup block containing the event.
    block_number: u64,
    /// The transaction hash of the `initiatePermit2` call.
    tx_hash: B256,
}

impl OrderEvent {
    /// Create a new order event, computing the key from the deadline and outputs.
    pub(crate) fn new(
        block_number: u64,
        tx_hash: B256,
        deadline: u64,
        outputs: &[RollupOrders::Output],
    ) -> Self {
        let key = order_event_key(deadline, outputs);
        Self { key, block_number, tx_hash }
    }
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
    /// Order initiation events (rollup-only), keyed by hash of deadline + outputs.
    /// Values are `(block_number, tx_hash)`.
    order_events: HashMap<B256, (u64, B256)>,
    retention_blocks: u64,
}

impl EventStore {
    /// Create a new event store with the given retention window in blocks.
    pub(crate) fn new(retention_blocks: u64) -> Self {
        Self {
            rollup: ChainEvents::new(),
            host: ChainEvents::new(),
            order_events: HashMap::new(),
            retention_blocks,
        }
    }

    /// Update the tips for both chains and prune old events.
    pub(crate) fn update_tips(&mut self, tip: BlockTip) {
        self.rollup.tip = tip.rollup;
        self.rollup.prune(self.retention_blocks);
        let cutoff = self.rollup.tip.saturating_sub(self.retention_blocks);
        self.order_events.retain(|_, (block_number, _)| *block_number >= cutoff);

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
        self.order_events.insert(event.key, (event.block_number, event.tx_hash));
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

    /// Find the initiation tx hash for an order identified by its deadline and outputs.
    pub(crate) fn find_order_tx(
        &self,
        deadline: u64,
        outputs: &[RollupOrders::Output],
    ) -> Option<B256> {
        let key = order_event_key(deadline, outputs);
        self.order_events.get(&key).map(|(_, tx_hash)| *tx_hash)
    }

    /// Total number of stored filled events across both chains.
    pub(crate) fn filled_count(&self) -> usize {
        self.rollup.filled_count() + self.host.filled_count()
    }

    /// Total number of stored order events.
    pub(crate) fn order_count(&self) -> usize {
        self.order_events.len()
    }
}

fn order_event_key(deadline: u64, outputs: &[RollupOrders::Output]) -> B256 {
    let deadline_bytes = deadline.to_ne_bytes();
    let mut data = Vec::with_capacity(deadline_bytes.len() + outputs.abi_encoded_size());
    data.extend_from_slice(&deadline_bytes);
    data.extend(outputs.abi_encode());
    keccak256(&data)
}
