use anchor_lang::prelude::*;
use anchor_spl::associated_token::AssociatedToken;
use anchor_spl::token::{Mint, Token, TokenAccount};

use crate::constants::*;
use crate::errors::ZebraError;
use crate::state::{FeeSplitConfig, GlobalConfig};

#[derive(Accounts)]
pub struct InitializeConfig<'info> {
    /// Pays for account creation and becomes the initial admin. Later
    /// admin rotation is a follow-up instruction (admin_set_admin) that
    /// this MVP omits — admin key should be a Squads multisig from day one,
    /// per spec's recommendation, not rotated ad hoc.
    #[account(mut)]
    pub payer: Signer<'info>,

    #[account(
        init,
        payer = payer,
        space = GlobalConfig::LEN,
        seeds = [CONFIG_SEED],
        bump,
    )]
    pub global_config: Account<'info, GlobalConfig>,

    #[account(
        init,
        payer = payer,
        space = FeeSplitConfig::LEN,
        seeds = [FEE_SPLIT_SEED],
        bump,
    )]
    pub fee_split: Account<'info, FeeSplitConfig>,

    pub wzec_mint: Account<'info, Mint>,

    /// PDA used purely as a token-account authority (no data of its own),
    /// seeds = [STAMPEDE_AUTHORITY_SEED]. Never asserted to be a signer
    /// outside of a CPI this program itself issues.
    #[account(seeds = [STAMPEDE_AUTHORITY_SEED], bump)]
    pub stampede_authority: SystemAccount<'info>,

    #[account(
        init,
        payer = payer,
        associated_token::mint = wzec_mint,
        associated_token::authority = stampede_authority,
    )]
    pub stampede_vault: Account<'info, TokenAccount>,

    pub token_program: Program<'info, Token>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub system_program: Program<'info, System>,
}

pub fn handler(
    ctx: Context<InitializeConfig>,
    admin: Pubkey,
    keeper: Pubkey,
    protocol_treasury: Pubkey,
    zebra_mint: Pubkey,
    approved_swap_program: Pubkey,
    recovery_address: Pubkey,
    herd_bps_min: Option<u16>,
    treasury_bps_max: Option<u16>,
) -> Result<()> {
    let herd_bps_min = herd_bps_min.unwrap_or(DEFAULT_HERD_BPS_MIN);
    let treasury_bps_max = treasury_bps_max.unwrap_or(DEFAULT_TREASURY_BPS_MAX);

    require!(herd_bps_min <= DEFAULT_HERD_BPS, ZebraError::InvalidFeeSplitSum);
    require!(
        treasury_bps_max >= DEFAULT_TREASURY_BPS,
        ZebraError::InvalidFeeSplitSum
    );

    let global_config = &mut ctx.accounts.global_config;
    global_config.admin = admin;
    global_config.keeper = keeper;
    global_config.protocol_treasury = protocol_treasury;
    global_config.zebra_mint = zebra_mint;
    global_config.zebra_herd = Pubkey::default(); // set later via admin_set_zebra_herd
    global_config.wzec_mint = ctx.accounts.wzec_mint.key();
    global_config.approved_swap_program = approved_swap_program;
    global_config.stampede_vault = ctx.accounts.stampede_vault.key();
    global_config.recovery_address = recovery_address;
    global_config.herd_bps_min = herd_bps_min;
    global_config.treasury_bps_max = treasury_bps_max;
    global_config.paused = false;
    global_config.bump = ctx.bumps.global_config;

    // Sanity-check the compile-time defaults once, here, so a future edit to
    // constants.rs that breaks the invariant fails at the very first
    // initialize_config call in tests, not silently in prod.
    let default_sum = DEFAULT_HERD_BPS
        .checked_add(DEFAULT_STAMPEDE_BPS)
        .and_then(|s| s.checked_add(DEFAULT_TREASURY_BPS))
        .and_then(|s| s.checked_add(DEFAULT_DEPLOYER_BPS))
        .ok_or(ZebraError::MathOverflow)?;
    require_eq!(default_sum, FEE_SPLIT_DENOMINATOR, ZebraError::InvalidFeeSplitSum);

    let fee_split = &mut ctx.accounts.fee_split;
    fee_split.herd_bps = DEFAULT_HERD_BPS;
    fee_split.stampede_bps = DEFAULT_STAMPEDE_BPS;
    fee_split.treasury_bps = DEFAULT_TREASURY_BPS;
    fee_split.deployer_bps = DEFAULT_DEPLOYER_BPS;
    fee_split.bump = ctx.bumps.fee_split;

    emit!(crate::events::ConfigInitialized {
        admin,
        keeper,
        zebra_mint,
        wzec_mint: ctx.accounts.wzec_mint.key(),
    });

    Ok(())
}
