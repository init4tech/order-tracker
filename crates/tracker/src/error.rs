use alloy::{
    primitives::B256,
    transports::{RpcError, TransportErrorKind},
};

/// Errors returned by the order tracker.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// The order was not found in the transaction cache.
    #[error("order {0} not found in tx-cache")]
    OrderNotFound(B256),
    /// Permit2 nonce bitmap query failed.
    #[error("failed to check Permit2 nonce")]
    NonceCheck(#[from] signet_orders::permit2::Permit2Error),
    /// ERC-20 `balanceOf` call failed.
    #[error("failed to query ERC-20 balance")]
    BalanceQuery(#[source] alloy::contract::Error),
    /// ERC-20 `allowance` call failed.
    #[error("failed to query ERC-20 allowance")]
    AllowanceQuery(#[source] alloy::contract::Error),
    /// On-chain `Filled` event log query failed.
    #[error("failed to query Filled event logs")]
    FilledEventQuery(#[from] RpcError<TransportErrorKind>),
    /// Transaction cache request failed.
    #[error("tx-cache request failed")]
    TxCache(#[from] signet_tx_cache::TxCacheError),
}
