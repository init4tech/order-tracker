use crate::{Amount, PrettyDuration, Timestamp};
use alloy::primitives::Address;
use serde::Serialize;

/// Diagnostic report explaining why an order may not have been filled.
///
/// Each field represents an independent check. Fields are `None` when the check was not run (for
/// example, because a prior check already determined the order's status conclusively).
#[derive(Debug, Clone, Serialize)]
pub struct OrderDiagnostics {
    /// Whether the order was found in the transaction cache.
    pub is_in_cache: MaybeBool,
    /// Deadline expiry check.
    pub deadline_check: DeadlineCheck,
    /// ERC-20 allowance check for each input token (owner -> Permit2).
    pub allowance_checks: AllowanceChecks,
    /// ERC-20 balance check for each input token.
    pub balance_checks: BalanceChecks,
}

/// Result of checking an order's deadline against the current time.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct DeadlineCheck {
    /// How long until the deadline expires.
    pub expires_in: PrettyDuration,
    /// The order's deadline.
    pub deadline: Timestamp,
    /// The timestamp at which the check was performed.
    pub checked_at: Timestamp,
}

/// Aggregated ERC-20 allowance check across all input tokens.
#[derive(Debug, Clone, Serialize)]
pub struct AllowanceChecks {
    /// Whether all input tokens have sufficient allowance to Permit2.
    pub all_sufficient: MaybeBool,
    /// The timestamp at which the check was performed.
    pub checked_at: Timestamp,
    /// Per-token allowance details.
    pub checks: Vec<AllowanceCheck>,
}

/// ERC-20 allowance for a single input token.
#[derive(Debug, Clone, Serialize)]
pub struct AllowanceCheck {
    /// Whether allowance >= required.
    pub sufficient: bool,
    /// The token contract address.
    pub token_contract: Address,
    /// Human-readable token symbol (e.g. "WETH"), or "unknown" if unresolvable.
    pub token_symbol: String,
    /// The current allowance from the order owner to the Permit2 contract.
    pub allowance: Amount,
    /// The amount required by the order.
    pub required: Amount,
}

/// Aggregated ERC-20 balance check across all input tokens.
#[derive(Debug, Clone, Serialize)]
pub struct BalanceChecks {
    /// Whether the order owner has sufficient balance for all input tokens.
    pub all_sufficient: MaybeBool,
    /// The timestamp at which the check was performed.
    pub checked_at: Timestamp,
    /// Per-token balance details.
    pub checks: Vec<BalanceCheck>,
}

/// ERC-20 balance for a single input token.
#[derive(Debug, Clone, Serialize)]
pub struct BalanceCheck {
    /// Whether balance >= required.
    pub sufficient: bool,
    /// The token contract address.
    pub token_contract: Address,
    /// Human-readable token symbol (e.g. "WETH"), or "unknown" if unresolvable.
    pub token_symbol: String,
    /// The owner's current balance.
    pub balance: Amount,
    /// The amount required by the order.
    pub required: Amount,
}

/// A tri-state bool.
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MaybeBool {
    /// The check returned false.
    False,
    /// The check returned true.
    True,
    /// The check could not be performed (e.g. RPC failure).
    Unknown,
}

impl From<bool> for MaybeBool {
    fn from(value: bool) -> Self {
        if value { Self::True } else { Self::False }
    }
}
