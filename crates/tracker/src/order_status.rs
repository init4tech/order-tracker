use crate::{Amount, order_diagnostics::OrderDiagnostics};
use alloy::primitives::{Address, B256};
use serde::Serialize;

/// Combined status and diagnostics for a tracked order.
#[derive(Debug, Clone, Serialize)]
pub struct OrderReport {
    /// The resolved lifecycle status.
    pub status: OrderStatus,
    /// Diagnostic details from all checks performed.
    pub diagnostics: OrderDiagnostics,
}

/// The resolved lifecycle status of a Signet order.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum OrderStatus {
    /// Order is pending: found in the transaction cache, the Permit2 nonce has not been consumed,
    /// and the deadline has not passed.
    Pending {
        /// Seconds remaining until the order's deadline.
        seconds_remaining: u64,
    },
    /// Order has been filled: the Permit2 nonce has been consumed on-chain.
    Filled {
        /// Details about the fill transaction, if located. `None` if the fill event could not be
        /// correlated to a specific transaction.
        fill_info: Option<FillInfo>,
    },
    /// Order expired: the deadline has passed and the Permit2 nonce was not consumed.
    Expired {
        /// How long ago the deadline passed, in seconds.
        expired_ago: u64,
    },
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
