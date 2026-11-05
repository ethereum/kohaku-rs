#![doc = include_str!("../README.md")]

mod adapter_data;
pub mod verifier_override;

use alloy::{
    primitives::{Address, B256, Bytes, U256},
    providers::{DynProvider, Provider},
    rpc::types::TransactionRequest,
    sol_types::SolCall,
    transports::{RpcError, TransportErrorKind},
};
use kohaku_railgun::{
    account::{chain::ChainId, signer::RailgunSigner},
    caip::AssetId,
    note::Note,
    note::utxo::UtxoNote,
    poi::types::PoiStatus,
    transact::{AmountError, Spend, SpendError, attach_fee, proved_transaction::ProvedOperation},
    wallet::{RailgunTransactor, RailgunWallet},
};
use kohaku_userop_kit::{
    builder::UserOperationBuilder,
    bundler::{Bundler, BundlerError, BundlerExt},
    signable_user_operation::SignableUserOperation,
    smart_account::SmartAccount,
    user_operation::UserOperationGasEstimate,
};
use rand::CryptoRng;
use serde::Serialize;
use std::sync::Arc;
use thiserror::Error;
use tracing::info;

use crate::{
    adapter_data::{encode_paymaster_data, encode_railgun_adapter_data, paymaster_railgun_address},
    verifier_override::verifier_state_override,
};

/// Fee used to select notes before the bundler has estimated gas.
const SEED_FEE: u128 = 100_000_000;

/// Result of a single-proof UserOperation build, for logs and UIs.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SingleProofReport {
    pub total_gas_limit: u128,
    pub max_fee_per_gas: u128,
    pub fee: u128,
    pub paymaster_verification_measured: u128,
    pub paymaster_verification_limit: u128,
}

#[derive(Debug, Error)]
pub enum UserOpError {
    #[error("Railgun transaction error: {0}")]
    Railgun(#[from] kohaku_railgun::wallet::RailgunError),
    #[error("Bundler error: {0}")]
    Bundler(#[from] BundlerError),
    #[error("RPC error: {0}")]
    Rpc(#[from] RpcError<TransportErrorKind>),
    #[error("Unable to construct valid note configuration for fee payment")]
    FeeNoteNotFound,
    #[error("Spend error: {0}")]
    Spend(#[from] SpendError),
    #[error("Amount error: {0}")]
    Amount(#[from] AmountError),
    #[error("POI status unavailable; refresh observation before building a transaction")]
    PoiStatusUnavailable,
    #[error(
        "gas limits fixed before proving are too low: {what} needs {needed}, limit is {limit}. Nothing was sent"
    )]
    LimitsTooLow {
        what: &'static str,
        needed: u128,
        limit: u128,
    },
    #[error("Privacy Paymaster not configured for chain: {0}")]
    PrivacyPaymasterNotConfigured(u64),
    #[error("Other: {0}")]
    Other(Box<dyn std::error::Error + Send + Sync>),
}

/// Prepares paymaster-backed UserOperations using a caller-supplied chain provider.
pub struct RailgunUserOps<'a> {
    wallet: &'a mut RailgunWallet,
    provider: DynProvider,
    policy: Option<&'a mut dyn UserOpPoiPolicy>,
}

/// Optional local note policy for POI-aware UserOperation building.
/// Implementations must derive status from a validated local list copy.
pub trait UserOpPoiPolicy {
    fn status(&self, note: &UtxoNote) -> PoiStatus;
    fn register(
        &mut self,
        operations: &[ProvedOperation],
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>>;
}

impl<'a> RailgunUserOps<'a> {
    pub fn new(wallet: &'a mut RailgunWallet, provider: impl Provider + 'static) -> Self {
        Self {
            wallet,
            provider: provider.erased(),
            policy: None,
        }
    }

    pub fn with_policy(
        wallet: &'a mut RailgunWallet,
        provider: impl Provider + 'static,
        policy: &'a mut dyn UserOpPoiPolicy,
    ) -> Self {
        Self {
            wallet,
            provider: provider.erased(),
            policy: Some(policy),
        }
    }

    fn transactions(&mut self) -> RailgunTransactor<'_> {
        self.wallet.transactions()
    }

    async fn spends_with_fee(
        &mut self,
        spends: &[Spend],
        fee_payer: Arc<dyn RailgunSigner>,
        fee_address: kohaku_railgun::account::address::RailgunAddress,
        fee_token: Address,
        fee: u128,
        adapt_contract: Address,
        adapt_params: [u8; 32],
        rng: &mut impl CryptoRng,
    ) -> Result<Vec<Spend>, UserOpError> {
        let all = self.wallet.all_unspent_notes();
        let unavailable: Vec<_> = self
            .policy
            .as_ref()
            .map(|policy| {
                all.iter()
                    .filter(|note| policy.status(note) == PoiStatus::Unavailable)
                    .map(|note| (note.viewing_pubkey, note.asset))
                    .collect()
            })
            .unwrap_or_default();
        let available: Vec<_> = all
            .into_iter()
            .filter(|note| {
                self.policy
                    .as_ref()
                    .is_none_or(|policy| policy.status(note) == PoiStatus::Valid)
            })
            .collect();
        match attach_fee(
            spends,
            fee_payer,
            fee_address,
            AssetId::Erc20(fee_token),
            fee,
            &available,
            adapt_contract,
            adapt_params,
            rng,
        ) {
            Err(AmountError::InsufficientBalance { from, asset, .. })
                if unavailable.contains(&(from.viewing_pubkey(), asset)) =>
            {
                Err(UserOpError::PoiStatusUnavailable)
            }
            result => Ok(result?),
        }
    }

    fn ensure_notes_valid(&self, spends: &[Spend]) -> Result<(), UserOpError> {
        if let Some(policy) = &self.policy {
            for spend in spends {
                for note in spend.inputs() {
                    let status = policy.status(note);
                    if status != PoiStatus::Valid {
                        return Err(UserOpError::Spend(SpendError::NoteNotValid {
                            blinded_commitment: note.blinded_commitment,
                            status,
                        }));
                    }
                }
            }
        }
        Ok(())
    }

    async fn prove_spends(
        &mut self,
        spends: &[Spend],
        rng: &mut impl CryptoRng,
        real: bool,
    ) -> Result<Vec<ProvedOperation>, UserOpError> {
        self.ensure_notes_valid(spends)?;
        Ok(self.transactions().prove_spends(spends, rng, real).await?)
    }

    fn register_ops(&mut self, operations: &[ProvedOperation]) -> Result<(), UserOpError> {
        if let Some(policy) = &mut self.policy {
            policy.register(operations).map_err(UserOpError::Other)?;
        }
        Ok(())
    }

    /// The UserOperation [`Self::prepare_userop`] would build, with a dummy proof: same
    /// calldata, same paymaster data, nothing proved and the spending signer never asked.
    ///
    /// It cannot be sent. It exists to be simulated, with the verifier told to accept the dummy
    /// proof ([`crate::transact::verifier_override`]), so that the gas limits, hence the fee,
    /// are known before the one real proof.
    pub async fn dummy_userop<S: SmartAccount>(
        &mut self,
        spends: &[Spend],
        sender: &S,
        fee_payer: Arc<dyn RailgunSigner>,
        fee_token: Address,
        calldata: &S::Call,
        fee: u128,
        gas: UserOperationGasEstimate,
        rng: &mut impl CryptoRng,
    ) -> Result<SignableUserOperation, UserOpError> {
        let (userop, _) = self
            .dummy_userop_with_operations(
                spends, sender, fee_payer, fee_token, calldata, fee, gas, rng,
            )
            .await?;
        Ok(userop)
    }

    /// [`Self::dummy_userop`] with the operations it wraps, whose circuit sizes decide which
    /// verifying keys the estimate must override.
    #[allow(clippy::too_many_arguments)]
    async fn dummy_userop_with_operations<S: SmartAccount>(
        &mut self,
        spends: &[Spend],
        sender: &S,
        fee_payer: Arc<dyn RailgunSigner>,
        fee_token: Address,
        calldata: &S::Call,
        fee: u128,
        gas: UserOperationGasEstimate,
        rng: &mut impl CryptoRng,
    ) -> Result<(SignableUserOperation, Vec<ProvedOperation>), UserOpError> {
        let (privacy_paymaster, railgun_fee_adapter) = self.paymaster_contracts(fee_token)?;
        let fee_address = paymaster_railgun_address(ChainId::evm(self.wallet.chain().id));
        let with_fee = self
            .spends_with_fee(
                spends,
                fee_payer,
                fee_address,
                fee_token,
                fee,
                railgun_fee_adapter,
                *sender.address().into_word(),
                rng,
            )
            .await?;
        // Unsigned circuit inputs: a gas estimate must not ask the spending signer.
        let operations = self.prove_spends(&with_fee, rng, false).await?;
        let userop = self
            .userop_from(
                &operations,
                sender,
                calldata,
                privacy_paymaster,
                railgun_fee_adapter,
                fee_token,
                fee,
                gas,
            )
            .await?;
        Ok((userop, operations))
    }

    /// Same result as [`Self::prepare_userop`] with exactly one proof, hence one spending
    /// signature per operation, instead of one per round of the fee convergence loop.
    ///
    /// The loop exists because the fee depends on the gas, which the bundler only estimates by
    /// simulating a UserOperation carrying a valid proof, itself bound to the fee. Here the
    /// caller fixes `gas` beforehand, typically from a simulation of [`Self::dummy_userop`].
    /// The fee is `total gas limit x maxFeePerGas`, as the privacy paymaster computes it, so
    /// its check passes by construction.
    ///
    /// After proving, the paymaster verification gas is measured on the real proof and the
    /// bundler is asked for its own estimate. If either exceeds a limit the call fails before
    /// anything is sent: that costs a second signature, never funds.
    pub async fn prepare_userop_single_proof<S: SmartAccount>(
        &mut self,
        spends: &[Spend],
        bundler: &dyn Bundler,
        sender: &S,
        fee_payer: Arc<dyn RailgunSigner>,
        fee_token: Address,
        calldata: S::Call,
        gas: UserOperationGasEstimate,
        rng: &mut impl CryptoRng,
    ) -> Result<(SignableUserOperation, SingleProofReport), UserOpError> {
        let (privacy_paymaster, railgun_fee_adapter) = self.paymaster_contracts(fee_token)?;
        let fee_address = paymaster_railgun_address(ChainId::evm(self.wallet.chain().id));
        let total_gas = total_gas_limit(&gas);
        let fee = total_gas * gas.max_fee_per_gas;

        // The one proof. This is the only place the spending signer is used.
        let with_fee = self
            .spends_with_fee(
                spends,
                fee_payer,
                fee_address,
                fee_token,
                fee,
                railgun_fee_adapter,
                *sender.address().into_word(),
                rng,
            )
            .await?;
        let operations = self.prove_spends(&with_fee, rng, true).await?;
        let signable = self
            .userop_from(
                &operations,
                sender,
                &calldata,
                privacy_paymaster,
                railgun_fee_adapter,
                fee_token,
                fee,
                gas,
            )
            .await?;

        // Check the limits against the real proof before anything leaves.
        let pmv_limit = gas.paymaster_verification_gas_limit.unwrap_or(0);
        let pmv_measured =
            estimate_paymaster_verification_gas_limit(&self.provider, &signable).await?;
        if pmv_measured > pmv_limit {
            return Err(UserOpError::LimitsTooLow {
                what: "paymaster verification",
                needed: pmv_measured,
                limit: pmv_limit,
            });
        }
        let bundler_view = bundler.estimate_gas(&signable, None).await?;
        for (what, needed, limit) in [
            (
                "pre-verification",
                bundler_view.pre_verification_gas,
                gas.pre_verification_gas,
            ),
            (
                "account verification",
                bundler_view.verification_gas_limit,
                gas.verification_gas_limit,
            ),
            // Without execution calldata the call limit constrains nothing: a bundler's default
            // there is not a requirement.
            (
                "call",
                if signable.user_op.call_data.is_empty() {
                    0
                } else {
                    bundler_view.call_gas_limit
                },
                gas.call_gas_limit,
            ),
        ] {
            if needed > limit {
                return Err(UserOpError::LimitsTooLow {
                    what,
                    needed,
                    limit,
                });
            }
        }

        self.register_ops(&operations)?;
        let report = SingleProofReport {
            total_gas_limit: total_gas,
            max_fee_per_gas: gas.max_fee_per_gas,
            fee,
            paymaster_verification_measured: pmv_measured,
            paymaster_verification_limit: pmv_limit,
        };
        Ok((signable, report))
    }

    /// Estimates the ERC-4337 gas limits before proving, so the fee is fixed with one proof
    /// (hence one spending signature) via [`Self::prepare_userop_single_proof`], instead of
    /// the iterative re-proving [`Self::prepare_userop`] does.
    ///
    /// The bundler estimates a dummy-proof UserOperation ([`Self::dummy_userop`]) with a state
    /// override under which the Railgun verifier accepts the dummy proof
    /// ([`crate::transact::verifier_override`]), at the gas cost of a real one. Two rounds,
    /// because the fee is an output note whose value can change which notes are spent: the
    /// second uses the fee the first round's estimate implies. The bundler must support state
    /// overrides on `eth_estimateUserOperationGas`.
    pub async fn simulate_gas_limits<S: SmartAccount>(
        &mut self,
        spends: &[Spend],
        bundler: &dyn Bundler,
        sender: &S,
        fee_payer: Arc<dyn RailgunSigner>,
        fee_token: Address,
        calldata: &S::Call,
        rng: &mut impl CryptoRng,
    ) -> Result<UserOperationGasEstimate, UserOpError> {
        let mut fee = SEED_FEE;
        let mut estimate = None;
        for _round in 0..2 {
            // Gas and prices left at zero: the bundler sets its own for the simulation.
            let (dummy, operations) = self
                .dummy_userop_with_operations(
                    spends,
                    sender,
                    fee_payer.clone(),
                    fee_token,
                    calldata,
                    fee,
                    UserOperationGasEstimate {
                        pre_verification_gas: 0,
                        verification_gas_limit: 0,
                        call_gas_limit: 0,
                        paymaster_verification_gas_limit: Some(0),
                        paymaster_post_op_gas_limit: Some(0),
                        max_fee_per_gas: 0,
                        max_priority_fee_per_gas: 0,
                    },
                    rng,
                )
                .await?;
            let circuits = operations.iter().map(|op| {
                (
                    op.transaction.nullifiers.len(),
                    op.transaction.commitments.len(),
                )
            });
            let overrides =
                verifier_state_override(self.wallet.chain().railgun_smart_wallet, circuits);
            let gas = bundler.estimate_gas(&dummy, Some(&overrides)).await?;
            fee = total_gas_limit(&gas) * gas.max_fee_per_gas;
            estimate = Some(gas);
        }
        Ok(estimate.expect("two rounds ran"))
    }

    /// Fixes the fee by estimation ([`Self::simulate_gas_limits`]) then proves exactly once
    /// ([`Self::prepare_userop_single_proof`]): one spending signature, no iterative re-proving.
    /// Falls back to nothing: if the estimate fails, the error is returned and nothing is signed.
    pub async fn prepare_userop_single<S: SmartAccount>(
        &mut self,
        spends: &[Spend],
        bundler: &dyn Bundler,
        sender: &S,
        fee_payer: Arc<dyn RailgunSigner>,
        fee_token: Address,
        calldata: S::Call,
        rng: &mut impl CryptoRng,
    ) -> Result<SignableUserOperation, UserOpError> {
        let gas = self
            .simulate_gas_limits(
                spends,
                bundler,
                sender,
                fee_payer.clone(),
                fee_token,
                &calldata,
                rng,
            )
            .await?;
        info!(
            "Estimated before signing: pre-verification {}, account verification {}, call {}, paymaster verification {}, post-op {}; total {} gas at {} wei/gas",
            gas.pre_verification_gas,
            gas.verification_gas_limit,
            gas.call_gas_limit,
            gas.paymaster_verification_gas_limit.unwrap_or(0),
            gas.paymaster_post_op_gas_limit.unwrap_or(0),
            total_gas_limit(&gas),
            gas.max_fee_per_gas,
        );
        let (signable, _report) = self
            .prepare_userop_single_proof(
                spends, bundler, sender, fee_payer, fee_token, calldata, gas, rng,
            )
            .await?;
        Ok(signable)
    }

    fn paymaster_contracts(&self, fee_token: Address) -> Result<(Address, Address), UserOpError> {
        let privacy_paymaster = self.wallet.chain().privacy_paymaster.ok_or(
            UserOpError::PrivacyPaymasterNotConfigured(self.wallet.chain().id),
        )?;
        let railgun_fee_adapter = self.wallet.chain().railgun_fee_adapter.ok_or(
            UserOpError::PrivacyPaymasterNotConfigured(self.wallet.chain().id),
        )?;
        if fee_token != self.wallet.chain().wrapped_base_token {
            return Err(UserOpError::Other(Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Currently only the wrapped base token is supported for fee payment",
            ))));
        }
        Ok((privacy_paymaster, railgun_fee_adapter))
    }

    /// UserOperation around already built operations, with the given gas figures.
    #[allow(clippy::too_many_arguments, clippy::ptr_arg)]
    async fn userop_from<S: SmartAccount>(
        &self,
        // `&Vec` because `get_fee_operation` takes one.
        operations: &Vec<ProvedOperation>,
        sender: &S,
        calldata: &S::Call,
        privacy_paymaster: Address,
        railgun_fee_adapter: Address,
        fee_token: Address,
        fee: u128,
        gas: UserOperationGasEstimate,
    ) -> Result<SignableUserOperation, UserOpError> {
        let fee_asset = AssetId::Erc20(fee_token);
        let fee_operation = get_fee_operation(operations, fee_asset, fee)?;
        let fee_note = get_fee_note(fee_operation, fee_asset, fee)?;
        let transactions = operations.iter().map(|op| op.transaction.clone()).collect();
        let paymaster_data = encode_paymaster_data(
            railgun_fee_adapter,
            encode_railgun_adapter_data(fee_note.random(), fee_token, fee, transactions),
        );
        Ok(UserOperationBuilder::new_with_smart_account(sender)
            .await
            .map_err(|e| UserOpError::Other(Box::new(e)))?
            .with_call(calldata)
            .with_paymaster_and_data(privacy_paymaster, Bytes::from(paymaster_data))
            .with_gas(gas)
            .build())
    }

    /// Build spends into a broadcastable 7702 UserOperation.
    ///
    /// Constructs a UserOperation sent from the `delegator_address` that executes the provided
    /// transaction, with an additional fee note transfer to cover the bundler fees. The
    /// `fee_payer` is the signer that will authorize the fee note transfer to the bundler's
    /// address for the estimated fee amount in `fee_token`.
    pub async fn prepare_userop<S: SmartAccount>(
        &mut self,
        spends: &[Spend],
        bundler: &dyn Bundler,
        sender: &S,
        fee_payer: Arc<dyn RailgunSigner>,
        fee_token: Address,
        calldata: S::Call,
        rng: &mut impl CryptoRng,
    ) -> Result<SignableUserOperation, UserOpError> {
        let (privacy_paymaster, railgun_fee_adapter) = self.paymaster_contracts(fee_token)?;
        let fee_asset = AssetId::Erc20(fee_token);

        // Initial arbitrary estimation of fee note value.
        // IMPORTANT: Needs to be high enough to not cause a revert. Most
        // bundlers seem to use a fixed maxCost value for estimation (IE 27_000_000
        // for pimlico). Setting this too low causes an unrecoverable estimation
        // failure.
        let mut fee_value = 100_000_000;

        info!("Iteratively building UserOperation to converge on accurate fee estimate");
        for _ in 0..5 {
            info!(
                "Building broadcast transaction with fee value: {}",
                fee_value
            );
            let fee_address = paymaster_railgun_address(ChainId::evm(self.wallet.chain().id));
            let with_fee = self
                .spends_with_fee(
                    spends,
                    fee_payer.clone(),
                    fee_address,
                    fee_token,
                    fee_value,
                    railgun_fee_adapter,
                    *sender.address().into_word(),
                    rng,
                )
                .await?;
            let operations = self.prove_spends(&with_fee, rng, true).await?;

            // Get the fee operation & note so the decrypted commitment data can be sent to the
            // paymaster.
            let fee_operation = get_fee_operation(&operations, fee_asset, fee_value)?;
            let fee_note = get_fee_note(fee_operation, fee_asset, fee_value)?;

            let random = fee_note.random();
            let asset = fee_token;
            let value = fee_value;
            let transactions = operations.iter().map(|op| op.transaction.clone()).collect();

            let paymaster_data = encode_paymaster_data(
                railgun_fee_adapter,
                encode_railgun_adapter_data(random, asset, value, transactions),
            );

            // Construct UserOperation
            let mut signable = UserOperationBuilder::new_with_smart_account(sender)
                .await
                .map_err(|e| UserOpError::Other(Box::new(e)))?
                .with_call(&calldata)
                .with_paymaster_and_data(privacy_paymaster, Bytes::from(paymaster_data))
                .with_gas_estimate(bundler, None)
                .await?
                .build();

            // Recalculate fee and check for convergence.
            info!("Prepared broadcast transaction: {:?}", signable);
            let estimated_paymaster_verification_gas_limit =
                estimate_paymaster_verification_gas_limit(&self.provider, &signable).await?;
            info!(
                "Estimated paymaster verification gas limit: {}",
                estimated_paymaster_verification_gas_limit
            );
            signable.user_op.paymaster_verification_gas_limit =
                Some(estimated_paymaster_verification_gas_limit);
            let total_gas = signable.total_gas_limit();
            let new_fee = total_gas * signable.user_op.max_fee_per_gas;
            info!("Estimated total gas: {}", total_gas);
            info!(
                "Estimated max fee per gas: {}",
                signable.user_op.max_fee_per_gas
            );

            //? Return once the fee converges within 1% of the previous estimate.
            if new_fee <= fee_value && new_fee.abs_diff(fee_value) <= fee_value / 100 {
                info!("Fee converged at {}, total gas: {}", new_fee, total_gas);
                self.register_ops(&operations)?;
                return Ok(signable);
            }
            fee_value = new_fee;
            info!("Fee updated to {}", new_fee);
        }

        return Err(UserOpError::Other(Box::new(std::io::Error::new(
            std::io::ErrorKind::Other,
            "Failed to converge on fee estimate",
        ))));
    }
}

/// The sum of the limits, what the privacy paymaster multiplies by `maxFeePerGas` for the fee.
fn total_gas_limit(gas: &UserOperationGasEstimate) -> u128 {
    gas.pre_verification_gas
        + gas.verification_gas_limit
        + gas.call_gas_limit
        + gas.paymaster_verification_gas_limit.unwrap_or(0)
        + gas.paymaster_post_op_gas_limit.unwrap_or(0)
}

/// Gets the operation containing the fee note
fn get_fee_operation<'a>(
    operations: &'a Vec<ProvedOperation>,
    fee_asset: AssetId,
    fee_value: u128,
) -> Result<&'a ProvedOperation, UserOpError> {
    let Some(fee_note_pos) = operations.iter().position(|o| {
        o.inner
            .out_notes()
            .iter()
            .any(|n| is_fee_note(n, fee_asset, fee_value))
    }) else {
        return Err(UserOpError::FeeNoteNotFound);
    };
    Ok(&operations[fee_note_pos])
}

/// Gets the fee note from the operation
fn get_fee_note(
    operation: &ProvedOperation,
    fee_asset: AssetId,
    fee_value: u128,
) -> Result<Box<dyn Note>, UserOpError> {
    operation
        .inner
        .out_notes()
        .into_iter()
        .find(|n| is_fee_note(n, fee_asset, fee_value))
        .ok_or(UserOpError::FeeNoteNotFound)
}

fn is_fee_note(note: &Box<dyn Note>, fee_asset: AssetId, fee_value: u128) -> bool {
    note.asset() == fee_asset && note.value() == fee_value && note.memo() == "fee"
}

async fn estimate_paymaster_verification_gas_limit(
    provider: &DynProvider,
    user_op: &SignableUserOperation,
) -> Result<u128, UserOpError> {
    let entry_point = user_op.entry_point;
    let Some(paymaster) = user_op.user_op.paymaster else {
        return Ok(0);
    };
    let user_op = user_op.user_op.into_packed();
    let data: Bytes = abi::PrivacyPaymaster::validatePaymasterUserOpCall {
        userOp: abi::PrivacyPaymaster::PackedUserOperation {
            sender: user_op.sender,
            nonce: user_op.nonce,
            initCode: user_op.initCode,
            callData: user_op.callData,
            accountGasLimits: user_op.accountGasLimits,
            preVerificationGas: user_op.preVerificationGas,
            gasFees: user_op.gasFees,
            paymasterAndData: user_op.paymasterAndData,
            signature: Bytes::new(),
        },
        userOpHash: B256::ZERO,
        maxCost: U256::ZERO,
    }
    .abi_encode()
    .into();

    let request = TransactionRequest::default()
        .from(entry_point)
        .to(paymaster)
        .input(data.into());
    Ok(provider.estimate_gas(request).await? as u128)
}

mod abi {
    use alloy::sol;

    sol!(
        contract PrivacyPaymaster {
            function validatePaymasterUserOp(
                PackedUserOperation calldata userOp,
                bytes32 userOpHash,
                uint256 maxCost
            ) external returns (bytes memory context, uint256 validationData);

            struct PackedUserOperation {
                address sender;
                uint256 nonce;
                bytes initCode;
                bytes callData;
                bytes32 accountGasLimits;
                uint256 preVerificationGas;
                bytes32 gasFees;
                bytes paymasterAndData;
                bytes signature;
            }
        }
    );
}
