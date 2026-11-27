use alloy::primitives::{Address, Bytes};
#[cfg(feature = "bundler")]
use alloy::rpc::types::state::StateOverride;

use crate::{smart_account::Call, user_operation::UserOperationGasLimits};

/// A UserOperation to be prepared for a smart account.
#[derive(Debug, Clone)]
pub struct UserOperationRequest<C = Call> {
    /// Calls the account executes, encoded with its execution ABI.
    pub calls: Vec<C>,
    /// The paymaster and the data passed to it.
    pub paymaster: Option<(Address, Bytes)>,
    /// Gas limits. Estimated by the bundler when `None`.
    pub gas_limits: Option<UserOperationGasLimits>,
    pub max_fee_per_gas: u128,
    pub max_priority_fee_per_gas: u128,
    /// Placeholder signature for gas estimation. The account's stub signature when `None`.
    pub stub_signature: Option<Bytes>,
    /// State override applied only while estimating gas limits.
    #[cfg(feature = "bundler")]
    pub state_override: Option<StateOverride>,
}

impl<C> UserOperationRequest<C> {
    /// Sets the calls the account executes.
    #[must_use]
    pub fn with_calls(mut self, calls: impl IntoIterator<Item = C>) -> Self {
        self.calls = calls.into_iter().collect();
        self
    }

    /// Sets the paymaster and the data passed to it.
    #[must_use]
    pub fn with_paymaster(mut self, paymaster: Address, paymaster_data: Bytes) -> Self {
        self.paymaster = Some((paymaster, paymaster_data));
        self
    }

    /// Sets the gas limits, so preparing skips estimation.
    #[must_use]
    pub fn with_gas_limits(mut self, gas_limits: UserOperationGasLimits) -> Self {
        self.gas_limits = Some(gas_limits);
        self
    }

    /// Sets the max fee per gas.
    #[must_use]
    pub fn with_max_fee_per_gas(mut self, max_fee_per_gas: u128) -> Self {
        self.max_fee_per_gas = max_fee_per_gas;
        self
    }

    /// Sets the max priority fee per gas.
    #[must_use]
    pub fn with_max_priority_fee_per_gas(mut self, max_priority_fee_per_gas: u128) -> Self {
        self.max_priority_fee_per_gas = max_priority_fee_per_gas;
        self
    }

    /// Sets the placeholder signature used to simulate this UserOperation during gas estimation.
    ///
    /// Signing replaces it with the real signature.
    #[must_use]
    pub fn with_stub_signature(mut self, signature: Bytes) -> Self {
        self.stub_signature = Some(signature);
        self
    }

    /// Sets a state override applied only while estimating gas limits.
    #[cfg(feature = "bundler")]
    #[must_use]
    pub fn with_state_override(mut self, state_override: StateOverride) -> Self {
        self.state_override = Some(state_override);
        self
    }
}

impl<C> Default for UserOperationRequest<C> {
    fn default() -> Self {
        Self {
            calls: Vec::new(),
            paymaster: None,
            gas_limits: None,
            max_fee_per_gas: 0,
            max_priority_fee_per_gas: 0,
            stub_signature: None,
            #[cfg(feature = "bundler")]
            state_override: None,
        }
    }
}
