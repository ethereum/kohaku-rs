use std::fmt;

use alloy::{network::TransactionBuilder, rpc::types::TransactionRequest, sol_types::SolCall};
use ruint::aliases::U256;

use crate::{
    abis::tornado::Tornado,
    note::{Note, Nullifier, Secret},
    pool::{Asset, Pool},
    provider::TornadoProvider,
};

#[derive(Clone)]
pub struct Deposit {
    provider: TornadoProvider,
    pool: Pool,
    nullifier: Nullifier,
    secret: Secret,
}

impl Deposit {
    pub fn new(
        provider: TornadoProvider,
        pool: Pool,
        nullifier: Nullifier,
        secret: Secret,
    ) -> Self {
        Self {
            provider,
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

    /// Returns the pool this deposit is for.
    #[must_use]
    pub fn pool(&self) -> &Pool {
        &self.pool
    }

    /// Returns the provider this deposit was created from.
    #[must_use]
    pub fn provider(&self) -> &TornadoProvider {
        &self.provider
    }

    /// Returns the value required for this deposit transaction.
    ///
    /// For ERC20 pools, this will be zero since the ERC20 token is transferred via
    /// a `transferFrom` call.
    #[must_use]
    pub fn value(&self) -> U256 {
        match self.pool.asset {
            Asset::Native { .. } => U256::from(self.pool.amount_wei),
            Asset::Erc20 { .. } => U256::ZERO,
        }
    }

    /// Returns the input data for this deposit transaction.
    #[must_use]
    pub fn input(&self) -> Vec<u8> {
        let deposit_call = Tornado::depositCall {
            _commitment: self.note().commitment().into(),
        };

        deposit_call.abi_encode()
    }

    /// Returns the note associated with this deposit.
    #[must_use]
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

impl fmt::Debug for Deposit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Deposit")
            .field("pool", &self.pool)
            .field("nullifier", &self.nullifier)
            .field("secret", &self.secret)
            .finish()
    }
}
