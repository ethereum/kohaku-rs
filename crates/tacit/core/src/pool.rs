use alloy::primitives::{Address, address};
use ruint::aliases::U256;
use serde::{Deserialize, Serialize};

use crate::statement::pool_asset;

/// A relayer that submits spends and is paid from the shielded funds in the same transaction.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RelayerConfig {
    /// The relayer's base URL, `…/evm-pool/keeper`.
    pub url: String,
    /// The address the relayer must quote from; a quote from any other address is refused.
    pub address: Address,
    /// The most a relayed spend may pay the relayer, in wei; a dearer quote is refused.
    pub max_fee: u128,
}

/// One deployment of the Tacit pool.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Pool {
    pub chain_id: u64,
    /// `TacitEvmPool`, native ETH.
    pub address: Address,
    /// The block the pool was deployed in; history is read from here.
    pub deploy_block: u64,
    /// History is kept once it is this many blocks deep.
    pub confirmations: u64,
    /// Blocks per `eth_getLogs` request to start with; narrowed when a node refuses a range.
    pub log_chunk: u64,
    pub relayer: Option<RelayerConfig>,
}

impl Pool {
    /// The pool on Ethereum mainnet: one immutable contract with no owner, pause or upgrade.
    #[must_use]
    pub fn ethereum() -> Self {
        Self {
            chain_id: 1,
            address: address!("0x000000c2A20657CE25f2Ba99737933D031AFBEE9"),
            deploy_block: 26_069_245,
            confirmations: 12,
            log_chunk: 5_000,
            relayer: Some(RelayerConfig {
                url: "https://tacit-evm-pool-keeper.onrender.com/evm-pool/keeper".into(),
                address: address!("0x7c9f8aE4e48Cbb2727F95b6477a1cf92bCFc43D0"),
                max_fee: 5 * 10_u128.pow(16),
            }),
        }
    }

    /// A deployment at `address` on `chain_id`, without a relayer.
    #[must_use]
    pub fn new(chain_id: u64, address: Address, deploy_block: u64) -> Self {
        Self {
            chain_id,
            address,
            deploy_block,
            confirmations: 12,
            log_chunk: 5_000,
            relayer: None,
        }
    }

    #[must_use]
    pub fn with_relayer(mut self, relayer: Option<RelayerConfig>) -> Self {
        self.relayer = relayer;
        self
    }

    #[must_use]
    pub fn with_confirmations(mut self, confirmations: u64) -> Self {
        self.confirmations = confirmations;
        self
    }

    #[must_use]
    pub fn with_log_chunk(mut self, blocks: u64) -> Self {
        self.log_chunk = blocks.max(1);
        self
    }

    /// The pool's asset id, an input of every proof: ETH, bound to this chain and pool.
    #[must_use]
    pub fn asset(&self) -> U256 {
        pool_asset(self.chain_id, self.address, Address::ZERO)
    }
}
