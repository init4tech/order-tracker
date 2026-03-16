use crate::{Amount, order_diagnostics::OrderDiagnostics};
use alloy::primitives::{Address, B256};
use serde::Serialize;
use std::fmt::{self, Display, Formatter};

/// The resolved lifecycle status of a Signet order.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum OrderStatus {
    /// Order is pending: the Permit2 nonce has not been consumed, and the deadline has not passed.
    Pending {
        /// The order hash.
        order_hash: B256,
        /// Diagnostics for the order.
        #[serde(flatten)]
        diagnostics: OrderDiagnostics,
    },
    /// Order has been filled: the Permit2 nonce has been consumed on-chain.
    Filled {
        /// The order hash.
        order_hash: B256,
        /// Details about the fill transaction, if located. `None` if the fill event could not be
        /// correlated to a specific transaction.
        fill_info: Option<FillInfo>,
    },
    /// Order expired: the deadline has passed and the Permit2 nonce was not consumed.
    Expired {
        /// The order hash.
        order_hash: B256,
        /// Diagnostics for the order.
        #[serde(flatten)]
        diagnostics: OrderDiagnostics,
    },
}

impl OrderStatus {
    /// The order hash for this status.
    pub const fn order_hash(&self) -> B256 {
        match self {
            Self::Pending { order_hash, .. }
            | Self::Filled { order_hash, .. }
            | Self::Expired { order_hash, .. } => *order_hash,
        }
    }

    /// Whether this is a terminal state (filled or expired).
    pub const fn is_terminal(&self) -> bool {
        matches!(self, Self::Filled { .. } | Self::Expired { .. })
    }
}

/// Which chain a transaction was observed on.
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Chain {
    /// The Signet rollup chain.
    Rollup,
    /// The host (L1) chain.
    Host,
}

impl Chain {
    /// Returns the chain name as a string slice.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Rollup => "rollup",
            Self::Host => "host",
        }
    }
}

impl Display for Chain {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        self.as_str().fmt(f)
    }
}

/// A transaction hash with the chain it was observed on.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct ChainTransaction {
    /// Which chain the transaction is on.
    pub chain: Chain,
    /// The transaction hash.
    pub tx_hash: B256,
}

/// Information about a fill that matched an order.
#[derive(Debug, Clone, Serialize)]
pub struct FillInfo {
    /// The block number containing the fill.
    pub block_number: u64,
    /// The rollup transaction that initiated the order (from the `Order` event).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rollup_initiation_tx: Option<B256>,
    /// The transaction that emitted the `Filled` event.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fill_tx: Option<ChainTransaction>,
    /// The outputs delivered by the fill.
    pub outputs: Vec<FillOutput>,
}

/// A single output from a fill event.
#[derive(Debug, Clone, Serialize)]
pub struct FillOutput {
    /// The token contract address.
    pub token_contract: Address,
    /// Human-readable token symbol (e.g. "WETH"), or "unknown" if unresolvable.
    pub token_symbol: String,
    /// The amount delivered.
    pub amount: Amount,
    /// The recipient address.
    pub recipient: Address,
    /// The chain the output targets.
    pub chain: Chain,
}
