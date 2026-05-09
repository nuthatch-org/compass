# Compass

**A decentralised Subgraph-MCP Gateway as a Horizon data service.**

Compass turns every subgraph an indexer serves into a discoverable, pay-per-call [MCP](https://modelcontextprotocol.io) tool — settled in GRT via [TAP v2 / GraphTally](https://thegraph.com/blog/graphtally-brings-improved-verification-to-the-graph/) or in USDC via [x402](https://x402.org). Any AI agent that speaks MCP (Claude, Cursor, OpenAI Agents, Eliza, etc.) can query 50,000+ subgraphs for under a cent each, with no API key and no central operator.

```
Agent (Claude / Cursor / OpenAI Agents)
    │
    │  POST /   {"method": "tools/call", "params": {"name": "ens_mainnet", "arguments": {"query": "..."}}}
    │  tap-receipt: {"receipt": {...}, "signature": "0x..."}
    ▼
compass-gateway (this repo — Rust, axum)
    │  validate TAP receipt (EIP-712, compass-tap)
    │  OR: return HTTP 402 with x402 payment spec (compass-x402)
    ▼
graph-node (local, same indexer host)
    │  POST /subgraphs/id/{deploymentId}
    ▼
Subgraph data  →  back to agent as MCP tool result
```

Indexers stake GRT into `MCPDataService.sol` on Arbitrum One and collect query fees by submitting signed RAVs — the same [Horizon](https://thegraph.com/blog/the-graph-horizon/) settlement flow used by SubgraphService and RPCDataService (Dispatch).

---

## Status

> **Week 1 — scaffold complete.** The Rust workspace compiles, EIP-712 receipt validation is tested, and `MCPDataService.sol` is written. Contract deployment and end-to-end MCP demo coming in week 3.

| Week | Milestone |
|------|-----------|
| 1–2  | `MCPDataService.sol` Foundry tests + Arbitrum Sepolia deploy |
| 3–4  | `tools/list` + `tools/call` working end-to-end; Claude Desktop demo |
| 5    | x402 facade + Coinbase Bazaar auto-listing |
| 6    | Auto-derived per-entity MCP tools from `__schema` introspection |
| 7    | `compass-cli` (`compass run`, `compass register`) |
| 8    | Launch: GRC-007 forum post, Bazaar listing, blog post |

---

## How it works

### For agents

Compass exposes a standard [MCP Streamable HTTP](https://modelcontextprotocol.io/specification/2025-03-26/basic/transports#streamable-http) endpoint. Point any MCP-compatible client at it:

```json
// tools/list — discover available subgraphs
POST https://compass.your-indexer.example.com/
{"jsonrpc": "2.0", "id": 1, "method": "tools/list"}

// tools/call — query a subgraph (TAP receipt required)
POST https://compass.your-indexer.example.com/
tap-receipt: {"receipt": {"data_service": "0x...", "service_provider": "0x...", "timestamp_ns": 1234567890, "nonce": 1, "value": 10000000000000, "metadata": "0x"}, "signature": "0x..."}
{"jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": {"name": "ens_mainnet", "arguments": {"query": "{ domains(first: 5) { name labelName } }"}}}
```

No receipt? The server returns HTTP 402 with an [x402](https://x402.org) payment spec so the agent knows exactly how to pay.

### For indexers

Each subgraph in `compass.toml` becomes one MCP tool. Agents query it; you collect GRT. The only new process is `compass-gateway` — a lightweight Rust sidecar that connects to your existing `graph-node` and validates TAP receipts before forwarding queries.

---

## Quickstart (indexers)

### Prerequisites

- An existing Graph Protocol indexer running [graph-node](https://github.com/graphprotocol/graph-node)
- GRT provisioned to `MCPDataService.sol` (see [Contract Deployment](#contract-deployment))
- Docker + Docker Compose

### 1. Configure

```bash
cp config.example.toml compass.toml
```

Edit `compass.toml`:

```toml
[gateway]
indexer_address = "0xYOUR_INDEXER_STAKING_ADDRESS"
endpoint_url    = "https://compass.your-indexer.example.com"

[tap]
data_service_address = "0xMCPDATASERVICE_PROXY_ADDRESS"  # after deploy

[graph_node]
url = "http://graph-node:8000"   # container name if running in same docker stack

[[subgraphs]]
deployment_id = "QmYourDeploymentCID..."
name          = "ens_mainnet"
description   = "ENS name resolution on Ethereum mainnet."
network       = "mainnet"
```

### 2. Run

```bash
docker compose up -d
```

The gateway starts on port **8091** (host) / **8090** (container).

If your `graph-node` runs in the StakeSquid docker stack, Compass joins `graphprotocol-mainnet-docker_default` automatically. Verify the network name first:

```bash
docker network ls | grep graph
```

### 3. Test

```bash
# Health
curl http://localhost:8091/health

# List tools (no payment required)
curl -s -X POST http://localhost:8091/ \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/list"}' | jq .

# Call a tool without a receipt — expect HTTP 402
curl -v -X POST http://localhost:8091/ \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"ens_mainnet","arguments":{"query":"{ domains(first:3){name} }"}}}'
```

---

## Contract Deployment

```bash
cd contracts

# Install dependencies (same libs as Dispatch / drpc-service)
forge install graphprotocol/contracts OpenZeppelin/openzeppelin-contracts OpenZeppelin/openzeppelin-contracts-upgradeable

# Deploy to Arbitrum Sepolia
DEPLOYER_PRIVATE_KEY=0x... \
ARBITRUM_SEPOLIA_RPC_URL=https://... \
ARBISCAN_API_KEY=... \
forge script script/Deploy.s.sol --rpc-url arbitrum_sepolia --broadcast --verify -vvvv
```

Copy the printed proxy address into `compass.toml` as `tap.data_service_address`.

---

## Development

```bash
# Build
cargo build

# Test (includes EIP-712 cross-language vector)
cargo test

# Run locally (with a compass.toml pointing at a live graph-node)
RUST_LOG=compass_gateway=debug cargo run --bin compass-gateway
```

### Workspace crates

| Crate | Purpose |
|-------|---------|
| `compass-core` | Config, shared types (`McpTool`, `PaymentSpec`), errors |
| `compass-tap` | TAP v2 EIP-712 receipt validation (ported from [dispatch-tap](https://github.com/cargopete/drpc-service)) |
| `compass-mcp` | MCP Streamable HTTP JSON-RPC 2.0 protocol types |
| `compass-x402` | HTTP 402 builder; x402 payment spec for agent-discovery path |
| `compass-schema` | GraphQL `__schema` → MCP tool schema derivation (week 6) |
| `compass-gateway` | Main binary: axum server, TAP validation, graph-node forwarding |

---

## Payment flow

```
Agent signs TAP receipt (GRT wei, EIP-712)
    └── tap-receipt header on tools/call
            └── compass-gateway validates signature + value + addresses
                    └── forwards GraphQL to graph-node
                            └── returns result

Indexer accumulates receipts → submits RAV → MCPDataService.collect()
    └── GraphTallyCollector → PaymentsEscrow → GraphPayments
            └── GRT distributed: protocol tax / data service cut / delegator cut / indexer
```

For agents without a TAP signer: compass-gateway returns HTTP 402 with an x402 USDC-on-Base payment spec. After the agent pays, Compass settles in GRT (week 5).

---

## Relation to existing Horizon data services

Compass is the third community-built Horizon data service alongside Dispatch (JSON-RPC, GRC-005) and Mainline (Firehose, GRC-006). It reuses the same `RPCDataService.sol` pattern and `tap-rs` receipt types — `MCPDataService.sol` is ~60 lines of delta from `RPCDataService.sol`.

GRC-007 forum post: coming at launch.

---

## License

Apache-2.0
