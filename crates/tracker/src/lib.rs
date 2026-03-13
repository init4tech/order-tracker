//! Order lifecycle tracking and diagnostics for Signet.
//!
//! Provides tools to determine the status of a Signet order and diagnose why an unfilled order was
//! not filled. This is a pure library with no IO concerns — consumers (CLIs, GUIs, web apps)
//! provide their own providers and render the structured output however they choose.

#![warn(
    missing_copy_implementations,
    missing_debug_implementations,
    missing_docs,
    unreachable_pub,
    clippy::missing_const_for_fn,
    rustdoc::all
)]
#![cfg_attr(not(test), warn(unused_crate_dependencies))]
#![deny(unused_must_use, rust_2018_idioms)]
#![cfg_attr(docsrs, feature(doc_cfg, doc_auto_cfg))]

mod amount;
pub use amount::Amount;

mod error;
pub use error::Error;

mod order_status;
pub use order_status::{Chain, ChainTransaction, FillInfo, FillOutput, OrderReport, OrderStatus};

mod order_diagnostics;
pub use order_diagnostics::{
    AllowanceCheck, BalanceCheck, DeadlineCheck, OrderDiagnostics, TokenAllowance, TokenBalance,
};

mod order_tracker;
pub use order_tracker::OrderTracker;

mod timestamp;
pub use timestamp::Timestamp;

mod fill_search;
mod token_symbol_cache;
