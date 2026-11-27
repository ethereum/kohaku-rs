use alloy::{
    network::TransactionBuilder,
    primitives::{Address, U256, address},
    rpc::types::TransactionRequest,
    sol_types::SolCall,
};
use pqsa_core::StealthScheme;
use pqsa_per_payment::SchemeId3;

use crate::{
    abis::{IERC5564Announcer, IERC6538Registry},
    scheme3::{Announcement, MetaAddress},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Deployment {
    pub chain_id: u64,
    pub announcer: Address,
    pub registry: Address,
    pub start_block: u64,
}

impl Deployment {
    pub const ETHEREUM: Deployment = Deployment {
        chain_id: 1,
        announcer: address!("0x55649E01B5Df198D18D95b5cc5051630cfD45564"),
        registry: address!("0x6538E6bf4B0eBd30A8Ea093027Ac2422ce5d6538"),
        start_block: 20_042_207,
    };

    pub const SEPOLIA: Deployment = Deployment {
        chain_id: 11_155_111,
        announcer: address!("0x55649E01B5Df198D18D95b5cc5051630cfD45564"),
        registry: address!("0x6538E6bf4B0eBd30A8Ea093027Ac2422ce5d6538"),
        start_block: 5_486_597,
    };
}

/// Builds an unsigned ERC-6538 `registerKeys` transaction.
#[must_use]
pub fn register_keys(deployment: &Deployment, meta_address: &MetaAddress) -> TransactionRequest {
    let call = IERC6538Registry::registerKeysCall {
        schemeId: U256::from(SchemeId3::SCHEME_ID),
        stealthMetaAddress: SchemeId3::meta_to_bytes(meta_address).into(),
    };
    TransactionRequest::default()
        .with_chain_id(deployment.chain_id)
        .with_to(deployment.registry)
        .with_input(call.abi_encode())
}

/// Builds an unsigned ERC-5564 announcement transaction.
#[must_use]
pub fn announce(deployment: &Deployment, announcement: &Announcement) -> TransactionRequest {
    let (address, ephemeral_public_key, metadata) = SchemeId3::announcement_to_bytes(announcement);
    let call = IERC5564Announcer::announceCall {
        schemeId: U256::from(SchemeId3::SCHEME_ID),
        stealthAddress: Address::from(address),
        ephemeralPubKey: ephemeral_public_key.into(),
        metadata: metadata.into(),
    };
    TransactionRequest::default()
        .with_chain_id(deployment.chain_id)
        .with_to(deployment.announcer)
        .with_input(call.abi_encode())
}
