# Signet Order Tracker

Tracks the lifecycle of Signet orders to help diagnose fill failures, with an HTTP/WebSocket API for subscribing to order status updates.

The tracker monitors the transaction cache for new orders, watches rollup chain events for fills and initiations, polls the host chain for block progression, and runs diagnostics (deadline checks, balance/allowance checks, Permit2 nonce verification) on each block tick. When an order's status changes, all subscribed WebSocket clients are notified.

## Crates

### `signet-tracker`

Tracking library that provides `OrderTracker` for querying order status. Performs on-chain diagnostics via the alloy `Provider` trait (fill detection, deadline checks, balance/allowance verification, Permit2 nonce checks) and fetches orders from the transaction cache.

All output types implement `Serialize` for JSON rendering. The single output type `OrderStatus` has three variants — `Pending`, `Filled`, and `Expired` — each containing the order hash and relevant diagnostic fields.

### `signet-tracker-server`

HTTP/WebSocket API server built on axum. Connects to rollup and host chain RPC endpoints and the transaction cache, then serves order status via REST and streaming WebSocket endpoints.

Background tasks handle:
- **Block watching** — subscribes to rollup blocks via WebSocket, polls the host chain via HTTP until it advances to match
- **Event watching** — subscribes to `Filled` and `Order` log events on the rollup chain
- **Order discovery** — polls the transaction cache for new orders
- **State management** — central loop that processes events, tracks orders, re-runs diagnostics on each block, and broadcasts status updates

## API

### `GET /orders/{order_hash}`

Full on-demand diagnostics for a single order. Looks the order up in the transaction cache and returns an `OrderStatus` JSON object. Returns 404 if the order is not currently in the cache.

#### Example
```
curl -s http://localhost:8019/orders/<ORDER-HASH> | jq
```

---

### `GET /orders/{order_hash}/ws`

WebSocket subscription for a single order. Looks the order up in the transaction cache, sends the initial status snapshot, then pushes updates on each change. Closes when the order reaches a terminal state (filled or expired).

#### Example
```
websocat -E ws://localhost:8019/orders/<ORDER-HASH>/ws
```

---

### `GET /orders/ws`

WebSocket subscription for all orders. The client may send a JSON filter at any time to control which updates are forwarded:

#### Example
```
websocat -E ws://localhost:8019/orders/ws
```
To set a filter for just pending and filled for example:
```json
{"statuses": ["pending", "filled"]}
```

---

### `GET /healthcheck`

Returns `{"status": "ok"}`.

---

## Example Output

<details>
<summary>Pending order</summary>

```json
{
  "status": "pending",
  "order_hash": "0xba9011549e09027404f1dc1ff52e264043127eb916c35430b21e0af7962af68e",
  "is_in_cache": "true",
  "deadline_check": {
    "expires_in": "9m 44s",
    "deadline": {
      "secs_since_epoch": 1773671150,
      "utc": "2026-03-16T14:25:50Z"
    },
    "checked_at": {
      "secs_since_epoch": 1773670566,
      "utc": "2026-03-16T14:16:06Z"
    }
  },
  "allowance_checks": {
    "all_sufficient": "true",
    "checked_at": {
      "secs_since_epoch": 1773670566,
      "utc": "2026-03-16T14:16:06Z"
    },
    "checks": [
      {
        "sufficient": true,
        "token_contract": "0x0000000000000000007369676e65742d77657468",
        "token_symbol": "WETH",
        "allowance": {
          "raw": "0xffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
          "decimal": "115792089237316195423570985008687907853269984665640564039457584007913129639935"
        },
        "required": {
          "raw": "0x186a1",
          "decimal": "100001"
        }
      }
    ]
  },
  "balance_checks": {
    "all_sufficient": "true",
    "checked_at": {
      "secs_since_epoch": 1773670566,
      "utc": "2026-03-16T14:16:06Z"
    },
    "checks": [
      {
        "sufficient": true,
        "token_contract": "0x0000000000000000007369676e65742d77657468",
        "token_symbol": "WETH",
        "balance": {
          "raw": "0x56b2e99e471505253",
          "decimal": "99956999985967419987"
        },
        "required": {
          "raw": "0x186a1",
          "decimal": "100001"
        }
      }
    ]
  }
}
```

</details>

<details>
<summary>Filled order</summary>

```json
{
  "status": "filled",
  "order_hash": "0xba9011549e09027404f1dc1ff52e264043127eb916c35430b21e0af7962af68e",
  "fill_info": {
    "block_number": 702923,
    "rollup_initiation_tx": "0x0e7921a5680474c274c3b1566118cd03757a5f70dea3d26079ca783022d9c0b0",
    "fill_tx": {
      "chain": "host",
      "tx_hash": "0x10981db211b027c0e023ab8871d1c3d530d2adcf6186de72c90cd2048fe8380d"
    },
    "outputs": [
      {
        "token_contract": "0xd1278f17e86071f1e658b656084c65b7fd3c90ef",
        "token_symbol": "WETH",
        "amount": {
          "raw": "0x186a1",
          "decimal": "100001"
        },
        "recipient": "0x1b97c846b67196658af5c4a0c549892e9f0cf708",
        "chain": "rollup"
      }
    ]
  }
}
```

</details>

## Configuration

Configuration is via environment variables. Run with `-h` or `--help` to see the full list.

| Variable | Description | Default |
|----------|-------------|---------|
| `HOST_RPC_URL` | URL for host chain RPC node (HTTP) | N/A |
| `ROLLUP_RPC_URL` | URL for rollup RPC node (WebSocket) | N/A |
| `TX_POOL_URL` | URL of the transaction cache | N/A |
| `TRACKER_PORT` | Port for the HTTP/WS server | `8019` |
| `CHAIN_NAME` | Signet chain name | N/A |

## Run

```
RUST_LOG=init4=debug,signet=debug,info cargo run -p signet-tracker-server
```

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or [MIT License](LICENSE-MIT) at your option.
