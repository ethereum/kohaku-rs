use kohaku_tornadocash::deposit::Deposit;

use crate::wallet::{Wallet, WalletError};

/// Extension for a [`Deposit`] to derive its note material from a [`Wallet`].
pub trait DepositWalletExt: Sized {
    /// Set this deposit's secret and nullifier using `wallet`.
    ///
    /// # Errors
    /// Returns an error if the keychain, provider, or store fails.
    fn with_wallet(
        self,
        wallet: &Wallet,
    ) -> impl std::future::Future<Output = Result<Self, WalletError>>;
}

impl DepositWalletExt for Deposit {
    async fn with_wallet(self, wallet: &Wallet) -> Result<Self, WalletError> {
        let pool = self.pool().clone();
        let (_, secret, nullifier) = wallet.reserve(&pool).await?;

        Ok(self.with_nullifier(nullifier).with_secret(secret))
    }
}
