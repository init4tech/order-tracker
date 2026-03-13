# Signet Tracker

**Keep this file and the readme up to date.** After any change to the repo (new files, renamed modules, added dependencies, changed conventions, etc.), update the relevant sections of this document and the README before finishing.

Cargo workspace for tracking the lifecycle and diagnosing fill failures of Signet orders, plus an HTTP/WebSocket API server for subscribing to order status updates.

## Project Structure

```
Cargo.toml - Workspace root (members: crates/*)

crates/tracker/ - signet-tracker: pure tracking library (no IO, no server deps)
  src/lib.rs - Library root, module exports
  src/error.rs - Error enum (thiserror, #[from] conversions)
  src/order_status.rs - OrderReport, OrderStatus enum (Pending, Filled, Expired), and FillInfo
  src/order_diagnostics.rs - OrderDiagnostics report with per-check result types
  src/order_tracker.rs - OrderTracker<RuP, HostP>: single public entry point (`status`)
  src/fill_search.rs - Filled event scanning, output matching, ERC-20 balance/allowance queries

crates/tracker-server/ - signet-tracker-server: HTTP/WS API server (lib + bin)
  src/lib.rs - Server library root, signal handling, config_from_env
  src/main.rs - Binary entry point
  src/config.rs - TrackerConfig (FromEnv + Init4Config), provider type aliases, connect methods
  src/initialization.rs - Provider and tx-cache connection with exponential backoff retry
  src/service.rs - Axum HTTP server with graceful shutdown via CancellationToken
```

## Build & Run

- **Rust edition**: 2024, MSRV 1.88
- **Build**: `cargo build`
- **Test**: `cargo t`
- **Lint**: `cargo clippy --all-features --all-targets` and `cargo clippy --no-default-features --all-targets`
- **Formatting**: `cargo +nightly fmt` (uses `rustfmt.toml` with `reorder_imports`, `use_field_init_shorthand`, `use_small_heuristics = "Max"`)

## Key Dependencies

### signet-tracker (library)
- **signet-sdk crates** (`signet-constants`, `signet-orders`, `signet-tx-cache`, `signet-types`, `signet-zenith`): Signet chain types, order signing, Permit2 nonce checks, tx-cache client
- **alloy**: Ethereum provider/types, contract calls, event log queries
- **thiserror**: Library error types
- **tracing**: Instrumentation
- **serde**: Serializable output types for consumer rendering

### signet-tracker-server (API server)
- **signet-tracker**: The tracking library
- **init4-bin-base**: Config loading (FromEnv derive), tracing/metrics init, provider config types
- **alloy**: RPC providers
- **axum**: HTTP server
- **backon**: Exponential backoff retry for provider/tx-cache connections
- **tokio** + **tokio-util**: Async runtime, signal handling, CancellationToken

## Conventions

- All output types implement `Serialize` so consumers can render as JSON, display in a GUI, etc.
- `OrderTracker` is generic over `RuP: Provider` and `HostP: Provider` — no concrete provider types
- Public API is minimal: `OrderTracker::new` and `OrderTracker::status` only — diagnostics are always run internally and returned alongside the derived status in `OrderReport`
- The `fill_search` module is `pub(crate)` — its ERC-20 helpers are used by the tracker but not part of the public API
- Workspace dependencies are declared in the root `Cargo.toml` and inherited by crates via `.workspace = true`
- No global static config — `config_from_env()` returns the config and OTLP guard; the binary owns them on the stack
- Provider connections use exponential backoff retry (following the pattern from `init4/filler`)
- Graceful shutdown via `CancellationToken` cancelled on SIGINT/SIGTERM
