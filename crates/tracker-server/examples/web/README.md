# Web Order Tracker

Single-page web app for monitoring Signet orders via the tracker server's WebSocket endpoint. Shows a live table of orders with expandable details — click any row to see full diagnostics or fill info.

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

With the server running, open `index.html` in a browser, enter the server URL, and click Connect. The default URL is `ws://localhost:8019/orders/ws`.
