//! Pimlico-specific bundler methods.

use alloy::{eips::eip1559::Eip1559Estimation, providers::Provider, transports::TransportResult};
use serde::Deserialize;

/// Gas prices Pimlico accepts, by inclusion speed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub struct PimlicoGasPrice {
    pub slow: Eip1559Estimation,
    pub standard: Eip1559Estimation,
    pub fast: Eip1559Estimation,
}

/// Pimlico-specific bundler methods.
#[cfg_attr(native, async_trait::async_trait)]
#[cfg_attr(wasm, async_trait::async_trait(?Send))]
pub trait PimlicoApi: Send + Sync {
    async fn get_user_operation_gas_price(&self) -> TransportResult<PimlicoGasPrice>;
}

#[cfg_attr(native, async_trait::async_trait)]
#[cfg_attr(wasm, async_trait::async_trait(?Send))]
impl<P: Provider> PimlicoApi for P {
    async fn get_user_operation_gas_price(&self) -> TransportResult<PimlicoGasPrice> {
        self.client()
            .request_noparams("pimlico_getUserOperationGasPrice")
            .await
    }
}
