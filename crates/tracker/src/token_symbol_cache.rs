use alloy::{primitives::Address, providers::Provider, sol};
use signet_constants::SignetSystemConstants;
use std::{collections::HashMap, sync::RwLock};
use tracing::{debug, warn};

sol! {
    /// Minimal ERC-20 interface for querying the token symbol.
    #[sol(rpc)]
    interface IERC20Meta {
        function symbol() external view returns (string);
    }
}

const UNKNOWN: &str = "unknown";

/// Resolves ERC-20 token addresses to human-readable symbols, caching results in memory.
///
/// Checks signet constants first (free), then falls back to on-chain `symbol()` calls. Failed
/// lookups are cached as `"unknown"` to avoid repeated RPC calls.
#[derive(Debug)]
pub(crate) struct TokenSymbolCache {
    cache: RwLock<HashMap<Address, String>>,
}

impl TokenSymbolCache {
    /// Create a new cache seeded from the given constants.
    pub(crate) fn new(constants: &SignetSystemConstants) -> Self {
        let mut cache = HashMap::new();

        let rollup_tokens = constants.rollup().tokens();
        cache.insert(rollup_tokens.weth(), "WETH".into());
        cache.insert(rollup_tokens.wbtc(), "WBTC".into());

        let host_tokens = constants.host().tokens();
        cache.insert(host_tokens.weth(), "WETH".into());
        cache.insert(host_tokens.wbtc(), "WBTC".into());
        cache.insert(host_tokens.usdc(), "USDC".into());
        cache.insert(host_tokens.usdt(), "USDT".into());

        Self { cache: RwLock::new(cache) }
    }

    /// Resolve the symbol for a token address. Checks the cache first, then queries on-chain.
    /// Returns `"unknown"` if the symbol cannot be resolved.
    pub(crate) async fn resolve<P: Provider>(&self, provider: &P, address: Address) -> String {
        if let Some(cached) = self.cache.read().unwrap().get(&address) {
            return cached.clone();
        }

        let symbol = match IERC20Meta::new(address, provider).symbol().call().await {
            Ok(symbol) => {
                debug!(%address, %symbol, "resolved token symbol on-chain");
                symbol
            }
            Err(error) => {
                warn!(%address, %error, "failed to resolve token symbol");
                UNKNOWN.into()
            }
        };

        self.cache.write().unwrap().insert(address, symbol.clone());
        symbol
    }
}
