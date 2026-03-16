/// In-memory store of recent chain events.
pub(crate) mod event_store;
/// Central state manager processing loop.
pub(crate) mod state_manager;
/// Per-order lifecycle state.
pub(crate) mod tracked_order;
