//! x402 payment facade for Compass.
//!
//! Builds HTTP 402 payment-required responses and (week 5) validates
//! `X-Payment` headers from agents that have settled via USDC on Base.
//!
//! The primary payment rail for production is TAP/GraphTally (GRT, gas-free
//! batch settlement via compass-tap). x402 is the agent-discovery path:
//! agents that don't yet have a TAP signer can pay per-call in USDC on Base,
//! and auto-list on the Coinbase x402 Bazaar at first settlement.

use compass_core::types::PaymentSpec;

/// USDC contract address on Base mainnet.
const USDC_BASE: &str = "0x833589fcd6edb6e08f4c7c32d4f71b54bda02913";

/// Build an x402 `PaymentSpec` for a given MCP tool call.
///
/// `price_grt_wei` is converted to a USDC micro-amount stub (week 5 TODO:
/// use a proper GRT→USDC rate from an on-chain oracle or config).
pub fn payment_spec_for(
    tool_name: &str,
    price_grt_wei: u64,
    indexer_address: &str,
) -> PaymentSpec {
    // Stub conversion: treat 1 GRT wei ≈ 0.1 USDC micro (6 decimals).
    // At $0.10/GRT: 10_000_000_000_000 GRT wei = 0.00001 GRT ≈ $0.000001 USDC = 1 USDC micro.
    // TODO week 5: replace with configurable GRT/USDC rate or Chainlink feed.
    let usdc_micro = price_grt_wei / 10_000_000_000_000;
    let max_amount = usdc_micro.max(1).to_string();

    PaymentSpec {
        scheme: "exact".to_string(),
        network: "base".to_string(),
        max_amount_required: max_amount,
        resource: tool_name.to_string(),
        description: format!("GraphQL query via Compass MCP — tool: {tool_name}"),
        pay_to: indexer_address.to_string(),
        asset: USDC_BASE.to_string(),
    }
}

/// Build the JSON body for an HTTP 402 response.
pub fn payment_required_body(
    tool_name: &str,
    price_grt_wei: u64,
    indexer_address: &str,
) -> serde_json::Value {
    let spec = payment_spec_for(tool_name, price_grt_wei, indexer_address);
    serde_json::json!({
        "x402Version": 1,
        "accepts": [spec],
        "error": "payment required"
    })
}

// TODO week 5: implement validate_x_payment(header: &str, tool_name: &str) -> anyhow::Result<()>
// that verifies an X-Payment header from an agent that has settled via Base USDC.
