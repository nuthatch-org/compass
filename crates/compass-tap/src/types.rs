use alloy_primitives::{Address, Bytes};
use serde::{Deserialize, Serialize};

/// EIP-712 type string for the TAP v2 Receipt struct.
/// Must match exactly what the deployed GraphTallyCollector uses.
pub const RECEIPT_TYPE_STRING: &str =
    "Receipt(address data_service,address service_provider,uint64 timestamp_ns,uint64 nonce,uint128 value,bytes metadata)";

/// A TAP v2 receipt — one per MCP tool call, signed by the payer.
/// Mirrors the on-chain Solidity struct in GraphTallyCollector.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Receipt {
    pub data_service: Address,
    pub service_provider: Address,
    pub timestamp_ns: u64,
    pub nonce: u64,
    pub value: u128,
    #[serde(default)]
    pub metadata: Bytes,
}

/// An EIP-712 signed receipt, transmitted as JSON in the `tap-receipt` HTTP header.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignedReceipt {
    pub receipt: Receipt,
    /// Hex-encoded 65-byte ECDSA signature: r(32) || s(32) || v(1).
    pub signature: String,
}
