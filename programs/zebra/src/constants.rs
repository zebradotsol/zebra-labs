//! Shared constants for the Zebra program.
//!
//! Fee-split basis points in this program are NOT expressed out of the usual
//! 10_000 bps == 100% convention. They are expressed out of
//! `FEE_SPLIT_DENOMINATOR` (200), because the four buckets (herd, stampede,
//! treasury, deployer) always partition one fixed 2.00% pool fee that is
//! skimmed upstream (by the AMM/pool) and handed to `deposit_fee_split` as
//! `zec_amount`. See spec: "Сумма всегда = 200 (2.00%)".

use anchor_lang::prelude::*;

/// Denominator every fee-split bucket is measured against. Must always equal
/// the sum of herd_bps + stampede_bps + treasury_bps + deployer_bps.
pub const FEE_SPLIT_DENOMINATOR: u16 = 200;

/// Default fee split, applied at `initialize_config` time.
pub const DEFAULT_HERD_BPS: u16 = 50; // 0.50%
pub const DEFAULT_STAMPEDE_BPS: u16 = 50; // 0.50%
pub const DEFAULT_TREASURY_BPS: u16 = 20; // 0.20%
pub const DEFAULT_DEPLOYER_BPS: u16 = 80; // 0.80%

/// Default admin-tunable bounds. `herd_bps` can never be pushed below this
/// (protects burners' payout pool from being starved), `treasury_bps` can
/// never be pushed above this (caps the protocol's own extractable share).
pub const DEFAULT_HERD_BPS_MIN: u16 = 50;
pub const DEFAULT_TREASURY_BPS_MAX: u16 = 40;

// ---- PDA seeds ----
pub const CONFIG_SEED: &[u8] = b"config";
pub const FEE_SPLIT_SEED: &[u8] = b"fee_split";
pub const HERD_SEED: &[u8] = b"herd";
pub const STAMPEDE_AUTHORITY_SEED: &[u8] = b"stampede";
pub const HERD_DEPLOYER_SEED: &[u8] = b"herd_deployer";

/// Max length of the optional Zcash address string accepted by
/// `harvest_burn`. Both transparent (t1/t3, 35 chars) and shielded (u1/zs,
/// up to ~160 chars encoded) addresses fit comfortably under this.
pub const MAX_ZCASH_ADDRESS_LEN: usize = 200;

/// Max holders settled in a single `keeper_settle_holders` call. Bounded so
/// the instruction can never blow the transaction account-lock limit or
/// compute budget; the keeper paginates larger snapshots across multiple
/// calls.
pub const MAX_HOLDERS_PER_SETTLE_BATCH: usize = 20;

/// Well-known SPL Memo program ID
/// ("MemoSq4gqABAXKb96qnH8TysNcWxMyWCqXgDLGmfcHr"). Called via a raw CPI
/// built by hand in `harvest_burn` rather than through `anchor_spl::memo`,
/// so this program's memo usage never depends on that wrapper's own version
/// churn.
pub const MEMO_PROGRAM_ID: Pubkey = pubkey!("MemoSq4gqABAXKb96qnH8TysNcWxMyWCqXgDLGmfcHr");
