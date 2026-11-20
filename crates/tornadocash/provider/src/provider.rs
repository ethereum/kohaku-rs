use alloy::{primitives::Address, providers::Provider};
use kohaku_tornadocash::{Field, Pool};

use crate::abis::tornado::Tornado;

/// Extension trait for [`Provider`] with Tornadocash-specific accessors.
#[expect(async_fn_in_trait)]
pub trait TornadoProviderExt: Provider + Sized {
    /// Indicates whether `nullifier_hash` has been spent.
    async fn is_spent(
        &self,
        pool: &Pool,
        nullifier_hash: Field,
    ) -> Result<bool, alloy::contract::Error> {
        Tornado::new(pool.address, self)
            .isSpent(nullifier_hash.into())
            .call()
            .await
    }

    /// Indicates whether each of `nullifier_hashes` has been spent.
    ///
    /// The returned vector will have the same length and ordering as `nullifier_hashes`.
    async fn is_spent_array(
        &self,
        pool: &Pool,
        nullifier_hashes: &[Field],
    ) -> Result<Vec<bool>, alloy::contract::Error> {
        let hashes: Vec<_> = nullifier_hashes.iter().map(|&h| h.into()).collect();
        Tornado::new(pool.address, self)
            .isSpentArray(hashes)
            .call()
            .await
    }

    /// Indicates whether `root` is known to the Tornado pool.
    async fn is_known_root(
        &self,
        pool: &Pool,
        root: Field,
    ) -> Result<bool, alloy::contract::Error> {
        Tornado::new(pool.address, self)
            .isKnownRoot(root.into())
            .call()
            .await
    }

    /// Returns the address of the contract that verifies the pool's withdrawal proofs.
    async fn verifier(&self, pool: &Pool) -> Result<Address, alloy::contract::Error> {
        Tornado::new(pool.address, self).verifier().call().await
    }
}

impl<P: Provider> TornadoProviderExt for P {}
