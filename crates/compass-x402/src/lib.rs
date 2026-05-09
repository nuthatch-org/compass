//! x402 payment facade for Compass.
//!
//! Builds HTTP 402 payment-required responses and validates `X-Payment` headers
//! from agents that have settled via USDC on Base.
//!
//! The primary payment rail for production is TAP/GraphTally (GRT, gas-free
//! batch settlement via compass-tap). x402 is the agent-discovery path:
//! agents that don't yet have a TAP signer can pay per-call in USDC on Base,
//! and auto-list on the Coinbase x402 Bazaar at first settlement.

use anyhow::Context;
use serde::Deserialize;

use compass_core::types::PaymentSpec;

/// USDC contract address on Base mainnet.
const USDC_BASE: &str = "0x833589fcd6edb6e08f4c7c32d4f71b54bda02913";

/// ERC-20 Transfer event topic: keccak256("Transfer(address,address,uint256)")
const TRANSFER_TOPIC: &str =
    "0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef";

// ---- X-Payment header types -------------------------------------------------

/// Top-level structure of the JSON value in the `X-Payment` header.
#[derive(Debug, Deserialize)]
pub struct XPaymentProof {
    pub scheme: String,
    pub network: String,
    pub payload: XPaymentPayload,
}

/// Inner payload — the agent includes the tx hash of their on-chain USDC transfer.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct XPaymentPayload {
    /// 0x-prefixed transaction hash on Base.
    pub tx_hash: String,
}

// ---- 402 response builder ---------------------------------------------------

/// Convert a GRT wei price to the equivalent USDC micro-amount (6 decimals).
///
/// Stub conversion: 1 GRT wei ≈ 0.1 USDC micro at $0.10/GRT.
/// 10_000_000_000_000 GRT wei (= 0.00001 GRT) → 1 USDC micro (= $0.000001).
///
/// TODO: replace with a configurable GRT/USDC rate or Chainlink feed.
pub fn usdc_micro_for(price_grt_wei: u64) -> u64 {
    (price_grt_wei / 10_000_000_000_000).max(1)
}

/// Build an x402 `PaymentSpec` for a given MCP tool call.
pub fn payment_spec_for(
    tool_name: &str,
    price_grt_wei: u64,
    indexer_address: &str,
) -> PaymentSpec {
    PaymentSpec {
        scheme: "exact".to_string(),
        network: "base".to_string(),
        max_amount_required: usdc_micro_for(price_grt_wei).to_string(),
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

// ---- X-Payment validation ---------------------------------------------------

/// Validate an `X-Payment` header for a USDC-on-Base payment.
///
/// Checks:
///   1. `scheme` == "exact" and `network` == "base".
///   2. The transaction exists on Base and succeeded (status 0x1).
///   3. The receipt contains a USDC Transfer log to `pay_to` with
///      `amount >= required_usdc_micro`.
///
/// `http_client` is the shared reqwest client from AppState (reused for Base RPC).
pub async fn validate_x_payment(
    header: &str,
    required_usdc_micro: u64,
    pay_to: &str,
    base_rpc_url: &str,
    http_client: &reqwest::Client,
) -> anyhow::Result<()> {
    let proof: XPaymentProof =
        serde_json::from_str(header).context("failed to parse X-Payment header JSON")?;

    if proof.scheme != "exact" {
        anyhow::bail!("unsupported x402 scheme: {} (expected 'exact')", proof.scheme);
    }
    if proof.network != "base" {
        anyhow::bail!("unsupported x402 network: {} (expected 'base')", proof.network);
    }

    // --- Fetch transaction receipt from Base via JSON-RPC --------------------

    let rpc_resp: serde_json::Value = http_client
        .post(base_rpc_url)
        .json(&serde_json::json!({
            "jsonrpc": "2.0",
            "method": "eth_getTransactionReceipt",
            "params": [&proof.payload.tx_hash],
            "id": 1
        }))
        .send()
        .await
        .context("Base RPC request failed")?
        .json()
        .await
        .context("Base RPC response parse failed")?;

    let receipt = rpc_resp
        .get("result")
        .filter(|r| !r.is_null())
        .ok_or_else(|| anyhow::anyhow!("transaction not found: {}", proof.payload.tx_hash))?;

    // Check transaction success (status == 0x1).
    let status = receipt.get("status").and_then(|s| s.as_str()).unwrap_or("0x0");
    if !status.eq_ignore_ascii_case("0x1") {
        anyhow::bail!("transaction {} failed or is still pending", proof.payload.tx_hash);
    }

    // --- Scan logs for a USDC Transfer to pay_to -----------------------------

    // Build the expected topic2 (32-byte padded address).
    let pay_to_norm = pay_to.to_lowercase();
    let pay_to_padded = format!(
        "0x000000000000000000000000{}",
        pay_to_norm.trim_start_matches("0x")
    );

    let logs = receipt
        .get("logs")
        .and_then(|l| l.as_array())
        .ok_or_else(|| anyhow::anyhow!("receipt has no logs field"))?;

    for log in logs {
        // Must be emitted by the USDC contract.
        let addr = log.get("address").and_then(|a| a.as_str()).unwrap_or("");
        if !addr.eq_ignore_ascii_case(USDC_BASE) {
            continue;
        }

        let topics = match log.get("topics").and_then(|t| t.as_array()) {
            Some(t) if t.len() >= 3 => t,
            _ => continue,
        };

        let topic0 = topics[0].as_str().unwrap_or("");
        let topic2 = topics[2].as_str().unwrap_or(""); // indexed `to` address

        if !topic0.eq_ignore_ascii_case(TRANSFER_TOPIC) {
            continue;
        }
        if !topic2.eq_ignore_ascii_case(&pay_to_padded) {
            continue;
        }

        // Parse the transfer amount from the log data (ABI-encoded uint256, big-endian).
        let data_hex = log
            .get("data")
            .and_then(|d| d.as_str())
            .unwrap_or("0x")
            .trim_start_matches("0x");

        // Take the low 128 bits (32 hex chars) — USDC supply fits comfortably in u128.
        let amount_hex = if data_hex.len() > 32 {
            &data_hex[data_hex.len() - 32..]
        } else {
            data_hex
        };
        let amount = u128::from_str_radix(amount_hex, 16).unwrap_or(0);

        if amount >= required_usdc_micro as u128 {
            return Ok(());
        }
        anyhow::bail!(
            "USDC transfer amount {} micro < required {} micro",
            amount,
            required_usdc_micro
        );
    }

    anyhow::bail!(
        "no USDC Transfer to {} found in transaction {}",
        pay_to,
        proof.payload.tx_hash
    )
}
