use alloy::primitives::Address;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pool {
    pub chain_id: u64,
    pub address: Address,
    pub factory: Address,
    pub deployed_block: u64,
}

impl Pool {
    #[must_use]
    pub fn id(&self) -> String {
        format!("{}-{}", self.chain_id, self.address)
    }
}
