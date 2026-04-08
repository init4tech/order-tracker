//! Order lifecycle tracking and diagnostics for Signet.
//!
//! Provides tools to determine the status of a Signet order and diagnose why an unfilled order was
//! not filled. This is a pure library with no IO concerns — consumers (CLIs, GUIs, web apps)
//! provide their own providers and render the structured output however they choose.

#![cfg_attr(not(test), warn(unused_crate_dependencies))]
#![cfg_attr(docsrs, feature(doc_cfg, doc_auto_cfg))]

mod amount;
pub use amount::Amount;

mod error;
pub use error::Error;

mod order_status;
pub use order_status::{Chain, ChainTransaction, FillInfo, FillOutput, OrderStatus};

mod order_diagnostics;
pub use order_diagnostics::{
    AllowanceCheck, AllowanceChecks, BalanceCheck, BalanceChecks, DeadlineCheck, MaybeBool,
    OrderDiagnostics,
};

mod order_tracker;
pub use order_tracker::OrderTracker;

mod pretty_duration;
pub use pretty_duration::PrettyDuration;

mod timestamp;
pub use timestamp::Timestamp;

mod fill_search;
pub use fill_search::fill_outputs_are_superset_of_order_outputs;

mod token_symbol_cache;
