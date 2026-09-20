use alloy::{network::TransactionBuilder, rpc::types::TransactionRequest, sol_types::SolCall};
use ruint::aliases::U256;

use crate::{
    abis::tornado::Tornado,
    note::{Note, Nullifier, Secret},
    pool::{Asset, Pool},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Deposit {
    pool: Pool,
    nullifier: Nullifier,
    secret: Secret,
}

impl Deposit {
    pub fn new(pool: Pool, nullifier: Nullifier, secret: Secret) -> Self {
        Self {
            pool,
            nullifier,
            secret,
        }
    }

    pub fn with_nullifier(self, nullifier: Nullifier) -> Self {
        Self { nullifier, ..self }
    }

    pub fn with_secret(self, secret: Secret) -> Self {
        Self { secret, ..self }
    }

    /// Returns the value required for this deposit transaction.
    ///
    /// For ERC20 pools, this will be zero since the ERC20 token is transferred via
    /// a `transferFrom` call.
    pub fn value(&self) -> U256 {
        match self.pool.asset {
            Asset::Native { .. } => U256::from(self.pool.amount_wei),
            Asset::Erc20 { .. } => U256::ZERO,
        }
    }

    /// Returns the input data for this deposit transaction.
    pub fn input(&self) -> Vec<u8> {
        let deposit_call = Tornado::depositCall {
            _commitment: self.note().commitment().into(),
        };

        deposit_call.abi_encode()
    }

    /// Returns the note associated with this deposit.
    pub fn note(&self) -> Note {
        Note::new(
            self.nullifier,
            self.secret,
            self.pool.symbol(),
            self.pool.amount(),
            self.pool.chain_id,
        )
    }
}

impl From<Deposit> for TransactionRequest {
    fn from(deposit: Deposit) -> Self {
        TransactionRequest::default()
            .with_to(deposit.pool.address)
            .with_value(deposit.value())
            .input(deposit.input().into())
    }
}
