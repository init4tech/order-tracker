# TUI Order Tracker

Terminal UI for monitoring Signet orders via the tracker server's WebSocket endpoint. Shows a live, navigable list of orders with status and diagnostics.

## Prerequisites

The tracker server must be running. It requires several environment variables (example values for Parmigiana testnet):

```sh
export HOST_RPC_URL=https://host-rpc.parmigiana.signet.sh
export ROLLUP_RPC_URL=wss://rpc.parmigiana.signet.sh
export TX_POOL_URL=https://transactions.parmigiana.signet.sh
export CHAIN_NAME=parmigiana
# optional: TRACKER_PORT (defaults to 8019)

cargo run -p signet-tracker-server
```

## Usage

With the server running:

```sh
cargo run -p signet-tracker-server --example tui
```

Connects to `ws://localhost:8019/orders/ws` by default. Pass a custom URL as an argument:

```sh
cargo run -p signet-tracker-server --example tui -- ws://some-host:8019/orders/ws
```

## Controls

- `↑`/`↓` or `k`/`j` — navigate the order list
- `q` or `Esc` — quit
