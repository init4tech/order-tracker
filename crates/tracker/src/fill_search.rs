use crate::{
    Error,
    order_status::{Chain, ChainTransaction, FillInfo, FillOutput},
};
use alloy::{
    primitives::{Address, B256, Log, U256},
    providers::Provider,
    sol,
    sol_types::SolEvent,
};
use signet_types::SignedOrder;
use signet_zenith::RollupOrders;
use tracing::{instrument, trace, warn};

sol! {
    /// Minimal ERC-20 interface for balance and allowance queries.
    #[sol(rpc)]
    interface IERC20 {
        function balanceOf(address account) external view returns (uint256);
        function allowance(address owner, address spender) external view returns (uint256);
    }
}

/// Check the ERC-20 balance of `owner` for the given `token`.
pub(crate) async fn balance_of<P: Provider>(
    provider: &P,
    token: Address,
    owner: Address,
) -> Result<U256, Error> {
    IERC20::new(token, provider).balanceOf(owner).call().await.map_err(Error::BalanceQuery)
}

/// Check the ERC-20 allowance from `owner` to `spender` for the given `token`.
pub(crate) async fn allowance<P: Provider>(
    provider: &P,
    token: Address,
    owner: Address,
    spender: Address,
) -> Result<U256, Error> {
    IERC20::new(token, provider)
        .allowance(owner, spender)
        .call()
        .await
        .map_err(Error::AllowanceQuery)
}

/// Search for a `Filled` event whose outputs are a superset of the order's expected outputs.
///
/// Scans `orders_contract` for [`RollupOrders::Filled`] events within `block_range`. Returns the
/// first match, or `None` if no matching fill was found.
#[instrument(skip_all, fields(
    order_hash = %order.order_hash(),
    contract = %orders_contract,
    from = block_range.start(),
    to = block_range.end(),
))]
pub(crate) async fn find_fill_events<P: Provider>(
    provider: &P,
    order: &SignedOrder,
    orders_contract: Address,
    chain: Chain,
    resolve_chain_id: impl Fn(u32) -> Chain,
    block_range: core::ops::RangeInclusive<u64>,
) -> Result<Option<FillInfo>, Error> {
    let filter = alloy::rpc::types::Filter::new()
        .address(orders_contract)
        .event_signature(RollupOrders::Filled::SIGNATURE_HASH)
        .from_block(*block_range.start())
        .to_block(*block_range.end());

    let logs = provider.get_logs(&filter).await.map_err(Error::FilledEventQuery)?;
    let expected_outputs = order.outputs();
    trace!(log_count = logs.len(), "scanning Filled events");

    for log in logs {
        let Some(tx_hash) = log.transaction_hash else { continue };
        let Some(block_number) = log.block_number else { continue };

        let decoded = match RollupOrders::Filled::decode_log(&Log::new_unchecked(
            log.address(),
            log.topics().to_vec(),
            log.data().data.clone(),
        )) {
            Ok(decoded) => decoded,
            Err(error) => {
                warn!(%tx_hash, block_number, %error, "failed to decode Filled event");
                continue;
            }
        };

        trace!(
            %tx_hash, block_number,
            fill_outputs = ?decoded.outputs.iter().map(|o| format!(
                "{}:{} -> {}@{}", o.token, o.amount, o.recipient, o.chainId
            )).collect::<Vec<_>>(),
            expected_outputs = ?expected_outputs.iter().map(|o| format!(
                "{}:{} -> {}@{}", o.token, o.amount, o.recipient, o.chainId
            )).collect::<Vec<_>>(),
            "comparing fill outputs"
        );

        if fill_outputs_are_superset_of_order_outputs(&decoded.outputs, expected_outputs) {
            return Ok(Some(FillInfo {
                block_number,
                rollup_initiation_tx: None,
                fill_tx: Some(ChainTransaction { chain, tx_hash }),
                outputs: decoded
                    .data
                    .outputs
                    .into_iter()
                    .map(|output| FillOutput {
                        token_contract: output.token,
                        token_symbol: String::new(),
                        amount: output.amount.into(),
                        recipient: output.recipient,
                        chain: resolve_chain_id(output.chainId),
                    })
                    .collect(),
            }));
        }
    }

    Ok(None)
}

/// Details about the order initiation on the rollup.
pub(crate) struct OrderInitiation {
    /// The block number where the order was initiated.
    pub(crate) block_number: u64,
    /// The transaction hash of the `initiatePermit2` call.
    pub(crate) tx_hash: B256,
}

/// Search for the `Order` event on the rollup that matches this order's deadline, returning the
/// block number and tx hash where the order was initiated.
#[instrument(skip_all, fields(
    order_hash = %order.order_hash(),
    contract = %orders_contract,
    from = block_range.start(),
    to = block_range.end(),
))]
pub(crate) async fn find_order_initiation<P: Provider>(
    provider: &P,
    order: &SignedOrder,
    orders_contract: Address,
    block_range: core::ops::RangeInclusive<u64>,
) -> Result<Option<OrderInitiation>, Error> {
    let filter = alloy::rpc::types::Filter::new()
        .address(orders_contract)
        .event_signature(RollupOrders::Order::SIGNATURE_HASH)
        .from_block(*block_range.start())
        .to_block(*block_range.end());

    let logs = provider.get_logs(&filter).await.map_err(Error::FilledEventQuery)?;
    let deadline: U256 = order.permit().permit.deadline;
    trace!(log_count = logs.len(), %deadline, "scanning Order events");

    for log in logs {
        let Some(block_number) = log.block_number else { continue };
        let Some(tx_hash) = log.transaction_hash else { continue };

        let decoded = match RollupOrders::Order::decode_log(&Log::new_unchecked(
            log.address(),
            log.topics().to_vec(),
            log.data().data.clone(),
        )) {
            Ok(decoded) => decoded,
            Err(error) => {
                warn!(%tx_hash, block_number, %error, "failed to decode Order event");
                continue;
            }
        };

        if decoded.deadline == deadline {
            return Ok(Some(OrderInitiation { block_number, tx_hash }));
        }
    }

    Ok(None)
}

/// Checks whether `fill_outputs` is a multiset superset of `order_outputs` — i.e. every order
/// output has a distinct matching fill output (by token, amount, and recipient). Fills aggregate
/// multiple orders, so extra fill outputs are expected and allowed. Each fill output can only
/// satisfy one order output, so duplicate order outputs require correspondingly many fills.
///
/// `chainId` is intentionally excluded from comparison: the order specifies the desired output
/// chain, but the fill event records the chain it was executed on, which may differ.
pub fn fill_outputs_are_superset_of_order_outputs(
    fill_outputs: &[RollupOrders::Output],
    order_outputs: &[RollupOrders::Output],
) -> bool {
    let mut available: Vec<_> = fill_outputs.iter().collect();
    order_outputs.iter().all(|expected| {
        available
            .iter()
            .position(|fill| {
                fill.token == expected.token
                    && fill.amount == expected.amount
                    && fill.recipient == expected.recipient
            })
            .map(|idx| available.swap_remove(idx))
            .is_some()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy::primitives::Address;

    fn output(token: u8, amount: u64, recipient: u8, chain_id: u32) -> RollupOrders::Output {
        RollupOrders::Output {
            token: Address::repeat_byte(token),
            amount: U256::from(amount),
            recipient: Address::repeat_byte(recipient),
            chainId: chain_id,
        }
    }

    #[test]
    fn exact_match_is_superset() {
        let fill = vec![output(1, 100, 2, 1), output(1, 100, 2, 1)];
        let order = fill.clone();
        assert!(fill_outputs_are_superset_of_order_outputs(&fill, &order));
    }

    #[test]
    fn extra_fill_outputs_is_superset() {
        let fill = vec![output(1, 100, 2, 1), output(3, 200, 4, 2)];
        let order = vec![output(1, 100, 2, 1)];
        assert!(fill_outputs_are_superset_of_order_outputs(&fill, &order));
    }

    #[test]
    fn missing_fill_output_is_not_superset() {
        let fill = vec![output(1, 100, 2, 1)];
        let order = vec![output(1, 100, 2, 1), output(3, 200, 4, 2)];
        assert!(!fill_outputs_are_superset_of_order_outputs(&fill, &order));
    }

    #[test]
    fn different_amount_is_not_superset() {
        let fill = vec![output(1, 99, 2, 1)];
        let order = vec![output(1, 100, 2, 1)];
        assert!(!fill_outputs_are_superset_of_order_outputs(&fill, &order));
    }

    #[test]
    fn duplicate_order_output_not_satisfied_by_single_fill() {
        let fill = vec![output(1, 100, 2, 1)];
        let order = vec![output(1, 100, 2, 1), output(1, 100, 2, 1)];
        assert!(!fill_outputs_are_superset_of_order_outputs(&fill, &order));
    }

    #[test]
    fn duplicate_fills_with_single_order_output_is_superset() {
        let fill = vec![output(1, 100, 2, 1), output(1, 100, 2, 1)];
        let order = vec![output(1, 100, 2, 1)];
        assert!(fill_outputs_are_superset_of_order_outputs(&fill, &order));
    }

    #[test]
    fn both_empty_is_superset() {
        assert!(fill_outputs_are_superset_of_order_outputs(&[], &[]));
    }

    #[test]
    fn empty_fill_with_order_outputs_is_not_superset() {
        let order = vec![output(1, 100, 2, 1)];
        assert!(!fill_outputs_are_superset_of_order_outputs(&[], &order));
    }

    #[test]
    fn empty_order_outputs_with_fill_is_superset() {
        let fill = vec![output(1, 100, 2, 1)];
        assert!(fill_outputs_are_superset_of_order_outputs(&fill, &[]));
    }
}
