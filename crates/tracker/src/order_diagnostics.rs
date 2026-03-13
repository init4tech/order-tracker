use crate::{Amount, Timestamp};
use alloy::primitives::Address;
use serde::Serialize;

/// Diagnostic report explaining why an order may not have been filled.
///
/// Each field represents an independent check. Fields are `None` when the check was not run (for
/// example, because a prior check already determined the order's status conclusively).
#[derive(Debug, Clone, Default, Serialize)]
pub struct OrderDiagnostics {
    /// Whether the order was found in the transaction cache.
    pub in_cache: Option<bool>,
    /// Deadline expiry check.
    pub deadline: Option<DeadlineCheck>,
    /// Whether the order's Permit2 nonce has been consumed on-chain (indicates the order was filled).
    pub permit2_nonce_consumed: Option<bool>,
    /// ERC-20 allowance check for each input token (owner -> Permit2).
    pub allowances: Option<AllowanceCheck>,
    /// ERC-20 balance check for each input token.
    pub balances: Option<BalanceCheck>,
}

/// Result of checking an order's deadline against the current time.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct DeadlineCheck {
    /// The order's deadline.
    pub deadline: Timestamp,
    /// The timestamp at which the check was performed.
    pub checked_at: Timestamp,
    /// Whether the deadline has passed.
    pub is_expired: bool,
}

/// Aggregated ERC-20 allowance check across all input tokens.
#[derive(Debug, Clone, Serialize)]
pub struct AllowanceCheck {
    /// Per-token allowance details.
    pub tokens: Vec<TokenAllowance>,
    /// Whether all input tokens have sufficient allowance to Permit2.
    pub all_sufficient: bool,
}

/// ERC-20 allowance for a single input token.
#[derive(Debug, Clone, Serialize)]
pub struct TokenAllowance {
    /// The token contract address.
    pub token_contract: Address,
    /// Human-readable token symbol (e.g. "WETH"), or "unknown" if unresolvable.
    pub token_symbol: String,
    /// The current allowance from the order owner to the Permit2 contract.
    pub allowance: Amount,
    /// The amount required by the order.
    pub required: Amount,
    /// Whether allowance >= required.
    pub sufficient: bool,
}

/// Aggregated ERC-20 balance check across all input tokens.
#[derive(Debug, Clone, Serialize)]
pub struct BalanceCheck {
    /// Per-token balance details.
    pub tokens: Vec<TokenBalance>,
    /// Whether the order owner has sufficient balance for all input tokens.
    pub all_sufficient: bool,
}

/// ERC-20 balance for a single input token.
#[derive(Debug, Clone, Serialize)]
pub struct TokenBalance {
    /// The token contract address.
    pub token_contract: Address,
    /// Human-readable token symbol (e.g. "WETH"), or "unknown" if unresolvable.
    pub token_symbol: String,
    /// The owner's current balance.
    pub balance: Amount,
    /// The amount required by the order.
    pub required: Amount,
    /// Whether balance >= required.
    pub sufficient: bool,
}
