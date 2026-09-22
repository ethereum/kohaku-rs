use alloy::{
    dyn_abi::Eip712Domain,
    primitives::{address, Address},
    sol_types::eip712_domain,
};

/// The canonical EntryPoint address for 4337 v0.8
pub const ENTRY_POINT_08: Address = address!("0x4337084D9E255Ff0702461CF8895CE9E3b5Ff108");

/// The EIP-712 domain for 4337 v0.8, with the given chain ID.
pub(crate) const fn entry_point_08_domain(chain_id: u64) -> Eip712Domain {
    entry_point_domain(chain_id, ENTRY_POINT_08)
}

/// EIP-712 domain for ERC-4337 v0.8 against an arbitrary EntryPoint.
#[must_use]
pub const fn entry_point_domain(chain_id: u64, verifying_contract: Address) -> Eip712Domain {
    eip712_domain! {
        name: "ERC4337",
        version: "1",
        chain_id: chain_id,
        verifying_contract: verifying_contract,
    }
}
