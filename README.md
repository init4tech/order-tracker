# Signet Order Tracker

Tracks the lifecycle and diagnoses fill failures of Signet orders. Includes a pure tracking library and an HTTP API server.

## Crates

- **signet-tracker** (`crates/tracker/`) — Pure library for order status checks and diagnostics. No IO, no server deps.
- **signet-tracker-server** (`crates/tracker-server/`) — HTTP API server exposing order status via REST (and eventually WebSocket/SSE).

## Usage

```bash
HOST_RPC_URL=https://host-rpc.parmigiana.signet.sh \
ROLLUP_RPC_URL=https://rpc.parmigiana.signet.sh \
TX_POOL_URL=https://transactions.parmigiana.signet.sh \
CHAIN_NAME=parmigiana \
cargo run -p signet-tracker-server
```

Query an order:

```bash
curl http://localhost:8019/orders/0x<order_hash>
```

Run `--help` for the full list of environment variables.

## TODO

- [ ] Add WebSocket endpoint to subscribe to a single order's status (terminates on fill/expiry)
- [ ] Add WebSocket endpoint to subscribe to all orders from the point of subscription (never terminates - can apply filter)
- [ ] Fallback for orders not in tx-cache: scan `Order` events on the rollup, fetch `initiatePermit2` tx calldata, reconstruct the `SignedOrder`, recompute the order hash to find a match, then run diagnostics from the recovered order
- [ ] Full repo review
- [ ] Tests
