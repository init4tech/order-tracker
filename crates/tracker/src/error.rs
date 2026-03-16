use alloy::{
    primitives::B256,
    transports::{RpcError, TransportErrorKind},
};
use std::fmt::{self, Display, Formatter};

/// Errors returned by the order tracker.
#[derive(Debug)]
#[non_exhaustive]
pub enum Error {
    /// The order was not found in the transaction cache.
    OrderNotFound(B256),
    /// Permit2 nonce bitmap query failed.
    NonceCheck(signet_orders::permit2::Permit2Error),
    /// ERC-20 `balanceOf` call failed.
    BalanceQuery(alloy::contract::Error),
    /// ERC-20 `allowance` call failed.
    AllowanceQuery(alloy::contract::Error),
    /// On-chain `Filled` event log query failed.
    FilledEventQuery(RpcError<TransportErrorKind>),
    /// Transaction cache request failed.
    TxCache(signet_tx_cache::TxCacheError),
}

impl Display for Error {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::OrderNotFound(hash) => write!(f, "order {hash} not found in tx-cache")?,
            Self::NonceCheck(_) => "failed to check Permit2 nonce".fmt(f)?,
            Self::BalanceQuery(_) => "failed to query ERC-20 balance".fmt(f)?,
            Self::AllowanceQuery(_) => "failed to query ERC-20 allowance".fmt(f)?,
            Self::FilledEventQuery(_) => "failed to query Filled event logs".fmt(f)?,
            Self::TxCache(_) => "tx-cache request failed".fmt(f)?,
        }

        if f.alternate() {
            let mut source = std::error::Error::source(self);
            while let Some(cause) = source {
                write!(f, ": {cause}")?;
                source = cause.source();
            }
        }

        Ok(())
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::OrderNotFound(_) => None,
            Self::NonceCheck(error) => Some(error),
            Self::BalanceQuery(error) => Some(error),
            Self::AllowanceQuery(error) => Some(error),
            Self::FilledEventQuery(error) => Some(error),
            Self::TxCache(error) => Some(error),
        }
    }
}

impl From<signet_orders::permit2::Permit2Error> for Error {
    fn from(error: signet_orders::permit2::Permit2Error) -> Self {
        Self::NonceCheck(error)
    }
}

impl From<RpcError<TransportErrorKind>> for Error {
    fn from(error: RpcError<TransportErrorKind>) -> Self {
        Self::FilledEventQuery(error)
    }
}

impl From<signet_tx_cache::TxCacheError> for Error {
    fn from(error: signet_tx_cache::TxCacheError) -> Self {
        Self::TxCache(error)
    }
}
