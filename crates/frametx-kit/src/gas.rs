//! Pinned gas limits from MSP HEAD [`gas_profile.py`].

pub const RECENT_ROOT_FRAME_GAS: u64 = 30_000;
pub const RECENT_ROOT_TUPLE_BYTES: usize = 72;
pub const VERIFY_FRAME_GAS: u64 = 320_000;
pub const VERIFY_FRAME_STATE_GAS: u64 = 195_840;
pub const SETTLE_FRAME_GAS: u64 = 2_000_000;
pub const SETTLE_FRAME_STATE_GAS: u64 = 550_000;
pub const CLAIM_FRAME_GAS: u64 = 100_000;
pub const CLAIM_FRAME_STATE_GAS: u64 = 183_600;
pub const EIP7825_TX_GAS_CAP: u64 = 1 << 24;
pub const ETHEX_MEMPOOL_MAX_BYTES: usize = 128 * 1024;
pub const TAIL_4337_FRAME_GAS: u64 = 8_000_000;
pub const TAIL_4337_FRAME_STATE_GAS: u64 = 8_000_000;
pub const WILD_FRAME_GAS: u64 = 500_000;
pub const WILD_FRAME_STATE_GAS: u64 = 100_000;
pub const SHIELD_VERIFY_GAS: u64 = 80_000;
pub const SIGNATURE_GAS_SECP256K1: u64 = 2_800;
pub const CLAIM_WITHDRAWAL_CALLDATA: usize = 36;
pub const TX_VALUE_COST: u64 = 6_000;
pub const FRAME_TX_INTRINSIC: u64 = 12_000;
pub const PER_FRAME_GAS: u64 = 475;
pub const RECENT_ROOT_WINDOW: u64 = 8192;
pub const HEGOTA_CHAIN_ID: u64 = 8141;
pub const RECENT_ROOT_ADDRESS: alloy::primitives::Address =
    alloy::primitives::address!("0x0000000000000000000000000000000000008272");

pub const FRAME_MODE_DEFAULT: u8 = 0;
pub const FRAME_MODE_VERIFY: u8 = 1;
pub const FRAME_MODE_SENDER: u8 = 2;
pub const APPROVE_EXECUTION_AND_PAYMENT: u8 = 0x03;
pub const SIG_SCHEME_SECP256K1: u8 = 1;
