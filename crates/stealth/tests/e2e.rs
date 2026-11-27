#![cfg(not(target_arch = "wasm32"))]

use alloy::{
    network::TransactionBuilder,
    node_bindings::Anvil,
    primitives::{Address, U256},
    providers::{Provider, ProviderBuilder, ext::AnvilApi},
    rpc::types::TransactionRequest,
    signers::local::PrivateKeySigner,
    sol_types::SolCall,
};
use futures::{TryStreamExt, pin_mut, stream};
use kohaku_fork_kit::stealth::deploy_stealth;
use kohaku_stealth::{
    Account, AccountSeed, ExposeSecret, FetchError, FetchEvent, ScanEvent, SchemeError, SchemeId3,
    StealthProviderExt, StealthScheme, StreamedEventConfig,
    contracts::{announce, register_keys},
    get_announcements, get_announcements_for_user,
};
use pqsa_core::SenderState;
use rand::{RngExt, SeedableRng, rngs::StdRng};

#[path = "../src/abis.rs"]
mod abis;
use abis::IERC5564Announcer;

#[path = "fixtures/mod.rs"]
mod fixtures;

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn receive_and_spend() -> anyhow::Result<()> {
    let anvil = Anvil::new().try_spawn()?;
    let url = anvil.endpoint_url();
    let funder = PrivateKeySigner::from(anvil.keys()[0].clone());
    let recipient = PrivateKeySigner::from(anvil.keys()[1].clone());
    let provider = ProviderBuilder::new()
        .wallet(funder.clone())
        .connect_http(url.clone())
        .erased();
    let recipient_provider = ProviderBuilder::new()
        .wallet(recipient.clone())
        .connect_http(url.clone());
    let deployment = deploy_stealth(provider.clone()).await?;
    let fixture = fixtures::Payment::load();
    let account = fixture.account();
    assert_eq!(fixture.scheme_id, SchemeId3::SCHEME_ID);
    assert!(
        provider
            .resolve_meta_address(&deployment, recipient.address())
            .await?
            .is_none()
    );

    let registration_receipt = recipient_provider
        .send_transaction(register_keys(&deployment, account.meta_address()))
        .await?
        .get_receipt()
        .await?;
    assert!(registration_receipt.status());
    assert_eq!(registration_receipt.from, recipient.address());
    let registered = provider
        .resolve_meta_address(&deployment, recipient.address())
        .await?
        .expect("meta-address was registered");
    assert_eq!(
        SchemeId3::meta_to_bytes(&registered),
        fixture.meta_address.as_ref()
    );

    let seed = SenderState::resume(&[0x5a; 32], 0)
        .draw_seed::<SchemeId3>()
        .map_err(SchemeError::from)?;
    let announcement = SchemeId3::announce(&registered, &seed).map_err(SchemeError::from)?;
    let published = fixture.announcement();
    assert_eq!(
        SchemeId3::announcement_to_bytes(&announcement),
        SchemeId3::announcement_to_bytes(&published)
    );
    let announcement_receipt = provider
        .send_transaction(announce(&deployment, &announcement))
        .await?
        .get_receipt()
        .await?;
    assert!(announcement_receipt.status());
    assert_eq!(announcement_receipt.from, funder.address());
    let funding_receipt = provider
        .send_transaction(
            TransactionRequest::default()
                .with_chain_id(deployment.chain_id)
                .with_to(fixture.stealth_address)
                .with_value(U256::from(1_000_000_000_000_000_000u128)),
        )
        .await?
        .get_receipt()
        .await?;
    assert!(funding_receipt.status());
    assert_eq!(funding_receipt.from, funder.address());

    let mut rng = StdRng::from_seed([0x52; 32]);
    let other_seed = AccountSeed::generate(&mut rng);
    let other_account = Account::from_seed(other_seed.expose_secret())?;
    let other_announcement =
        SchemeId3::announce(other_account.meta_address(), &rng.random::<[u8; 64]>())?;
    let foreign_receipt = provider
        .send_transaction(announce(&deployment, &other_announcement))
        .await?
        .get_receipt()
        .await?;
    assert!(foreign_receipt.status());

    for (scheme_id, metadata) in [
        (SchemeId3::SCHEME_ID + 1, fixture.metadata.to_vec()),
        (
            SchemeId3::SCHEME_ID,
            fixture.metadata[..fixture.metadata.len() - 1].to_vec(),
        ),
    ] {
        let call = IERC5564Announcer::announceCall {
            schemeId: U256::from(scheme_id),
            stealthAddress: fixture.stealth_address,
            ephemeralPubKey: fixture.ephemeral_pub_key.clone(),
            metadata: metadata.into(),
        };
        let receipt = provider
            .send_transaction(
                TransactionRequest::default()
                    .with_to(deployment.announcer)
                    .with_input(call.abi_encode()),
            )
            .await?
            .get_receipt()
            .await?;
        assert!(receipt.status());
    }

    let unfunded_announcement = SchemeId3::announce(&registered, &rng.random::<[u8; 64]>())?;
    let unfunded_receipt = provider
        .send_transaction(announce(&deployment, &unfunded_announcement))
        .await?
        .get_receipt()
        .await?;
    assert!(unfunded_receipt.status());
    assert_eq!(
        provider
            .get_balance(Address::from(unfunded_announcement.stealth_address))
            .await?,
        U256::ZERO
    );

    // Advance past the announcements before fetching finalized logs.
    provider.anvil_mine(Some(64), None).await?;
    let fetched = get_announcements(
        &provider,
        deployment,
        0..=u64::MAX,
        StreamedEventConfig::default(),
    )
    .try_collect::<Vec<_>>()
    .await?;
    assert_eq!(
        fetched
            .iter()
            .filter(|event| matches!(event, FetchEvent::Announcement(_)))
            .count(),
        3
    );
    assert!(matches!(
        fetched.last(),
        Some(FetchEvent::RangeFetched {
            rejected_logs: 1,
            ..
        })
    ));

    let scanner = account.scanner()?;
    let events = get_announcements_for_user(
        &scanner,
        stream::iter(fetched.into_iter().map(Ok::<_, FetchError>)),
    );
    pin_mut!(events);
    let mut matches = Vec::new();
    let mut completed = false;
    while let Some(event) = events.try_next().await? {
        assert!(!completed, "match followed range completion");
        match event {
            ScanEvent::Match(found) => matches.push(found),
            ScanEvent::RangeScanned { rejected_logs, .. } => {
                assert_eq!(rejected_logs, 1);
                completed = true;
            }
        }
    }
    assert!(completed);
    assert_eq!(matches.len(), 2);
    assert_eq!(matches[0].stealth_address(), fixture.stealth_address);
    assert_eq!(
        matches[0].record.transaction_hash,
        announcement_receipt.transaction_hash
    );
    assert_eq!(
        matches[1].stealth_address(),
        Address::from(unfunded_announcement.stealth_address)
    );
    assert_eq!(
        matches[1].record.transaction_hash,
        unfunded_receipt.transaction_hash
    );

    let private_key = account.spend_key(&matches[0].match_result)?;
    assert_eq!(
        private_key.expose_secret().as_slice(),
        fixture.spend_key.as_ref()
    );
    let spender = PrivateKeySigner::from_slice(private_key.expose_secret())?;
    assert_eq!(spender.address(), fixture.stealth_address);

    let balance_before = provider.get_balance(funder.address()).await?;
    let spender_provider = ProviderBuilder::new()
        .wallet(spender)
        .connect_http(url)
        .erased();
    let spend_receipt = spender_provider
        .send_transaction(
            TransactionRequest::default()
                .with_to(funder.address())
                .with_value(U256::from(1000)),
        )
        .await?
        .get_receipt()
        .await?;
    assert!(spend_receipt.status());
    assert_eq!(
        provider.get_balance(funder.address()).await?,
        balance_before + U256::from(1000)
    );
    Ok(())
}
