use signet_tracker::OrderStatus;
use signet_types::SignedOrder;

/// A tracked order with its current derived status.
#[derive(Debug, Clone)]
pub(crate) struct TrackedOrder {
    /// The signed order from the tx-cache.
    order: SignedOrder,
    /// Current lifecycle status (includes diagnostics for pending/expired variants).
    status: OrderStatus,
}

impl TrackedOrder {
    /// Create a new tracked order from a signed order and its initial status.
    pub(crate) const fn new(order: SignedOrder, status: OrderStatus) -> Self {
        Self { order, status }
    }

    /// The signed order.
    pub(crate) const fn order(&self) -> &SignedOrder {
        &self.order
    }

    /// Current lifecycle status.
    pub(crate) const fn status(&self) -> &OrderStatus {
        &self.status
    }

    /// Replace the current status.
    pub(crate) fn set_status(&mut self, status: OrderStatus) {
        self.status = status;
    }

    /// Whether the order is in a terminal state (filled or expired).
    pub(crate) const fn is_terminal(&self) -> bool {
        matches!(self.status, OrderStatus::Filled { .. } | OrderStatus::Expired { .. })
    }
}
