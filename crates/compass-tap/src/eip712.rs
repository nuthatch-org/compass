use alloy_primitives::{keccak256, Address, B256, U256};
use alloy_sol_types::SolValue;
use k256::ecdsa::{RecoveryId, Signature as K256Sig, VerifyingKey};

use crate::types::{Receipt, RECEIPT_TYPE_STRING};

/// Compute the EIP-712 domain separator for a GraphTallyCollector deployment.
///
/// Standard values for Arbitrum One:
///   name = "GraphTallyCollector", chain_id = 42161,
///   verifying_contract = 0x8f69F5C07477Ac46FBc491B1E6D91E2bb0111A9e
pub fn domain_separator(name: &str, chain_id: u64, verifying_contract: Address) -> B256 {
    let type_hash = keccak256(
        b"EIP712Domain(string name,string version,uint256 chainId,address verifyingContract)",
    );
    let encoded = (
        type_hash,
        keccak256(name.as_bytes()),
        keccak256(b"1"),
        U256::from(chain_id),
        verifying_contract,
    )
        .abi_encode();
    keccak256(&encoded)
}

/// Compute the full EIP-712 message hash for a receipt.
/// hash = keccak256(0x1901 || domainSeparator || structHash)
pub fn eip712_hash(domain_sep: B256, receipt: &Receipt) -> B256 {
    let struct_hash = receipt_struct_hash(receipt);
    let mut buf = [0u8; 66];
    buf[0] = 0x19;
    buf[1] = 0x01;
    buf[2..34].copy_from_slice(domain_sep.as_slice());
    buf[34..66].copy_from_slice(struct_hash.as_slice());
    keccak256(buf)
}

/// Recover the Ethereum address that produced `sig_hex` over `hash`.
/// `sig_hex` must be a 65-byte r||s||v signature in hex (with or without 0x prefix).
/// `v` is accepted as either raw recovery id (0/1) or Ethereum-style (27/28).
pub fn recover_signer(hash: B256, sig_hex: &str) -> anyhow::Result<Address> {
    let bytes = hex::decode(sig_hex.trim_start_matches("0x"))?;
    anyhow::ensure!(
        bytes.len() == 65,
        "signature must be 65 bytes, got {}",
        bytes.len()
    );
    let v = bytes[64];
    let rec_id_byte = if v >= 27 { v - 27 } else { v };
    let rec_id = RecoveryId::from_byte(rec_id_byte)
        .ok_or_else(|| anyhow::anyhow!("invalid recovery id {v}"))?;
    let sig = K256Sig::from_slice(&bytes[..64])?;
    let vk = VerifyingKey::recover_from_prehash(hash.as_slice(), &sig, rec_id)?;
    let encoded = vk.to_encoded_point(false);
    let pubkey_hash = keccak256(&encoded.as_bytes()[1..]);
    Ok(Address::from_slice(&pubkey_hash[12..]))
}

/// Derive the Ethereum address corresponding to a signing key.
pub fn address_from_key(key: &k256::ecdsa::SigningKey) -> Address {
    let vk = VerifyingKey::from(key);
    let encoded = vk.to_encoded_point(false);
    let pubkey_hash = keccak256(&encoded.as_bytes()[1..]);
    Address::from_slice(&pubkey_hash[12..])
}

/// Compute the EIP-712 struct hash for a receipt.
fn receipt_struct_hash(r: &Receipt) -> B256 {
    let type_hash = keccak256(RECEIPT_TYPE_STRING.as_bytes());
    let encoded = (
        type_hash,
        r.data_service,
        r.service_provider,
        r.timestamp_ns,
        r.nonce,
        r.value,
        keccak256(&r.metadata),
    )
        .abi_encode();
    keccak256(&encoded)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eip712_hash_matches_dispatch_tap_golden() {
        // Golden value from dispatch-tap's cross-language EIP-712 test.
        let verifying_contract = Address::from([0x12u8; 20]);
        let dom = domain_separator("TAP", 31337, verifying_contract);
        let receipt = Receipt {
            data_service: Address::from([0x01u8; 20]),
            service_provider: Address::from([0x02u8; 20]),
            timestamp_ns: 1_000_000_000,
            nonce: 42,
            value: 1_000_000_000_000_000_000,
            metadata: Default::default(),
        };
        let hash = eip712_hash(dom, &receipt);
        let expected: B256 =
            "0x6a496be73e1ebc77612afedde0307b2099cc116600e590ce743771770f85d5ba"
                .parse()
                .unwrap();
        assert_eq!(hash, expected);
    }

    #[test]
    fn domain_separator_differs_by_chain() {
        let addr = Address::from([0x12u8; 20]);
        assert_ne!(domain_separator("TAP", 1, addr), domain_separator("TAP", 42161, addr));
    }
}
