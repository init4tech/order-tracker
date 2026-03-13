use crate::{
    Error, fill_search,
    order_diagnostics::{
        AllowanceCheck, BalanceCheck, DeadlineCheck, OrderDiagnostics, TokenAllowance, TokenBalance,
    },
    order_status::{Chain, FillInfo, FillOutput, OrderReport, OrderStatus},
    token_symbol_cache::TokenSymbolCache,
};
use alloy::{primitives::B256, providers::Provider};
use core::pin::pin;
use futures_util::TryStreamExt;
use signet_constants::SignetSystemConstants;
use signet_orders::permit2;
use signet_tx_cache::TxCache;
use signet_types::SignedOrder;
use tracing::{debug, instrument, trace};

/// Tracks the lifecycle and diagnoses fill failures of Signet orders.
///
/// Generic over rollup and host chain providers. All on-chain queries use the alloy `Provider`
/// trait, so any transport (HTTP, WS, IPC) works.
///
/// The single entry point — [`status`](Self::status) — accepts an order hash (`B256`), runs all
/// diagnostic checks, and derives the lifecycle status from the results.
#[derive(Debug)]
pub struct OrderTracker<RuP, HostP> {
    ru_provider: RuP,
    host_provider: HostP,
    tx_cache: TxCache,
    constants: SignetSystemConstants,
    token_symbols: TokenSymbolCache,
}

impl<RuP, HostP> OrderTracker<RuP, HostP> {
    /// Create a new order tracker.
    pub fn new(
        ru_provider: RuP,
        host_provider: HostP,
        tx_cache: TxCache,
        constants: SignetSystemConstants,
    ) -> Self {
        let token_symbols = TokenSymbolCache::new(&constants);
        Self { ru_provider, host_provider, tx_cache, constants, token_symbols }
    }
}

impl<RuP: Provider, HostP: Provider> OrderTracker<RuP, HostP> {
    /// Determine the current status of an order and run full diagnostics.
    ///
    /// Fetches the order from the tx-cache, performs all diagnostic checks (deadline, nonce,
    /// balances, allowances), and derives the lifecycle status from the results. If the order has
    /// been filled, scans on-chain events to locate the fill transaction.
    ///
    /// Returns [`Error::OrderNotFound`] if the order is not in the tx-cache.
    #[instrument(skip_all, fields(%order_hash))]
    pub async fn status(&self, order_hash: B256) -> Result<OrderReport, Error> {
        let order = self.fetch_order(order_hash).await?;
        let now = now_unix();

        let deadline = self.check_deadline(&order, now);
        let in_cache = self.is_in_cache(&order).await?;
        let nonce_consumed = self.is_nonce_consumed(&order).await?;
        let balances = self.check_balances(&order).await?;
        let allowances = self.check_allowances(&order).await?;

        let diagnostics = OrderDiagnostics {
            deadline: Some(deadline),
            in_cache: Some(in_cache),
            permit2_nonce_consumed: Some(nonce_consumed),
            balances: Some(balances),
            allowances: Some(allowances),
        };

        let status = if nonce_consumed {
            let fill_info = self.find_fill(&order).await?;
            OrderStatus::Filled { fill_info }
        } else if deadline.is_expired {
            OrderStatus::Expired { expired_ago: now - deadline.deadline.as_secs() }
        } else {
            OrderStatus::Pending { seconds_remaining: deadline.deadline.as_secs() - now }
        };

        Ok(OrderReport { status, diagnostics })
    }

    /// Fetch a specific order from the tx-cache by its hash.
    #[instrument(skip_all, fields(%order_hash))]
    async fn fetch_order(&self, order_hash: B256) -> Result<SignedOrder, Error> {
        let mut stream = pin!(self.tx_cache.stream_orders());
        while let Some(order) = stream.try_next().await.map_err(Error::TxCache)? {
            if *order.order_hash() == order_hash {
                return Ok(order);
            }
        }
        Err(Error::OrderNotFound(order_hash))
    }

    /// Check whether the order's deadline has passed.
    fn check_deadline(&self, order: &SignedOrder, checked_at: u64) -> DeadlineCheck {
        let deadline: u64 = order.permit().permit.deadline.to();
        DeadlineCheck {
            deadline: deadline.into(),
            checked_at: checked_at.into(),
            is_expired: checked_at > deadline,
        }
    }

    /// Check whether the order is present in the transaction cache.
    #[instrument(skip_all, fields(order_hash = %order.order_hash()))]
    async fn is_in_cache(&self, order: &SignedOrder) -> Result<bool, Error> {
        let target_hash = *order.order_hash();
        let found = self
            .tx_cache
            .stream_orders()
            .try_any(|cached| core::future::ready(*cached.order_hash() == target_hash))
            .await
            .map_err(Error::TxCache)?;
        trace!(found, "cache lookup complete");
        Ok(found)
    }

    /// Check whether the order's Permit2 nonce has been consumed on-chain.
    #[instrument(skip_all, fields(order_hash = %order.order_hash()))]
    async fn is_nonce_consumed(&self, order: &SignedOrder) -> Result<bool, Error> {
        permit2::is_order_nonce_consumed(&self.ru_provider, order).await.map_err(Error::NonceCheck)
    }

    /// Check the order owner's ERC-20 balances for all input tokens on the rollup.
    #[instrument(skip_all, fields(order_hash = %order.order_hash()))]
    async fn check_balances(&self, order: &SignedOrder) -> Result<BalanceCheck, Error> {
        let owner = order.permit().owner;
        let mut tokens = Vec::with_capacity(order.permit().permit.permitted.len());

        for permitted in &order.permit().permit.permitted {
            let balance =
                fill_search::balance_of(&self.ru_provider, permitted.token, owner).await?;
            let token_symbol = self.token_symbols.resolve(&self.ru_provider, permitted.token).await;
            tokens.push(TokenBalance {
                token_contract: permitted.token,
                token_symbol,
                balance: balance.into(),
                required: permitted.amount.into(),
                sufficient: balance >= permitted.amount,
            });
        }

        let all_sufficient = tokens.iter().all(|token| token.sufficient);
        Ok(BalanceCheck { tokens, all_sufficient })
    }

    /// Check the order owner's ERC-20 allowances to the Permit2 contract for all input tokens on the rollup.
    #[instrument(skip_all, fields(order_hash = %order.order_hash()))]
    async fn check_allowances(&self, order: &SignedOrder) -> Result<AllowanceCheck, Error> {
        let owner = order.permit().owner;
        let permit2_addr = permit2::PERMIT2;
        let mut tokens = Vec::with_capacity(order.permit().permit.permitted.len());

        for permitted in &order.permit().permit.permitted {
            let allowance_amount =
                fill_search::allowance(&self.ru_provider, permitted.token, owner, permit2_addr)
                    .await?;
            let token_symbol = self.token_symbols.resolve(&self.ru_provider, permitted.token).await;
            tokens.push(TokenAllowance {
                token_contract: permitted.token,
                token_symbol,
                allowance: allowance_amount.into(),
                required: permitted.amount.into(),
                sufficient: allowance_amount >= permitted.amount,
            });
        }

        let all_sufficient = tokens.iter().all(|token| token.sufficient);
        Ok(AllowanceCheck { tokens, all_sufficient })
    }

    /// Search for the on-chain fill transaction.
    ///
    /// Locates the `Order` event on the rollup matching this order's deadline to find the
    /// initiation block. Then searches for a matching `Filled` event in that block on both chains
    /// in parallel. The deadline is second-precision but combined with the output matching is
    /// sufficient to uniquely identify the order in practice.
    #[instrument(skip_all, fields(order_hash = %order.order_hash()))]
    async fn find_fill(&self, order: &SignedOrder) -> Result<Option<FillInfo>, Error> {
        let ru_orders = self.constants.rollup().orders();
        let host_orders = self.constants.host().orders();

        let ru_tip = self.ru_provider.get_block_number().await.map_err(Error::FilledEventQuery)?;
        let ru_start = ru_tip.saturating_sub(300);

        let initiation = fill_search::find_order_initiation(
            &self.ru_provider,
            order,
            ru_orders,
            ru_start..=ru_tip,
        )
        .await?;

        let Some(initiation) = initiation else {
            debug!("no Order event found on rollup, cannot locate fill");
            return Ok(None);
        };
        debug!(block = initiation.block_number, "found order initiation on rollup");

        // Search for the Filled event in that block on both chains in parallel.
        let block = initiation.block_number;
        let resolve_chain = |chain_id: u32| self.resolve_chain_id(chain_id);
        let (ru_result, host_result) = tokio::join!(
            fill_search::find_fill_events(
                &self.ru_provider,
                order,
                ru_orders,
                Chain::Rollup,
                &resolve_chain,
                block..=block,
            ),
            fill_search::find_fill_events(
                &self.host_provider,
                order,
                host_orders,
                Chain::Host,
                &resolve_chain,
                block..=block,
            ),
        );

        // Attach the initiation tx hash and resolve token symbols for outputs.
        let mut fill_info = ru_result?.or(host_result?);
        if let Some(ref mut info) = fill_info {
            info.rollup_initiation_tx = Some(initiation.tx_hash);
            self.resolve_fill_output_symbols(&mut info.outputs).await;
        }
        Ok(fill_info)
    }

    /// Resolve a numeric chain ID to a [`Chain`] variant.
    fn resolve_chain_id(&self, chain_id: u32) -> Chain {
        let chain_id = u64::from(chain_id);
        if chain_id == self.constants.host_chain_id() {
            Chain::Host
        } else if chain_id == self.constants.ru_chain_id() {
            Chain::Rollup
        } else {
            Chain::Host
        }
    }

    /// Resolve token symbols for fill outputs, using the host provider since fill outputs
    /// reference host-chain tokens.
    async fn resolve_fill_output_symbols(&self, outputs: &mut [FillOutput]) {
        for output in outputs {
            output.token_symbol =
                self.token_symbols.resolve(&self.host_provider, output.token_contract).await;
        }
    }
}

/// Current unix timestamp in seconds.
fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock before unix epoch")
        .as_secs()
}
