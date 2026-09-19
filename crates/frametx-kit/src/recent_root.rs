use alloy::primitives::{Address, B256, Bytes, keccak256};

use crate::gas::{RECENT_ROOT_TUPLE_BYTES, RECENT_ROOT_WINDOW};

/// `keccak256(address20(pool) || bytes32(epoch))` — EIP-8272 source id.
#[must_use]
pub fn source_id(pool: Address, epoch: u64) -> B256 {
    let mut buf = [0u8; 52];
    buf[..20].copy_from_slice(pool.as_slice());
    buf[44..].copy_from_slice(&epoch.to_be_bytes());
    keccak256(buf)
}

/// Pack `source_id(32) || uint64_be(slot) || root(32)`.
#[must_use]
pub fn recent_root_tuple(source: B256, slot: u64, root: B256) -> [u8; RECENT_ROOT_TUPLE_BYTES] {
    let mut out = [0u8; RECENT_ROOT_TUPLE_BYTES];
    out[..32].copy_from_slice(source.as_slice());
    out[32..40].copy_from_slice(&slot.to_be_bytes());
    out[40..].copy_from_slice(root.as_slice());
    out
}

#[must_use]
pub fn recent_root_tuple_bytes(source: B256, slot: u64, root: B256) -> Bytes {
    Bytes::copy_from_slice(&recent_root_tuple(source, slot, root))
}

/// Why a publication `slot` would be refused at `latest_slot`, or `None` if usable.
///
/// EIP-8272 judges against the earliest block that could carry the tx, so
/// `current_slot = latest_slot + 1`.
#[must_use]
pub fn recent_root_window_error(slot: u64, latest_slot: u64) -> Option<String> {
    let current_slot = latest_slot.saturating_add(1);
    if slot >= current_slot {
        return Some(format!(
            "recent-root ref is not yet referenceable: publication slot {slot} is not earlier than current slot {current_slot}"
        ));
    }
    if current_slot.saturating_sub(slot) >= RECENT_ROOT_WINDOW {
        return Some(format!(
            "recent-root ref expired: publication slot {slot} is outside the {RECENT_ROOT_WINDOW}-slot window at current slot {current_slot}"
        ));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tuple_is_72_bytes() {
        let t = recent_root_tuple(B256::ZERO, 1, B256::ZERO);
        assert_eq!(t.len(), 72);
        assert_eq!(&t[32..40], &1u64.to_be_bytes());
    }

    #[test]
    fn window_rejects_same_slot() {
        assert!(recent_root_window_error(11, 10).is_some());
        assert!(recent_root_window_error(10, 10).is_none());
    }
}
