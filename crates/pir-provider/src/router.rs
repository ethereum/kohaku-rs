use std::sync::Arc;

use serde_json::Value;
use tracing::debug;

use crate::{
    DatasetManifest, FallbackRpc, LookupBackend, MapFallback, MapLookup, PirProviderError, Route,
    RouteTable,
    routes::{encode_bytes, encode_qty, encode_uint256, encode_zero, parse_address_param},
};

/// Hybrid router: PIR allowlist + JSON-RPC fallback.
pub struct PirRouter {
    lookup: Arc<dyn LookupBackend>,
    fallback: Arc<dyn FallbackRpc>,
    routes: RouteTable,
}

impl PirRouter {
    /// Wrap a lookup backend with HTTP JSON-RPC fallback.
    ///
    /// `lookup` is how PIR bytes are fetched (remote `pir-client` wrapper, or
    /// a test map). `rpc_url` is the ordinary Ethereum node.
    ///
    /// # Errors
    ///
    /// Returns [`PirProviderError::InvalidUrl`] if `rpc_url` is not a valid HTTP URL.
    pub fn with_rpc(
        lookup: Arc<dyn LookupBackend>,
        rpc_url: &str,
        datasets: Vec<DatasetManifest>,
    ) -> Result<Self, PirProviderError> {
        Ok(Self::from_parts(
            lookup,
            Arc::new(crate::HttpFallback::new(rpc_url)?),
            datasets,
        ))
    }

    /// Build a router from injected backends (tests, custom transports).
    #[must_use]
    pub fn from_parts(
        lookup: Arc<dyn LookupBackend>,
        fallback: Arc<dyn FallbackRpc>,
        datasets: Vec<DatasetManifest>,
    ) -> Self {
        Self {
            lookup,
            fallback,
            routes: RouteTable::from_datasets(datasets),
        }
    }

    /// Convenience constructor for in-memory tests.
    #[must_use]
    pub fn mock(lookup: MapLookup, fallback: MapFallback, datasets: Vec<DatasetManifest>) -> Self {
        Self::from_parts(Arc::new(lookup), Arc::new(fallback), datasets)
    }

    /// Routing table used for this router.
    #[must_use]
    pub const fn routes(&self) -> &RouteTable {
        &self.routes
    }

    /// Dispatch one JSON-RPC method.
    ///
    /// # Errors
    ///
    /// Returns [`PirProviderError`] on PIR failure, invalid params, or fallback
    /// RPC/HTTP errors. A PIR miss is **not** an error: account methods return
    /// `0x0`.
    pub async fn request(&self, method: &str, params: Value) -> Result<Value, PirProviderError> {
        let params = if params.is_null() {
            Value::Array(Vec::new())
        } else {
            params
        };
        match self.routes.classify(method, &params) {
            Route::AccountBalance => self.account_field(&params, AccountField::Balance).await,
            Route::AccountNonce => self.account_field(&params, AccountField::Nonce).await,
            Route::Call(m) => self.dataset_lookup(m.key, &m.value_encoding).await,
            Route::Fallback => {
                debug!(method, "fallback RPC");
                self.fallback.request(method, params).await
            }
        }
    }

    async fn account_field(
        &self,
        params: &Value,
        field: AccountField,
    ) -> Result<Value, PirProviderError> {
        let key = parse_address_param(params)?;
        let value = self.lookup_blocking(key.to_vec()).await?;
        match value {
            Some(raw) => {
                let acct = parse_account_bytes(&raw).ok_or_else(|| {
                    PirProviderError::Client("account value is not 40 bytes".into())
                })?;
                Ok(match field {
                    AccountField::Balance => encode_qty(acct.balance),
                    AccountField::Nonce => encode_qty(u128::from(acct.nonce)),
                })
            }
            None => Ok(encode_qty(0)),
        }
    }

    async fn dataset_lookup(
        &self,
        key: Vec<u8>,
        encoding: &str,
    ) -> Result<Value, PirProviderError> {
        match self.lookup_blocking(key).await? {
            Some(raw) => Ok(match encoding {
                "bytes" | "account" => encode_bytes(&raw),
                _ => encode_uint256(&raw),
            }),
            None => Ok(encode_zero(encoding)),
        }
    }

    async fn lookup_blocking(&self, key: Vec<u8>) -> Result<Option<Vec<u8>>, PirProviderError> {
        let lookup = Arc::clone(&self.lookup);
        tokio::task::spawn_blocking(move || lookup.lookup(&key)).await?
    }
}

enum AccountField {
    Balance,
    Nonce,
}

fn parse_account_bytes(v: &[u8]) -> Option<AccountView> {
    if v.len() != 40 {
        return None;
    }
    Some(AccountView {
        balance: u128::from_be_bytes(v[16..32].try_into().ok()?),
        nonce: u64::from_be_bytes(v[32..40].try_into().ok()?),
    })
}

struct AccountView {
    balance: u128,
    nonce: u64,
}

#[cfg(test)]
mod tests {
    use crate::DatasetManifest;
    use serde_json::json;

    use super::*;
    use crate::routes::encode_uint256;

    fn account_bytes(balance: u128, nonce: u64) -> Vec<u8> {
        let mut v = vec![0u8; 40];
        v[16..32].copy_from_slice(&balance.to_be_bytes());
        v[32..40].copy_from_slice(&nonce.to_be_bytes());
        v
    }

    fn addr(n: u8) -> [u8; 20] {
        let mut a = [0u8; 20];
        a[19] = n;
        a
    }

    fn addr_hex(n: u8) -> String {
        format!("0x{}", hex::encode(addr(n)))
    }

    #[tokio::test]
    async fn get_balance_uses_pir_not_fallback() {
        let lookup = MapLookup::default();
        lookup.insert(addr(1), account_bytes(0x0163_4578_5d8a_0000, 7));
        let fallback = MapFallback::default();
        fallback.set("eth_getBalance", json!("0xdead"));
        let router = PirRouter::mock(lookup, fallback, Vec::new());

        let got = router
            .request("eth_getBalance", json!([addr_hex(1), "latest"]))
            .await
            .unwrap();
        assert_eq!(got, json!("0x16345785d8a0000"));
    }

    #[tokio::test]
    async fn get_nonce_uses_pir() {
        let lookup = MapLookup::default();
        lookup.insert(addr(1), account_bytes(1, 9));
        let router = PirRouter::mock(lookup, MapFallback::default(), Vec::new());
        let got = router
            .request("eth_getTransactionCount", json!([addr_hex(1)]))
            .await
            .unwrap();
        assert_eq!(got, json!("0x9"));
    }

    #[tokio::test]
    async fn missing_account_is_zero_not_fallback() {
        let fallback = MapFallback::default();
        fallback.set("eth_getBalance", json!("0xdead"));
        let router = PirRouter::mock(MapLookup::default(), fallback, Vec::new());
        let got = router
            .request("eth_getBalance", json!([addr_hex(9), "latest"]))
            .await
            .unwrap();
        assert_eq!(got, json!("0x0"));
    }

    #[tokio::test]
    async fn get_logs_uses_fallback() {
        let fallback = MapFallback::default();
        fallback.set("eth_getLogs", json!([]));
        let router = PirRouter::mock(MapLookup::default(), fallback, Vec::new());
        let got = router.request("eth_getLogs", json!([{}])).await.unwrap();
        assert_eq!(got, json!([]));
    }

    #[tokio::test]
    async fn unknown_eth_call_uses_fallback() {
        let fallback = MapFallback::default();
        fallback.set("eth_call", json!("0x01"));
        let router = PirRouter::mock(MapLookup::default(), fallback, Vec::new());
        let params = json!([{
            "to": addr_hex(3),
            "data": "0xdeadbeef"
        }, "latest"]);
        let got = router.request("eth_call", params).await.unwrap();
        assert_eq!(got, json!("0x01"));
    }

    #[tokio::test]
    async fn matched_eth_call_uses_pir() {
        let to = addr(8);
        let holder = addr(2);
        let mut packed = Vec::from(to);
        packed.extend_from_slice(&holder);
        let key = alloy::primitives::keccak256(&packed);

        let lookup = MapLookup::default();
        lookup.insert(key.as_slice(), 42u128.to_be_bytes());

        let datasets = vec![DatasetManifest {
            id: "erc20_balances".into(),
            selectors: vec!["0x70a08231".into()],
            contracts: vec![addr_hex(8)],
            key_scheme: "token_holder".into(),
            value_encoding: "uint256".into(),
            ..DatasetManifest::default()
        }];
        let fallback = MapFallback::default();
        fallback.set("eth_call", json!("0xdead"));
        let router = PirRouter::mock(lookup, fallback, datasets);

        let data = format!("0x70a08231{:0>64}", hex::encode(holder));
        let params = json!([{ "to": addr_hex(8), "data": data }, "latest"]);
        let got = router.request("eth_call", params).await.unwrap();
        assert_eq!(got, encode_uint256(&42u128.to_be_bytes()));
    }

    #[tokio::test]
    async fn historical_get_balance_uses_fallback() {
        let fallback = MapFallback::default();
        fallback.set("eth_getBalance", json!("0xabc"));
        let lookup = MapLookup::default();
        lookup.insert(addr(1), account_bytes(1, 0));
        let router = PirRouter::mock(lookup, fallback, Vec::new());
        let got = router
            .request("eth_getBalance", json!([addr_hex(1), "0x10"]))
            .await
            .unwrap();
        assert_eq!(got, json!("0xabc"));
    }
}
