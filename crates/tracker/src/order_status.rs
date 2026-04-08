use crate::{Amount, order_diagnostics::OrderDiagnostics};
use alloy::primitives::{Address, B256};
use serde::{Deserialize, Serialize};
use std::fmt::{self, Display, Formatter};

/// The resolved lifecycle status of a Signet order.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum OrderStatus {
    /// Order is pending: the Permit2 nonce has not been consumed, and the deadline has not passed.
    Pending {
        /// The order hash.
        order_hash: B256,
        /// The order owner (Permit2 signer).
        owner: Address,
        /// Diagnostics for the order.
        #[serde(flatten)]
        diagnostics: OrderDiagnostics,
    },
    /// Order has been filled: the Permit2 nonce has been consumed on-chain.
    Filled {
        /// The order hash.
        order_hash: B256,
        /// The order owner (Permit2 signer).
        owner: Address,
        /// Details about the fill transaction, if located. `None` if the fill event could not be
        /// correlated to a specific transaction.
        fill_info: Option<FillInfo>,
    },
    /// Order expired: the deadline has passed and the Permit2 nonce was not consumed.
    Expired {
        /// The order hash.
        order_hash: B256,
        /// The order owner (Permit2 signer).
        owner: Address,
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

    /// The order owner (Permit2 signer).
    pub const fn owner(&self) -> Address {
        match self {
            Self::Pending { owner, .. }
            | Self::Filled { owner, .. }
            | Self::Expired { owner, .. } => *owner,
        }
    }

    /// Whether this is a terminal state (filled or expired).
    pub const fn is_terminal(&self) -> bool {
        matches!(self, Self::Filled { .. } | Self::Expired { .. })
    }
}

/// Which chain a transaction was observed on.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
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
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct ChainTransaction {
    /// Which chain the transaction is on.
    pub chain: Chain,
    /// The transaction hash.
    pub tx_hash: B256,
}

/// Information about a fill that matched an order.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FillInfo {
    /// The block number containing the fill.
    pub block_number: u64,
    /// The rollup transaction that initiated the order (from the `Order` event).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub rollup_initiation_tx: Option<B256>,
    /// The transaction that emitted the `Filled` event.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub fill_tx: Option<ChainTransaction>,
    /// The outputs delivered by the fill.
    pub outputs: Vec<FillOutput>,
}

/// A single output from a fill event.
#[derive(Debug, Clone, Serialize, Deserialize)]
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        PrettyDuration, Timestamp,
        order_diagnostics::{
            AllowanceCheck, AllowanceChecks, BalanceChecks, DeadlineCheck, MaybeBool,
            OrderDiagnostics,
        },
    };
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
                all_sufficient: MaybeBool::True,
                checked_at: Timestamp::new(1_700_000_000),
                checks: vec![],
            },
        }
    }

    fn assert_json_roundtrip<T: serde::Serialize + serde::de::DeserializeOwned>(value: &T) {
        let json = serde_json::to_string(value).unwrap();
        let deserialized: T = serde_json::from_str(&json).unwrap();
        assert_eq!(json, serde_json::to_string(&deserialized).unwrap());
    }

    #[test]
    fn chain_roundtrip() {
        for chain in [Chain::Rollup, Chain::Host] {
            assert_json_roundtrip(&chain);
        }
    }

    #[test]
    fn chain_transaction_roundtrip() {
        assert_json_roundtrip(&ChainTransaction { chain: Chain::Rollup, tx_hash: B256::ZERO });
    }

    #[test]
    fn fill_output_roundtrip() {
        assert_json_roundtrip(&FillOutput {
            token_contract: Address::ZERO,
            token_symbol: "WETH".into(),
            amount: Amount::new(U256::from(1_000_000u64)),
            recipient: Address::ZERO,
            chain: Chain::Host,
        });
    }

    #[test]
    fn fill_info_full_roundtrip() {
        assert_json_roundtrip(&FillInfo {
            block_number: 42,
            rollup_initiation_tx: Some(B256::ZERO),
            fill_tx: Some(ChainTransaction { chain: Chain::Rollup, tx_hash: B256::ZERO }),
            outputs: vec![FillOutput {
                token_contract: Address::ZERO,
                token_symbol: "WETH".into(),
                amount: Amount::new(U256::from(1_000_000u64)),
                recipient: Address::ZERO,
                chain: Chain::Rollup,
            }],
        });
    }

    #[test]
    fn fill_info_minimal_roundtrip() {
        assert_json_roundtrip(&FillInfo {
            block_number: 1,
            rollup_initiation_tx: None,
            fill_tx: None,
            outputs: vec![],
        });
    }

    #[test]
    fn order_status_pending_roundtrip() {
        assert_json_roundtrip(&OrderStatus::Pending {
            order_hash: B256::ZERO,
            owner: Address::ZERO,
            diagnostics: sample_diagnostics(),
        });
    }

    #[test]
    fn order_status_filled_roundtrip() {
        assert_json_roundtrip(&OrderStatus::Filled {
            order_hash: B256::ZERO,
            owner: Address::ZERO,
            fill_info: Some(FillInfo {
                block_number: 99,
                rollup_initiation_tx: Some(B256::ZERO),
                fill_tx: Some(ChainTransaction { chain: Chain::Host, tx_hash: B256::ZERO }),
                outputs: vec![],
            }),
        });
    }

    #[test]
    fn order_status_filled_no_info_roundtrip() {
        assert_json_roundtrip(&OrderStatus::Filled {
            order_hash: B256::ZERO,
            owner: Address::ZERO,
            fill_info: None,
        });
    }

    #[test]
    fn order_status_expired_roundtrip() {
        assert_json_roundtrip(&OrderStatus::Expired {
            order_hash: B256::ZERO,
            owner: Address::ZERO,
            diagnostics: sample_diagnostics(),
        });
    }
}
