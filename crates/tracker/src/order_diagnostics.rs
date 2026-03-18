use crate::{Amount, PrettyDuration, Timestamp};
use alloy::primitives::Address;
use serde::{Deserialize, Serialize};

/// Diagnostic report explaining why an order may not have been filled.
///
/// Each field represents an independent check. Fields are `None` when the check was not run (for
/// example, because a prior check already determined the order's status conclusively).
#[derive(Debug, Clone, Serialize, Deserialize)]
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
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct DeadlineCheck {
    /// How long until the deadline expires.
    pub expires_in: PrettyDuration,
    /// The order's deadline.
    pub deadline: Timestamp,
    /// The timestamp at which the check was performed.
    pub checked_at: Timestamp,
}

/// Aggregated ERC-20 allowance check across all input tokens.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AllowanceChecks {
    /// Whether all input tokens have sufficient allowance to Permit2.
    pub all_sufficient: MaybeBool,
    /// The timestamp at which the check was performed.
    pub checked_at: Timestamp,
    /// Per-token allowance details.
    pub checks: Vec<AllowanceCheck>,
}

/// ERC-20 allowance for a single input token.
#[derive(Debug, Clone, Serialize, Deserialize)]
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
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BalanceChecks {
    /// Whether the order owner has sufficient balance for all input tokens.
    pub all_sufficient: MaybeBool,
    /// The timestamp at which the check was performed.
    pub checked_at: Timestamp,
    /// Per-token balance details.
    pub checks: Vec<BalanceCheck>,
}

/// ERC-20 balance for a single input token.
#[derive(Debug, Clone, Serialize, Deserialize)]
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
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
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

#[cfg(test)]
mod tests {
    use super::*;
    use alloy::primitives::U256;
    use std::time::Duration;

    fn sample_diagnostics() -> OrderDiagnostics {
        OrderDiagnostics {
            is_in_cache: MaybeBool::True,
            deadline_check: DeadlineCheck {
                expires_in: PrettyDuration::new(Duration::from_secs(30)),
                deadline: Timestamp::new(1_700_000_030),
                checked_at: Timestamp::new(1_700_000_000),
            },
            allowance_checks: AllowanceChecks {
                all_sufficient: MaybeBool::True,
                checked_at: Timestamp::new(1_700_000_000),
                checks: vec![AllowanceCheck {
                    sufficient: true,
                    token_contract: Address::ZERO,
                    token_symbol: "WETH".into(),
                    allowance: Amount::new(U256::from(1_000_000u64)),
                    required: Amount::new(U256::from(500_000u64)),
                }],
            },
            balance_checks: BalanceChecks {
                all_sufficient: MaybeBool::False,
                checked_at: Timestamp::new(1_700_000_000),
                checks: vec![BalanceCheck {
                    sufficient: false,
                    token_contract: Address::ZERO,
                    token_symbol: "WETH".into(),
                    balance: Amount::new(U256::from(100_000u64)),
                    required: Amount::new(U256::from(500_000u64)),
                }],
            },
        }
    }

    fn assert_json_roundtrip<T: serde::Serialize + serde::de::DeserializeOwned>(value: &T) {
        let json = serde_json::to_string(value).unwrap();
        let deserialized: T = serde_json::from_str(&json).unwrap();
        assert_eq!(json, serde_json::to_string(&deserialized).unwrap());
    }

    #[test]
    fn maybe_bool_roundtrip() {
        for value in [MaybeBool::True, MaybeBool::False, MaybeBool::Unknown] {
            assert_json_roundtrip(&value);
        }
    }

    #[test]
    fn deadline_check_roundtrip() {
        assert_json_roundtrip(&DeadlineCheck {
            expires_in: PrettyDuration::new(Duration::from_secs(90)),
            deadline: Timestamp::new(1_700_000_090),
            checked_at: Timestamp::new(1_700_000_000),
        });
    }

    #[test]
    fn allowance_check_roundtrip() {
        assert_json_roundtrip(&AllowanceCheck {
            sufficient: true,
            token_contract: Address::ZERO,
            token_symbol: "WETH".into(),
            allowance: Amount::new(U256::from(1_000_000u64)),
            required: Amount::new(U256::from(500_000u64)),
        });
    }

    #[test]
    fn allowance_checks_roundtrip() {
        assert_json_roundtrip(&AllowanceChecks {
            all_sufficient: MaybeBool::Unknown,
            checked_at: Timestamp::new(1_700_000_000),
            checks: vec![],
        });
    }

    #[test]
    fn balance_check_roundtrip() {
        assert_json_roundtrip(&BalanceCheck {
            sufficient: false,
            token_contract: Address::ZERO,
            token_symbol: "USDC".into(),
            balance: Amount::new(U256::from(100u64)),
            required: Amount::new(U256::from(1000u64)),
        });
    }

    #[test]
    fn balance_checks_roundtrip() {
        assert_json_roundtrip(&BalanceChecks {
            all_sufficient: MaybeBool::True,
            checked_at: Timestamp::new(1_700_000_000),
            checks: vec![BalanceCheck {
                sufficient: true,
                token_contract: Address::ZERO,
                token_symbol: "WETH".into(),
                balance: Amount::new(U256::from(999u64)),
                required: Amount::new(U256::from(100u64)),
            }],
        });
    }

    #[test]
    fn order_diagnostics_roundtrip() {
        assert_json_roundtrip(&sample_diagnostics());
    }
}
