use alloy::primitives::Address;
use serde::Deserialize;

/// Order status filter values for the all-orders WS endpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum StatusFilter {
    /// Only pending orders.
    Pending,
    /// Only filled orders.
    Filled,
    /// Only expired orders.
    Expired,
}

/// Optional filter a client can send after connecting to the all-orders endpoint.
#[derive(Debug, Clone, Default, Deserialize)]
pub(crate) struct OrderFilter {
    /// Only include orders from these owner addresses.
    /// TODO: implement owner filtering in the WS handler.
    #[expect(
        dead_code,
        reason = "deserialized from client messages, filtering not yet implemented"
    )]
    #[serde(default)]
    pub(crate) owners: Option<Vec<Address>>,
    /// Only include orders with these statuses.
    #[serde(default)]
    pub(crate) statuses: Option<Vec<StatusFilter>>,
}
