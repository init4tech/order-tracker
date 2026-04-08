use crate::{
    Error, Timestamp, fill_search,
    order_diagnostics::{
        AllowanceCheck, AllowanceChecks, BalanceCheck, BalanceChecks, DeadlineCheck, MaybeBool,
        OrderDiagnostics,
    },
    order_status::{Chain, FillInfo, FillOutput, OrderStatus},
    token_symbol_cache::TokenSymbolCache,
};
use alloy::{
    primitives::{Address, B256},
    providers::Provider,
};
use core::pin::pin;
use futures_util::TryStreamExt;
use signet_constants::SignetSystemConstants;
use signet_orders::permit2;
use signet_tx_cache::TxCache;
use signet_types::SignedOrder;
use std::time::Duration;
use tracing::{debug, instrument, warn};

/// Tracks the lifecycle and diagnoses fill failures of Signet orders.
///
/// Generic over rollup and host chain providers. All on-chain queries use the alloy `Provider`
/// trait, so any transport (HTTP, WS, IPC) works.
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
    /// Determine the current status of an order by fetching it from the tx-cache first.
    ///
    /// Returns [`Error::OrderNotFound`] if the order is not in the tx-cache.
    #[instrument(skip_all, fields(%order_hash))]
    pub async fn status(&self, order_hash: B256) -> Result<OrderStatus, Error> {
        let order = self.fetch_order(order_hash).await?;
        Ok(self.status_for_order(&order, true).await)
    }

    /// Determine the current status of an already-fetched order.
    ///
    /// Same as [`status`](Self::status) but skips the tx-cache lookup, using the provided
    /// [`SignedOrder`] directly. The caller provides `is_in_cache` to indicate whether the order
    /// is currently in the tx-cache.
    #[instrument(skip_all, fields(order_hash = %order.order_hash()))]
    pub async fn status_for_order(&self, order: &SignedOrder, is_in_cache: bool) -> OrderStatus {
        let order_hash = *order.order_hash();
        let owner = order.permit().owner;
        let now = now_unix();

        let deadline_check = self.check_deadline(order, now);
        let in_cache = MaybeBool::from(is_in_cache);
        let Ok(nonce_consumed) = self.is_nonce_consumed(order).await else {
            return self.build_pending_unknown_nonce(order_hash, owner, in_cache, deadline_check);
        };

        if nonce_consumed {
            let fill_info = self.find_fill(order).await.unwrap_or_default();
            return OrderStatus::Filled { order_hash, owner, fill_info };
        }

        let now_ts = Timestamp::from(now);
        let balance_checks = self.check_balances(order, now_ts).await;
        let allowance_checks = self.check_allowances(order, now_ts).await;

        let diagnostics = OrderDiagnostics {
            is_in_cache: in_cache,
            deadline_check,
            balance_checks,
            allowance_checks,
        };

        if deadline_check.deadline.as_secs() < now {
            OrderStatus::Expired { order_hash, owner, diagnostics }
        } else {
            OrderStatus::Pending { order_hash, owner, diagnostics }
        }
    }

    /// Check whether the order's Permit2 nonce has been consumed on-chain.
    #[instrument(skip_all, fields(order_hash = %order.order_hash()))]
    pub async fn is_nonce_consumed(&self, order: &SignedOrder) -> Result<bool, Error> {
        permit2::is_order_nonce_consumed(&self.ru_provider, order)
            .await
            .map_err(Error::NonceCheck)
            .inspect_err(|error| warn!("failed to check if order nonce was consumed: {error:#}"))
    }

    /// Check the order's deadline against the current time.
    pub fn check_deadline(&self, order: &SignedOrder, checked_at: u64) -> DeadlineCheck {
        let deadline: u64 = order.permit().permit.deadline.to();
        let expires_in = if checked_at < deadline {
            Duration::from_secs(deadline - checked_at)
        } else {
            Duration::ZERO
        };
        DeadlineCheck {
            expires_in: expires_in.into(),
            deadline: deadline.into(),
            checked_at: checked_at.into(),
        }
    }

    /// Check the order owner's ERC-20 balances for all input tokens on the rollup.
    pub async fn check_balances(
        &self,
        order: &SignedOrder,
        checked_at: Timestamp,
    ) -> BalanceChecks {
        let owner = order.permit().owner;
        let mut checks = Vec::with_capacity(order.permit().permit.permitted.len());
        let mut all_sufficient = MaybeBool::True;

        for permitted in &order.permit().permit.permitted {
            match fill_search::balance_of(&self.ru_provider, permitted.token, owner).await {
                Ok(balance) => {
                    let token_symbol =
                        self.token_symbols.resolve(&self.ru_provider, permitted.token).await;
                    let sufficient = balance >= permitted.amount;
                    if !sufficient {
                        all_sufficient = MaybeBool::False;
                    }
                    checks.push(BalanceCheck {
                        sufficient,
                        token_contract: permitted.token,
                        token_symbol,
                        balance: balance.into(),
                        required: permitted.amount.into(),
                    });
                }
                Err(error) => {
                    warn!(token_address = %permitted.token, "failed to check balance: {error:#}");
                    all_sufficient = MaybeBool::Unknown;
                }
            }
        }

        if checks.is_empty() {
            all_sufficient = MaybeBool::Unknown;
        }

        BalanceChecks { all_sufficient, checked_at, checks }
    }

    /// Check the order owner's ERC-20 allowances to the Permit2 contract for all input tokens on the rollup.
    pub async fn check_allowances(
        &self,
        order: &SignedOrder,
        checked_at: Timestamp,
    ) -> AllowanceChecks {
        let owner = order.permit().owner;
        let permit2_addr = permit2::PERMIT2;
        let mut checks = Vec::with_capacity(order.permit().permit.permitted.len());
        let mut all_sufficient = MaybeBool::True;

        for permitted in &order.permit().permit.permitted {
            match fill_search::allowance(&self.ru_provider, permitted.token, owner, permit2_addr)
                .await
            {
                Ok(allowance_amount) => {
                    let token_symbol =
                        self.token_symbols.resolve(&self.ru_provider, permitted.token).await;
                    let sufficient = allowance_amount >= permitted.amount;
                    if !sufficient {
                        all_sufficient = MaybeBool::False;
                    }
                    checks.push(AllowanceCheck {
                        sufficient,
                        token_contract: permitted.token,
                        token_symbol,
                        allowance: allowance_amount.into(),
                        required: permitted.amount.into(),
                    });
                }
                Err(error) => {
                    warn!(
                        token_address = %permitted.token,
                        %owner,
                        "failed to check allowance: {error:#}"
                    );
                    all_sufficient = MaybeBool::Unknown;
                }
            }
        }

        if checks.is_empty() {
            all_sufficient = MaybeBool::Unknown;
        }

        AllowanceChecks { all_sufficient, checked_at, checks }
    }

    /// Search for the on-chain fill transaction.
    #[instrument(skip_all, fields(order_hash = %order.order_hash()))]
    pub async fn find_fill(&self, order: &SignedOrder) -> Result<Option<FillInfo>, Error> {
        let ru_orders = self.constants.rollup().orders();
        let host_orders = self.constants.host().orders();

        let ru_tip = self
            .ru_provider
            .get_block_number()
            .await
            .map_err(Error::FilledEventQuery)
            .inspect_err(|error| warn!("failed to get rollup tip when finding fill: {error:#}"))?;
        let ru_start = ru_tip.saturating_sub(300);

        let initiation = fill_search::find_order_initiation(
            &self.ru_provider,
            order,
            ru_orders,
            ru_start..=ru_tip,
        )
        .await
        .inspect_err(|error| warn!("failed to find order initiation: {error:#}"))?;

        let Some(initiation) = initiation else {
            debug!("no Order event found on rollup, cannot locate fill");
            return Ok(None);
        };
        debug!(block = initiation.block_number, "found order initiation on rollup");

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

        let mut fill_info = ru_result
            .inspect_err(|error| warn!("failed to find fill events on rollup: {error:#}"))?
            .or(host_result
                .inspect_err(|error| warn!("failed to find fill events on host: {error:#}"))?);
        if let Some(ref mut info) = fill_info {
            info.rollup_initiation_tx = Some(initiation.tx_hash);
            self.resolve_fill_output_symbols(&mut info.outputs).await;
        }
        Ok(fill_info)
    }

    // --- private helpers ---

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

    async fn resolve_fill_output_symbols(&self, outputs: &mut [FillOutput]) {
        for output in outputs {
            output.token_symbol =
                self.token_symbols.resolve(&self.host_provider, output.token_contract).await;
        }
    }

    /// Build a Pending status when the nonce check failed.
    fn build_pending_unknown_nonce(
        &self,
        order_hash: B256,
        owner: Address,
        is_in_cache: MaybeBool,
        deadline_check: DeadlineCheck,
    ) -> OrderStatus {
        let now_ts = Timestamp::from(now_unix());
        OrderStatus::Pending {
            order_hash,
            owner,
            diagnostics: OrderDiagnostics {
                is_in_cache,
                deadline_check,
                balance_checks: BalanceChecks {
                    all_sufficient: MaybeBool::Unknown,
                    checked_at: now_ts,
                    checks: vec![],
                },
                allowance_checks: AllowanceChecks {
                    all_sufficient: MaybeBool::Unknown,
                    checked_at: now_ts,
                    checks: vec![],
                },
            },
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
