use anchor_lang::prelude::*;

/// Global, singleton configuration for the whole Zebra program.
/// PDA seeds = ["config"].
#[account]
pub struct GlobalConfig {
    pub admin: Pubkey,
    pub keeper: Pubkey,
    pub protocol_treasury: Pubkey,
    /// $ZEBRA SPL mint (the protocol's own token, bought back and burned by
    /// the "stampede" bucket of every deposit_fee_split call).
    pub zebra_mint: Pubkey,
    /// Herd PDA whose coin_mint == zebra_mint. Pubkey::default() until set
    /// via `admin_set_zebra_herd` (chicken-and-egg: the herd for $ZEBRA
    /// itself can only be created, via initialize_herd, after $ZEBRA
    /// exists).
    pub zebra_herd: Pubkey,
    /// Wrapped-ZEC SPL mint. See docs/ARCHITECTURE.md for the bridge trust
    /// model (custodial wrap vs. a third-party bridge) — this program does
    /// not implement minting/burning of wZEC itself, it only ever moves
    /// already-minted wZEC between token accounts.
    pub wzec_mint: Pubkey,
    /// Only swap program CPI'd into by `execute_stampede_swap_and_burn`.
    /// Prevents a caller from supplying an arbitrary "AMM program" and
    /// routing the buy&burn leg into a pool they control.
    pub approved_swap_program: Pubkey,
    /// Global wZEC vault (ATA of a PDA, seeds=[STAMPEDE_AUTHORITY_SEED])
    /// that every herd's stampede_bps share is parked in by
    /// deposit_fee_split, until execute_stampede_swap_and_burn cranks a
    /// swap-into-$ZEBRA + burn. One shared vault, not one per herd: the
    /// stampede leg always buys & burns $ZEBRA itself, never the
    /// depositing coin, so there is nothing herd-specific about it.
    pub stampede_vault: Pubkey,
    /// Single fixed destination `admin_recover_paused` is allowed to drain
    /// a paused herd's vault into. Never taken from instruction data.
    pub recovery_address: Pubkey,
    /// Floor on herd_bps: admin_set_fee_split cannot push herd_bps below
    /// this (units: out of FEE_SPLIT_DENOMINATOR, i.e. "50" == 0.50%).
    pub herd_bps_min: u16,
    /// Ceiling on treasury_bps, same units.
    pub treasury_bps_max: u16,
    pub paused: bool,
    pub bump: u8,
}

impl GlobalConfig {
    pub const LEN: usize = 8 // discriminator
        + 32 // admin
        + 32 // keeper
        + 32 // protocol_treasury
        + 32 // zebra_mint
        + 32 // zebra_herd
        + 32 // wzec_mint
        + 32 // approved_swap_program
        + 32 // stampede_vault
        + 32 // recovery_address
        + 2 // herd_bps_min
        + 2 // treasury_bps_max
        + 1 // paused
        + 1; // bump
}

/// How the "deployer" bucket of a herd's fee split is paid out.
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum PayeeMode {
    /// Paid directly to the wallet that called initialize_herd (the
    /// mint-authority signer at plant time).
    Me,
    /// Paid to an explicit third-party wallet fixed at initialize_herd time.
    Wallet,
    /// Accrued in `holders_accrual` and settled later, pro-rata across
    /// holders, by the keeper's `keeper_settle_holders` batches.
    Holders,
}

/// One Herd per coin mint (including, for the protocol's own token, one
/// whose coin_mint == GlobalConfig.zebra_mint).
/// PDA seeds = ["herd", coin_mint].
#[account]
pub struct Herd {
    pub coin_mint: Pubkey,
    /// ATA of this Herd PDA, denominated in wZEC. Holds the herd_bps share
    /// of every deposit until burners draw it down via harvest_burn.
    pub zec_vault: Pubkey,
    /// Cumulative wZEC ever credited to this herd via deposit_fee_split,
    /// minus whatever has already been paid out by harvest_burn. This is
    /// the number harvest_burn's pro-rata formula is computed against, NOT
    /// zec_vault's live token balance (kept in sync with it by
    /// construction, but this field is the authoritative accounting value).
    pub total_zec_deposited: u64,
    /// Cumulative wZEC ever deposited into this herd, lifetime, even after
    /// payouts draw total_zec_deposited down. Analytics-only.
    pub lifetime_zec_deposited: u64,
    /// Ceiling on total_zec_deposited. Further herd-bucket deposits past
    /// this cap are redirected to the protocol treasury instead of being
    /// dropped (see deposit_fee_split) — the cap is a payout-pool ceiling
    /// for this coin, not a burn rule.
    pub cap: u64,
    pub payee_mode: PayeeMode,
    /// Only meaningful when payee_mode == Wallet.
    pub payee_wallet: Pubkey,
    /// ATA of this herd's deployer-authority PDA (seeds=["herd_deployer",
    /// coin_mint]), denominated in wZEC. Every deposit_fee_split call parks
    /// the deployer_bps share here regardless of payee_mode; sweep_deployer_
    /// payout (Me/Wallet) or keeper_settle_holders (Holders) drains it.
    pub deployer_vault: Pubkey,
    /// Only meaningful when payee_mode == Holders: accounting mirror of
    /// deployer_vault's balance, waiting on a keeper snapshot +
    /// keeper_settle_holders batch. Kept in sync with deployer_vault by
    /// construction (see deposit_fee_split / keeper_settle_holders) — this
    /// field, not a live token balance read, is what bounds a settle batch.
    pub holders_accrual: u64,
    pub paused: bool,
    pub bump: u8,
}

impl Herd {
    pub const LEN: usize = 8 // discriminator
        + 32 // coin_mint
        + 32 // zec_vault
        + 8 // total_zec_deposited
        + 8 // lifetime_zec_deposited
        + 8 // cap
        + 1 // payee_mode (borsh encodes a unit-variant enum as a 1-byte tag)
        + 32 // payee_wallet
        + 32 // deployer_vault
        + 8 // holders_accrual
        + 1 // paused
        + 1; // bump
}

/// Singleton fee-split configuration. PDA seeds = ["fee_split"].
/// All four fields are in "out of FEE_SPLIT_DENOMINATOR (200)" units and must
/// always sum to exactly FEE_SPLIT_DENOMINATOR — enforced by
/// admin_set_fee_split, never assumed elsewhere.
#[account]
pub struct FeeSplitConfig {
    pub herd_bps: u16,
    pub stampede_bps: u16,
    pub treasury_bps: u16,
    pub deployer_bps: u16,
    pub bump: u8,
}

impl FeeSplitConfig {
    pub const LEN: usize = 8 + 2 + 2 + 2 + 2 + 1;
}
