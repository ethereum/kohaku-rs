use std::sync::Arc;

use alloy::{
    primitives::U256,
    providers::{Provider, ProviderBuilder},
    sol,
};
use kohaku_fork_kit::railgun::deploy_railgun;
use kohaku_railgun::{
    account::signer::RailgunSigner, builder::RailgunBuilder, caip::AssetId,
    indexer::syncer::RpcSyncer,
};
use rand::random;

sol! {
    #[sol(rpc)]
    contract WETH {
        function approve(address guy, uint256 wad) external returns (bool);
        function deposit() external payable;
    }
}

/// A local chain checks RPC event decoding, note decryption, on-chain root
/// verification, and restoration of the caller-owned checkpoint together.
#[tokio::test]
async fn test_sync_utxo() {
    let provider = ProviderBuilder::new().connect_anvil_with_wallet().erased();
    let chain = deploy_railgun(provider.clone()).await.unwrap();
    let asset = AssetId::Erc20(chain.wrapped_base_token);
    let signer =
        kohaku_railgun::account::signer::PrivateKeySigner::new_evm(random(), random(), chain.id);
    let syncer = Arc::new(
        RpcSyncer::new(chain.clone(), provider.clone())
            .with_batch_size(1000)
            .with_batch_delay(std::time::Duration::ZERO),
    );
    let mut railgun = RailgunBuilder::new(chain.clone(), provider.clone())
        .with_utxo_syncer(syncer.clone())
        .build()
        .await
        .unwrap();
    railgun.register(signer.clone()).await.unwrap();

    let weth = WETH::new(chain.wrapped_base_token, provider.clone());
    weth.deposit()
        .value(U256::from(100_000))
        .send()
        .await
        .unwrap()
        .get_receipt()
        .await
        .unwrap();
    weth.approve(chain.railgun_smart_wallet, U256::from(100_000))
        .send()
        .await
        .unwrap()
        .get_receipt()
        .await
        .unwrap();
    for tx in railgun
        .shield()
        .shield(signer.address(), asset, 100_000)
        .build(&mut rand::rng())
        .unwrap()
    {
        provider
            .send_transaction(tx.into())
            .await
            .unwrap()
            .get_receipt()
            .await
            .unwrap();
    }

    railgun.sync().await.unwrap();
    let checkpoint = railgun.state();
    let head = provider.get_block_number().await.unwrap();
    assert_eq!(checkpoint.utxo.synced_block, head);
    assert_eq!(railgun.unspent_notes(signer.address()).await.len(), 1);

    let mut restored = RailgunBuilder::new(chain, provider)
        .with_state(checkpoint)
        .with_utxo_syncer(syncer)
        .build()
        .await
        .unwrap();
    restored.register(signer.clone()).await.unwrap();
    restored.sync().await.unwrap();
    let notes = restored.unspent_notes(signer.address()).await;
    assert_eq!(notes.len(), 1);
    assert_eq!(notes[0].note.value, 99_750);
    assert_eq!(restored.synced_block(), head);
}
