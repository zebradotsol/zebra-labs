//! Anchor log events. The indexer (services/indexer) subscribes to program
//! logs and decodes these instead of re-deriving state by polling accounts —
//! cheaper and it gives a natural activity feed for /explore and
//! /coin/[mint]. Every state-changing instruction below emits exactly one of
//! these, unconditionally, as its last step.

use anchor_lang::prelude::*;

use crate::state::PayeeMode;

#[event]
pub struct ConfigInitialized {
    pub admin: Pubkey,
    pub keeper: Pubkey,
    pub zebra_mint: Pubkey,
    pub wzec_mint: Pubkey,
}

#[event]
pub struct ZebraHerdSet {
    pub herd: Pubkey,
}

#[event]
pub struct HerdInitialized {
    pub coin_mint: Pubkey,
    pub herd: Pubkey,
    pub payee_mode: PayeeMode,
    pub cap: u64,
}

#[event]
pub struct FeeSplitDeposited {
    pub coin_mint: Pubkey,
    pub herd: Pubkey,
    pub zec_amount: u64,
    pub herd_share: u64,
    pub stampede_share: u64,
    pub treasury_share: u64,
    pub deployer_share: u64,
    /// True if the herd_share was capped and the overflow was redirected to
    /// the protocol treasury instead (see deposit_fee_split).
    pub herd_cap_overflowed: bool,
}

#[event]
pub struct StampedeBurned {
    pub zebra_mint: Pubkey,
    pub zec_swapped: u64,
    pub zebra_burned: u64,
}

#[event]
pub struct HarvestBurned {
    pub coin_mint: Pubkey,
    pub herd: Pubkey,
    pub burner: Pubkey,
    pub burn_amount: u64,
    pub payout_zec: u64,
    pub supply_before_burn: u64,
    pub zcash_address: Option<String>,
}

#[event]
pub struct FeeSplitUpdated {
    pub herd_bps: u16,
    pub stampede_bps: u16,
    pub treasury_bps: u16,
    pub deployer_bps: u16,
}

#[event]
pub struct GlobalPausedSet {
    pub paused: bool,
}

#[event]
pub struct HerdPausedSet {
    pub coin_mint: Pubkey,
    pub paused: bool,
}

#[event]
pub struct HerdRecovered {
    pub coin_mint: Pubkey,
    pub amount: u64,
    pub destination: Pubkey,
}

#[event]
pub struct DeployerPayoutSwept {
    pub coin_mint: Pubkey,
    pub destination: Pubkey,
    pub amount: u64,
}

#[event]
pub struct HoldersSettled {
    pub coin_mint: Pubkey,
    pub total_paid: u64,
    pub holder_count: u32,
}
