//! TAP v2 (GraphTally) receipt validation for the Compass MCP gateway.
//!
//! Ported from dispatch-tap; this is the indexer/service side — it validates
//! incoming signed receipts from payers before forwarding queries to graph-node.
//!
//! On-chain escrow authorisation is enforced by MCPDataService.collect() when
//! the indexer submits a RAV — not here. This crate only checks that the
//! receipt is cryptographically valid and meets the minimum price.

pub mod eip712;
pub mod types;

pub use eip712::{address_from_key, domain_separator, eip712_hash, recover_signer};
pub use types::{Receipt, SignedReceipt, RECEIPT_TYPE_STRING};

use alloy_primitives::{Address, B256};

/// Validate a signed TAP receipt on the indexer (service) side.
///
/// Checks:
///   1. `data_service` matches the MCPDataService contract address.
///   2. `service_provider` matches this indexer's staking address.
///   3. `value` >= `min_value` (the configured price for this subgraph query).
///   4. EIP-712 signature is valid (signature was not tampered).
pub fn verify_receipt(
    signed: &SignedReceipt,
    domain_sep: B256,
    expected_data_service: Address,
    expected_service_provider: Address,
    min_value: u64,
) -> anyhow::Result<()> {
    if signed.receipt.data_service != expected_data_service {
        anyhow::bail!(
            "receipt data_service {} != expected {}",
            signed.receipt.data_service,
            expected_data_service,
        );
    }
    if signed.receipt.service_provider != expected_service_provider {
        anyhow::bail!(
            "receipt service_provider {} != expected {}",
            signed.receipt.service_provider,
            expected_service_provider,
        );
    }
    if (signed.receipt.value as u64) < min_value {
        anyhow::bail!(
            "receipt value {} < required minimum {}",
            signed.receipt.value,
            min_value,
        );
    }
    // Recover signer — verifies signature integrity. Any valid payer is acceptable here;
    // escrow authorisation is checked at on-chain collect() time.
    let hash = eip712_hash(domain_sep, &signed.receipt);
    recover_signer(hash, &signed.signature)?;
    Ok(())
}
