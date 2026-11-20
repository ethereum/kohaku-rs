use super::*;
#[cfg(all(test, native))]
mod poi_availability_tests {
    use alloy::providers::Provider;
    use tokio::{
        net::TcpListener,
        time::{Duration, timeout},
    };

    use super::*;
    use crate::{
        account::signer::PrivateKeySigner,
        builder::RailgunBuilder,
        chain_config::ChainConfig,
        crypto::keys::{ByteKey, SpendingKey, ViewingKey},
        indexer::{
            indexed_account::IndexedAccountState,
            syncer::{Operation, SyncEvent, SyncerError, TxidSyncer, UtxoSyncer},
        },
        poi::types::BlindedCommitmentType,
        state::RailgunCheckpoint,
        transact::SpendError,
    };

    const LIST: &str = "public-fixture-list";

    struct NoSync;
    #[async_trait::async_trait]
    impl UtxoSyncer for NoSync {
        async fn latest_block(&self) -> Result<u64, SyncerError> {
            panic!("no sync expected")
        }
        async fn sync(&self, _: u64, _: u64) -> Result<Vec<SyncEvent>, SyncerError> {
            panic!("no sync expected")
        }
    }
    #[async_trait::async_trait]
    impl TxidSyncer for NoSync {
        async fn latest_block(&self) -> Result<u64, SyncerError> {
            panic!("no sync expected")
        }
        async fn sync(&self, _: u64, _: u64) -> Result<Vec<Operation>, SyncerError> {
            panic!("no sync expected")
        }
    }

    async fn fixture(url: &str) -> (PoiWallet, Arc<dyn RailgunSigner>, AssetId) {
        fixture_with_notes(url, 1).await
    }

    async fn fixture_with_notes(
        url: &str,
        count: u32,
    ) -> (PoiWallet, Arc<dyn RailgunSigner>, AssetId) {
        // Public, never-funded keys. This seeds local state; it is not a scan test.
        let signer = PrivateKeySigner::new_evm(
            SpendingKey::from_bytes([1; 32]),
            ViewingKey::from_bytes([2; 32]),
            11_155_111,
        );
        let asset = AssetId::erc20(Address::repeat_byte(0x11));
        let mut state = RailgunCheckpoint::default();
        state.utxo.accounts.insert(
            signer.address(),
            IndexedAccountState {
                notes: (0..count)
                    .map(|leaf| {
                        UtxoNote::new(
                            0,
                            leaf,
                            signer.clone(),
                            asset,
                            10,
                            [5; 16],
                            "public fixture",
                            BlindedCommitmentType::Transact,
                        )
                    })
                    .collect(),
                ..Default::default()
            },
        );
        let mut chain = ChainConfig::sepolia();
        chain.poi_endpoint = url.to_owned();
        chain.subsquid_endpoint = url.to_owned();
        chain.list_keys = vec![LIST.into()];
        let rpc = alloy::providers::ProviderBuilder::new()
            .connect_http(url.parse().unwrap())
            .erased();
        let wallet = RailgunBuilder::new(chain, rpc)
            .with_state(state)
            .with_utxo_syncer(Arc::new(NoSync))
            .build()
            .await
            .unwrap();
        let mut provider =
            PoiWallet::new(wallet, PoiCheckpoint::default(), Arc::new(NoSync)).unwrap();
        provider.register(signer.clone()).await.unwrap();
        (provider, signer, asset)
    }

    #[tokio::test]
    async fn unsynced_notes_are_unavailable_and_cannot_spend() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let (mut provider, signer, asset) = fixture(&url).await;
        let notes = timeout(Duration::from_secs(5), provider.notes(signer.address()))
            .await
            .unwrap();
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].poi_status, Some(PoiStatus::Unavailable));
        assert_eq!(
            serde_json::to_value(&notes[0]).unwrap()["poiStatus"],
            "Unavailable"
        );
        let balance = timeout(Duration::from_secs(5), provider.balance(signer.address()))
            .await
            .unwrap();
        assert_eq!(balance.len(), 1);
        assert_eq!(balance[0].amount, 10);
        assert_eq!(balance[0].poi_status, Some(PoiStatus::Unavailable));
        let result = timeout(
            Duration::from_secs(5),
            provider.transactions().transfer_amount(
                signer.clone(),
                signer.address(),
                asset,
                1,
                "",
                &mut rand::rng(),
            ),
        )
        .await
        .unwrap();
        assert!(matches!(result, Err(PoiWalletError::PoiStatusUnavailable)));
        let notes = timeout(
            Duration::from_secs(5),
            provider.unspent_notes(signer.address()),
        )
        .await
        .unwrap();
        let explicit = Spend::try_new(signer.clone(), vec![notes[0].note.clone()])
            .unwrap()
            .transfer(signer.address(), 1, "")
            .unwrap()
            .with_change(&mut rand::rng())
            .unwrap();
        let explicit = timeout(
            Duration::from_secs(5),
            provider.transactions().prove(&[explicit], &mut rand::rng()),
        )
        .await
        .unwrap();
        assert!(matches!(
            explicit,
            Err(PoiWalletError::Spend(SpendError::NoteNotValid { .. }))
        ));
        assert!(
            timeout(Duration::from_millis(20), listener.accept())
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn unavailable_notes_of_another_asset_do_not_mask_insufficient_balance() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let (mut provider, signer, _) = fixture(&url).await;
        assert!(matches!(
            timeout(
                Duration::from_secs(5),
                provider.transactions().transfer_amount(
                    signer.clone(),
                    signer.address(),
                    AssetId::erc20(Address::repeat_byte(0x22)),
                    1,
                    "",
                    &mut rand::rng(),
                )
            )
            .await
            .unwrap(),
            Err(PoiWalletError::Amount(
                AmountError::InsufficientBalance { .. }
            ))
        ));
        assert!(
            timeout(Duration::from_millis(20), listener.accept())
                .await
                .is_err()
        );
    }

    #[test]
    fn unavailable_round_trips_locally_but_not_as_a_node_status() {
        assert_eq!(PoiStatus::Unavailable.to_string(), "Unavailable");
        assert_eq!(
            serde_json::to_value(PoiStatus::Unavailable).unwrap(),
            "Unavailable"
        );
        assert_eq!(
            serde_json::from_str::<PoiStatus>("\"Unavailable\"").unwrap(),
            PoiStatus::Unavailable
        );
        assert_eq!(
            "Unavailable".parse::<PoiStatus>().unwrap(),
            PoiStatus::Unavailable
        );
    }
}

#[cfg(all(test, native))]
mod spendable_notes_tests {
    use alloy::primitives::Address;

    use super::*;
    use crate::{
        account::signer::{PrivateKeySigner, RailgunSigner},
        caip::AssetId,
        crypto::keys::{ByteKey, SpendingKey, ViewingKey},
        note::utxo::UtxoNote,
        poi::types::BlindedCommitmentType,
        transact::AmountError,
    };

    fn signer() -> std::sync::Arc<PrivateKeySigner> {
        PrivateKeySigner::new_evm(
            SpendingKey::from_bytes([1; 32]),
            ViewingKey::from_bytes([2; 32]),
            11_155_111,
        )
    }

    fn note(signer: std::sync::Arc<PrivateKeySigner>, asset: AssetId, value: u128) -> UtxoNote {
        UtxoNote::new(
            0,
            0,
            signer,
            asset,
            value,
            [5; 16],
            "public fixture",
            BlindedCommitmentType::Transact,
        )
    }

    #[test]
    fn a_valid_note_stays_spendable_beside_an_unavailable_one() {
        let signer = signer();
        let asset = AssetId::erc20(Address::repeat_byte(0x11));
        let valid = note(signer.clone(), asset, 7);
        let unavailable = note(signer, asset, 10);
        let selected = SpendableNotes::from_unspent(vec![
            AnnotatedNote {
                note: valid,
                poi_status: Some(PoiStatus::Valid),
            },
            AnnotatedNote {
                note: unavailable,
                poi_status: Some(PoiStatus::Unavailable),
            },
        ]);
        assert_eq!(selected.notes.len(), 1);
        assert_eq!(selected.notes[0].value, 7);
        assert!(selected.explain(Ok(vec![])).is_ok());
    }

    #[test]
    fn missing_is_not_an_availability_error() {
        let signer = signer();
        let asset = AssetId::erc20(Address::repeat_byte(0x11));
        let missing = note(signer.clone(), asset, 10);
        let selected = SpendableNotes::from_unspent(vec![AnnotatedNote {
            note: missing,
            poi_status: Some(PoiStatus::Missing),
        }]);
        assert!(selected.notes.is_empty());
        let err = selected.explain(Err(AmountError::InsufficientBalance {
            from: signer.address(),
            asset,
            value: 1,
        }));
        assert!(matches!(
            err,
            Err(PoiWalletError::Amount(
                AmountError::InsufficientBalance { .. }
            ))
        ));
    }

    #[test]
    fn unavailable_on_another_asset_does_not_mask_a_shortfall() {
        let signer = signer();
        let wanted = AssetId::erc20(Address::repeat_byte(0x11));
        let other = AssetId::erc20(Address::repeat_byte(0x22));
        let unavailable = note(signer.clone(), other, 10);
        let selected = SpendableNotes::from_unspent(vec![AnnotatedNote {
            note: unavailable,
            poi_status: Some(PoiStatus::Unavailable),
        }]);
        let err = selected.explain(Err(AmountError::InsufficientBalance {
            from: signer.address(),
            asset: wanted,
            value: 1,
        }));
        assert!(matches!(
            err,
            Err(PoiWalletError::Amount(
                AmountError::InsufficientBalance { .. }
            ))
        ));
    }

    #[test]
    fn a_shortfall_on_an_unobserved_asset_is_unavailable() {
        let signer = signer();
        let asset = AssetId::erc20(Address::repeat_byte(0x11));
        let unavailable = note(signer.clone(), asset, 10);
        let selected = SpendableNotes::from_unspent(vec![AnnotatedNote {
            note: unavailable,
            poi_status: Some(PoiStatus::Unavailable),
        }]);
        let err = selected.explain(Err(AmountError::InsufficientBalance {
            from: signer.address(),
            asset,
            value: 1,
        }));
        assert!(matches!(err, Err(PoiWalletError::PoiStatusUnavailable)));
    }
}
