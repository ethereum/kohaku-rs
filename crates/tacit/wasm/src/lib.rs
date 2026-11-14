#![doc = include_str!("../README.md")]

mod convert;

use std::{cell::RefCell, rc::Rc};

use alloy::{providers::RootProvider, transports::http::reqwest::Url};
use js_sys::{Array, BigInt, Promise};
use kohaku_tacit::{
    Account, IncTree, Kind, Payer, Pool, ProvenTransaction, RelayerConfig, SavedState, Transaction,
    Wallet, circuit::Prover,
};
use kohaku_tacit_relayer::RelayerClient;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::future_to_promise;

use crate::convert::{address, amount, bigint, error, hex, object, pool_address};

/// An HTTP(S) JSON-RPC provider.
fn provider(url: &str) -> Result<RootProvider, JsError> {
    let url = Url::parse(url)?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(JsError::new("RPC URL must use http or https"));
    }
    Ok(RootProvider::new_http(url))
}

fn u64_of(value: &BigInt) -> Result<u64, JsError> {
    u64::try_from(amount(value)?).map_err(|_| JsError::new("value must be below 2^64"))
}

/// A wallet's keys, derived from one 32-byte identity key.
///
/// See [`kohaku_tacit::Account`].
#[wasm_bindgen]
pub struct TacitAccount {
    inner: Account,
}

#[wasm_bindgen]
impl TacitAccount {
    /// The account a 32-byte identity key opens.
    ///
    /// # Errors
    /// Throws if the key is not 32 bytes.
    #[wasm_bindgen(js_name = fromIdentity)]
    pub fn from_identity(identity: &[u8]) -> Result<TacitAccount, JsError> {
        let key: [u8; 32] = identity
            .try_into()
            .map_err(|_| JsError::new("an identity key is 32 bytes"))?;
        Ok(Self {
            inner: Account::from_identity(&key)?,
        })
    }

    /// The account at `m/29793'/1'/{account}'` of a BIP-39 phrase, as Kohaku's keystore derives
    /// it.
    ///
    /// # Errors
    /// Throws if the phrase is invalid.
    #[wasm_bindgen(js_name = fromMnemonic)]
    pub fn from_mnemonic(phrase: &str, account: u32) -> Result<TacitAccount, JsError> {
        Ok(Self {
            inner: Account::from_mnemonic(phrase, account)?,
        })
    }

    /// The account's pool address (`bp1…`).
    #[wasm_bindgen(getter)]
    #[must_use]
    pub fn address(&self) -> String {
        self.inner.address().to_string()
    }
}

/// One deployment of the pool. See [`kohaku_tacit::Pool`].
#[wasm_bindgen]
#[derive(Clone)]
pub struct TacitPool {
    inner: Pool,
}

#[wasm_bindgen]
impl TacitPool {
    /// The pool on Ethereum mainnet, with its relayer.
    #[must_use]
    pub fn ethereum() -> TacitPool {
        Self {
            inner: Pool::ethereum(),
        }
    }

    /// A deployment without a relayer.
    ///
    /// # Errors
    /// Throws if the address or a number is malformed.
    #[wasm_bindgen(constructor)]
    pub fn new(chain_id: &BigInt, pool: &str, deploy_block: &BigInt) -> Result<TacitPool, JsError> {
        Ok(Self {
            inner: Pool::new(u64_of(chain_id)?, address(pool)?, u64_of(deploy_block)?),
        })
    }

    /// The same pool, keeping history once it is `blocks` deep.
    #[wasm_bindgen(js_name = withConfirmations)]
    #[must_use]
    pub fn with_confirmations(&self, blocks: u32) -> TacitPool {
        Self {
            inner: self.inner.clone().with_confirmations(blocks.into()),
        }
    }

    /// The same pool with a relayer at `url` that must quote from `relayer` and charge at most
    /// `maxFee` wei.
    ///
    /// # Errors
    /// Throws if the address or fee is malformed.
    #[wasm_bindgen(js_name = withRelayer)]
    pub fn with_relayer(
        &self,
        url: String,
        relayer: &str,
        max_fee: &BigInt,
    ) -> Result<TacitPool, JsError> {
        Ok(Self {
            inner: self.inner.clone().with_relayer(Some(RelayerConfig {
                url,
                address: address(relayer)?,
                max_fee: amount(max_fee)?,
            })),
        })
    }

    #[wasm_bindgen(getter, js_name = chainId)]
    #[must_use]
    pub fn chain_id(&self) -> BigInt {
        bigint(self.inner.chain_id)
    }

    #[wasm_bindgen(getter)]
    #[must_use]
    pub fn address(&self) -> String {
        self.inner.address.to_string()
    }
}

/// Who sends a spend and what it pays them. See [`kohaku_tacit::Payer`].
#[wasm_bindgen]
#[derive(Clone, Copy)]
pub struct TacitPayer {
    inner: Payer,
}

#[wasm_bindgen]
impl TacitPayer {
    /// The user's own account sends the transaction; no fee.
    #[must_use]
    pub fn own() -> TacitPayer {
        Self { inner: Payer::SELF }
    }

    /// A relayer paid `fee` wei from the shielded funds.
    ///
    /// # Errors
    /// Throws if the address or fee is malformed.
    #[wasm_bindgen(constructor)]
    pub fn new(relayer: &str, fee: &BigInt) -> Result<TacitPayer, JsError> {
        Ok(Self {
            inner: Payer {
                relayer: address(relayer)?,
                fee: amount(fee)?,
            },
        })
    }

    #[wasm_bindgen(getter)]
    #[must_use]
    pub fn relayer(&self) -> String {
        self.inner.relayer.to_string()
    }

    #[wasm_bindgen(getter)]
    #[must_use]
    pub fn fee(&self) -> BigInt {
        bigint(self.inner.fee)
    }
}

/// A view of the pool's tree to prove against. See [`kohaku_tacit::IncTree`].
#[wasm_bindgen]
pub struct TacitTree {
    inner: IncTree,
}

#[wasm_bindgen]
impl TacitTree {
    #[wasm_bindgen(getter)]
    #[must_use]
    pub fn root(&self) -> String {
        hex(&self.inner.root().to_be_bytes::<32>())
    }

    #[wasm_bindgen(getter)]
    #[must_use]
    pub fn size(&self) -> BigInt {
        bigint(self.inner.size())
    }
}

/// The pool's prover, from the bytes of `transact.wasm` (`program`) and `transact_final.zkey`.
///
/// See [`kohaku_tacit::circuit::Prover`].
#[wasm_bindgen]
pub struct TacitProver {
    inner: Rc<Prover>,
}

#[wasm_bindgen]
impl TacitProver {
    /// Loads the prover. Either file must match its pinned hash.
    ///
    /// # Errors
    /// Throws if a file differs from its pin or cannot be read.
    #[wasm_bindgen(constructor)]
    // The program's parameter is not called `wasm`: that name would shadow wasm-bindgen's module
    // object in the generated JavaScript.
    pub fn new(program: &[u8], zkey: &[u8]) -> Result<TacitProver, JsError> {
        Ok(Self {
            inner: Rc::new(Prover::new(program, zkey)?),
        })
    }
}

/// A built `transact`, not yet proven. See [`kohaku_tacit::Transaction`].
#[wasm_bindgen]
pub struct TacitTransaction {
    inner: Transaction,
}

#[wasm_bindgen]
impl TacitTransaction {
    /// `shield`, `transfer`, `unshield` or `consolidate`.
    #[wasm_bindgen(getter)]
    #[must_use]
    pub fn kind(&self) -> String {
        match self.inner.kind() {
            Kind::Shield => "shield",
            Kind::Transfer => "transfer",
            Kind::Unshield => "unshield",
            Kind::Consolidate => "consolidate",
            _ => "other",
        }
        .into()
    }

    #[wasm_bindgen(getter)]
    #[must_use]
    pub fn amount(&self) -> BigInt {
        bigint(self.inner.amount())
    }

    #[wasm_bindgen(getter)]
    #[must_use]
    pub fn fee(&self) -> BigInt {
        bigint(self.inner.fee())
    }

    /// Whether it inserts notes, and so must reach the pool before another insertion does.
    #[wasm_bindgen(getter)]
    #[must_use]
    pub fn inserts(&self) -> bool {
        self.inner.inserts()
    }

    /// Proves the transaction. The proof is checked before it is returned.
    ///
    /// # Errors
    /// Throws if proving fails.
    pub fn prove(&self, prover: &TacitProver) -> Result<TacitProvenTransaction, JsError> {
        Ok(TacitProvenTransaction {
            inner: self.inner.clone().prove(&prover.inner, &mut rand::rng())?,
        })
    }
}

/// A proven `transact`. See [`kohaku_tacit::ProvenTransaction`].
#[wasm_bindgen]
pub struct TacitProvenTransaction {
    inner: ProvenTransaction,
}

#[wasm_bindgen]
impl TacitProvenTransaction {
    /// The call to send from the user's account: `{ to, data, value }`, `value` a bigint.
    #[must_use]
    pub fn call(&self) -> JsValue {
        let call = self.inner.call();
        object(&[
            ("to", call.target.to_string().into()),
            ("data", hex(&call.input).into()),
            ("value", BigInt::from(call.value.to::<u128>()).into()),
        ])
    }

    /// The transaction as a relayer's `/relay` takes it.
    ///
    /// # Errors
    /// Throws if it cannot be encoded.
    #[wasm_bindgen(js_name = relayRequest)]
    pub fn relay_request(&self) -> Result<JsValue, JsError> {
        let json = serde_json::to_string(&self.inner.relay_request())?;
        js_sys::JSON::parse(&json).map_err(|_| JsError::new("could not encode the request"))
    }
}

/// A wallet on one pool. See [`kohaku_tacit::Wallet`].
///
/// Methods that read the chain work on a copy and store it when they finish, so the wallet stays
/// usable while they run.
#[wasm_bindgen]
pub struct TacitWallet {
    inner: Rc<RefCell<Wallet>>,
}

#[wasm_bindgen]
impl TacitWallet {
    /// A wallet that has read nothing yet.
    #[wasm_bindgen(constructor)]
    #[must_use]
    pub fn new(account: &TacitAccount, pool: &TacitPool) -> TacitWallet {
        Self::wrap(Wallet::new(account.inner.clone(), pool.inner.clone()))
    }

    /// A wallet resuming from what [`TacitWallet::save`] returned.
    ///
    /// # Errors
    /// Throws if the saved state is malformed.
    pub fn restore(
        account: &TacitAccount,
        pool: &TacitPool,
        saved: &str,
    ) -> Result<TacitWallet, JsError> {
        let saved: SavedState = serde_json::from_str(saved)?;
        Ok(Self::wrap(Wallet::restore(
            account.inner.clone(),
            pool.inner.clone(),
            saved,
        )?))
    }

    fn wrap(wallet: Wallet) -> Self {
        Self {
            inner: Rc::new(RefCell::new(wallet)),
        }
    }

    /// The confirmed state as JSON, to persist.
    ///
    /// # Errors
    /// Throws if it cannot be encoded.
    pub fn save(&self) -> Result<String, JsError> {
        Ok(serde_json::to_string(&self.inner.borrow().save())?)
    }

    #[wasm_bindgen(getter)]
    #[must_use]
    pub fn address(&self) -> String {
        self.inner.borrow().address().to_string()
    }

    /// The balance in wei.
    #[must_use]
    pub fn balance(&self) -> BigInt {
        bigint(self.inner.borrow().balance())
    }

    /// The unspent notes: `{ index, value, block, leaf }`, numbers as bigints.
    #[must_use]
    pub fn notes(&self) -> Array {
        self.inner
            .borrow()
            .notes()
            .map(|n| {
                object(&[
                    ("index", bigint(n.index).into()),
                    ("value", bigint(n.value).into()),
                    ("block", bigint(n.block).into()),
                    ("leaf", hex(&n.leaf.to_be_bytes::<32>()).into()),
                ])
            })
            .collect()
    }

    /// The tree to prove against when the user's own account sends the transaction.
    #[must_use]
    pub fn tree(&self) -> TacitTree {
        TacitTree {
            inner: self.inner.borrow().tree().clone(),
        }
    }

    /// Reads what is new in the pool through `rpcUrl`.
    #[wasm_bindgen(unchecked_return_type = "Promise<void>")]
    pub fn sync(&self, rpc_url: &str) -> Promise {
        let inner = self.inner.clone();
        let provider = provider(rpc_url);
        let mut wallet = self.inner.borrow().clone();
        future_to_promise(async move {
            wallet.sync(&provider?).await.map_err(error)?;
            *inner.borrow_mut() = wallet;
            Ok(JsValue::UNDEFINED)
        })
    }

    /// Deposits `amount` wei for `to` (this wallet by default), sent by the user's account.
    ///
    /// # Errors
    /// Throws if the amount or address is invalid.
    #[allow(
        clippy::needless_pass_by_value,
        reason = "wasm-bindgen takes optional strings by value"
    )]
    pub fn shield(&self, amount: &BigInt, to: Option<String>) -> Result<TacitTransaction, JsError> {
        let to = to.as_deref().map(pool_address).transpose()?;
        let tx =
            self.inner
                .borrow()
                .shield(convert::amount(amount)?, to.as_ref(), &mut rand::rng())?;
        Ok(TacitTransaction { inner: tx })
    }

    /// Pays `amount` wei to the pool address `to`, proven against `tree`: [`TacitWallet::tree`], or
    /// a relayer's [`TacitRelayer::proving_tree`].
    ///
    /// # Errors
    /// Throws if the balance does not cover it in two notes.
    pub fn transfer(
        &self,
        to: &str,
        amount: &BigInt,
        payer: &TacitPayer,
        tree: &TacitTree,
    ) -> Result<TacitTransaction, JsError> {
        let to = pool_address(to)?;
        let wallet = self.inner.borrow();
        let tx = wallet.transfer(
            &to,
            convert::amount(amount)?,
            payer.inner,
            &tree.inner,
            &mut rand::rng(),
        )?;
        Ok(TacitTransaction { inner: tx })
    }

    /// Withdraws `amount` wei to the public address `recipient`.
    ///
    /// # Errors
    /// Throws if the balance does not cover it in two notes.
    pub fn unshield(
        &self,
        recipient: &str,
        amount: &BigInt,
        payer: &TacitPayer,
        tree: &TacitTree,
    ) -> Result<TacitTransaction, JsError> {
        let recipient = address(recipient)?;
        let wallet = self.inner.borrow();
        let tx = wallet.unshield(
            recipient,
            convert::amount(amount)?,
            payer.inner,
            &tree.inner,
            &mut rand::rng(),
        )?;
        Ok(TacitTransaction { inner: tx })
    }

    /// Merges the two largest notes into one.
    ///
    /// # Errors
    /// Throws if there are fewer than two notes.
    pub fn consolidate(
        &self,
        payer: &TacitPayer,
        tree: &TacitTree,
    ) -> Result<TacitTransaction, JsError> {
        let wallet = self.inner.borrow();
        let tx = wallet.consolidate(payer.inner, &tree.inner, &mut rand::rng())?;
        Ok(TacitTransaction { inner: tx })
    }
}

/// A pool's relayer. See [`kohaku_tacit_relayer::RelayerClient`].
#[wasm_bindgen]
pub struct TacitRelayer {
    inner: RelayerClient,
}

#[wasm_bindgen]
impl TacitRelayer {
    /// The client of `pool`'s configured relayer.
    ///
    /// # Errors
    /// Throws if the pool has no relayer.
    #[wasm_bindgen(constructor)]
    pub fn new(pool: &TacitPool) -> Result<TacitRelayer, JsError> {
        Ok(Self {
            inner: RelayerClient::new(&pool.inner)?,
        })
    }

    /// The relayer's checked quote, as the payer to build a spend with.
    #[wasm_bindgen(unchecked_return_type = "Promise<TacitPayer>")]
    pub fn quote(&self) -> Promise {
        let client = self.inner.clone();
        future_to_promise(async move {
            let quote = client.quote().await.map_err(error)?;
            Ok(TacitPayer {
                inner: quote.payer(),
            }
            .into())
        })
    }

    /// The tree a relayed spend that inserts notes is proven against.
    #[wasm_bindgen(js_name = provingTree, unchecked_return_type = "Promise<TacitTree>")]
    pub fn proving_tree(&self, wallet: &TacitWallet, rpc_url: &str) -> Promise {
        let client = self.inner.clone();
        let inner = wallet.inner.clone();
        let mut copy = wallet.inner.borrow().clone();
        let provider = provider(rpc_url);
        future_to_promise(async move {
            let tree = client.proving_tree(&mut copy, &provider?).await;
            *inner.borrow_mut() = copy;
            Ok(TacitTree { inner: tree }.into())
        })
    }

    /// Reads confirmed history from the relayer's feed into `wallet`, checked against the pool
    /// through `rpcUrl`.
    #[wasm_bindgen(js_name = syncFromFeed, unchecked_return_type = "Promise<void>")]
    pub fn sync_from_feed(&self, wallet: &TacitWallet, rpc_url: &str) -> Promise {
        let client = self.inner.clone();
        let inner = wallet.inner.clone();
        let mut copy = wallet.inner.borrow().clone();
        let provider = provider(rpc_url);
        future_to_promise(async move {
            client
                .sync_from_feed(&mut copy, &provider?)
                .await
                .map_err(error)?;
            *inner.borrow_mut() = copy;
            Ok(JsValue::UNDEFINED)
        })
    }

    /// Submits a proven spend. Resolves to the transaction hash.
    #[wasm_bindgen(unchecked_return_type = "Promise<string>")]
    pub fn relay(&self, tx: &TacitProvenTransaction) -> Promise {
        let client = self.inner.clone();
        let request = tx.inner.relay_request();
        future_to_promise(async move {
            let hash = client.relay(&request).await.map_err(error)?;
            Ok(JsValue::from_str(&hash.to_string()))
        })
    }
}
