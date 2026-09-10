use alloy::{
    primitives::B256,
    providers::{Provider, ProviderBuilder},
};
#[cfg(feature = "saga-sync")]
use kohaku_tornadocash::indexer::saga_sync::SagaSyncSyncer;
use kohaku_tornadocash::{
    indexer::{
        remote::RemoteSyncer,
        rpc::RpcSyncer,
        syncer::{SyncEvent, SyncerBackend},
    },
    provider::pool::Pool,
};
use tracing::info;

const REMOTE_SYNC_BASE_URL: &str = "https://raw.githubusercontent.com/Robert-MacWha/privacy-protocols/refs/heads/sync-state/tornadocash-sync";
#[cfg(feature = "saga-sync")]
const SAGA_SYNC_BASE_URL: &str = "https://saga.fatsolutions.xyz/sepolia";

const SYNC_RANGE_BLOCKS: u64 = 10_000;

#[tokio::test]
#[ignore = "run with `cargo test --release -- --ignored`"]
async fn test_remote_sync_matches_rpc() -> Result<(), anyhow::Error> {
    assert_matches_rpc(&RemoteSyncer::new(REMOTE_SYNC_BASE_URL)).await
}

#[tokio::test]
#[cfg(feature = "saga-sync")]
#[ignore = "run with `cargo test --release -- --ignored`"]
async fn test_saga_sync_matches_rpc() -> Result<(), anyhow::Error> {
    assert_matches_rpc(&SagaSyncSyncer::new(SAGA_SYNC_BASE_URL)).await
}

/// Syncs `target_syncer` and a live RPC syncer over the same block range, asserting that the
/// target syncer's reported event log exactly matches the RPC syncer's.
///
/// The RPC sync is considered ground truth. `target_syncer` must have a contiguous subset of the
/// RPC syncer's events over the same range.
async fn assert_matches_rpc(target_syncer: &dyn SyncerBackend) -> Result<(), anyhow::Error> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_test_writer()
        .try_init()
        .ok();

    let rpc_url = std::env::var("RPC_URL_SEPOLIA").expect("RPC_URL_SEPOLIA must be set");
    let provider = ProviderBuilder::new().connect(&rpc_url).await?.erased();
    let pool = Pool::SEPOLIA_ETHER_01;

    let chain_head = provider.get_block_number().await?;
    let from_block = chain_head
        .saturating_sub(SYNC_RANGE_BLOCKS)
        .max(pool.deployed_block);

    //? Since the target syncer is expected to lag behind the chain head, we just sync up to the
    //? target syncer's latest block.
    let to_block = chain_head.min(target_syncer.latest_block(&pool).await?);

    if to_block <= from_block {
        info!("target syncer has no coverage in [{from_block}, {chain_head}); skipping");
        return Ok(());
    }

    let rpc_events = RpcSyncer::new(provider)
        .with_batch_size(10_000)
        .sync(&pool, from_block, to_block)
        .await?;
    let synced_events = target_syncer.sync(&pool, from_block, to_block).await?;

    info!(
        "comparing [{from_block}, {to_block}): synced {} event(s), RPC saw {}",
        synced_events.len(),
        rpc_events.len()
    );

    let (mut rpc_commitments, mut rpc_nullifiers) = commitments_and_nullifiers(&rpc_events);
    let (mut synced_commitments, mut synced_nullifiers) =
        commitments_and_nullifiers(&synced_events);

    rpc_commitments.sort();
    synced_commitments.sort();
    assert_eq!(
        synced_commitments, rpc_commitments,
        "commitments over [{from_block}, {to_block}) don't match RPC"
    );

    rpc_nullifiers.sort();
    synced_nullifiers.sort();
    assert_eq!(
        synced_nullifiers, rpc_nullifiers,
        "nullifiers over [{from_block}, {to_block}) don't match RPC"
    );

    Ok(())
}

fn commitments_and_nullifiers(events: &[SyncEvent]) -> (Vec<B256>, Vec<B256>) {
    let mut commitments = Vec::new();
    let mut nullifiers = Vec::new();

    for event in events {
        match event {
            SyncEvent::Deposit(d) => commitments.push(d.commitment),
            SyncEvent::Withdrawal(w) => nullifiers.push(w.nullifierHash),
        }
    }

    (commitments, nullifiers)
}
