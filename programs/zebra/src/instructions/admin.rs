//! Small admin-gated instructions grouped in one file since each is a
//! handful of lines: fee-split tuning, pause switches, the fixed-address
//! recovery path, binding $ZEBRA's own herd, and rotating the allowlisted
//! swap program. `admin` should be a multisig (e.g. Squads) from the first
//! `initialize_config` call — nothing here supports a safer "propose then
//! execute" rotation flow, by design scope, but nothing stops `admin` from
//! *being* a multisig's PDA.

use anchor_lang::prelude::*;
use anchor_spl::token::{self, Mint, Token, TokenAccount, Transfer};

use crate::constants::*;
use crate::errors::ZebraError;
use crate::events::{FeeSplitUpdated, GlobalPausedSet, HerdPausedSet, HerdRecovered, ZebraHerdSet};
use crate::state::{FeeSplitConfig, GlobalConfig, Herd};

// ---------------------------------------------------------------------
// admin_set_fee_split
// ---------------------------------------------------------------------

#[derive(Accounts)]
pub struct AdminSetFeeSplit<'info> {
    #[account(address = global_config.admin @ ZebraError::UnauthorizedAdmin)]
    pub admin: Signer<'info>,

    #[account(seeds = [CONFIG_SEED], bump = global_config.bump)]
    pub global_config: Account<'info, GlobalConfig>,

    #[account(mut, seeds = [FEE_SPLIT_SEED], bump = fee_split.bump)]
    pub fee_split: Account<'info, FeeSplitConfig>,
}

pub fn admin_set_fee_split(
    ctx: Context<AdminSetFeeSplit>,
    herd_bps: u16,
    stampede_bps: u16,
    treasury_bps: u16,
    deployer_bps: u16,
) -> Result<()> {
    let sum = herd_bps
        .checked_add(stampede_bps)
        .and_then(|s| s.checked_add(treasury_bps))
        .and_then(|s| s.checked_add(deployer_bps))
        .ok_or(ZebraError::MathOverflow)?;
    require_eq!(sum, FEE_SPLIT_DENOMINATOR, ZebraError::InvalidFeeSplitSum);
    require!(
        herd_bps >= ctx.accounts.global_config.herd_bps_min,
        ZebraError::HerdBpsBelowMin
    );
    require!(
        treasury_bps <= ctx.accounts.global_config.treasury_bps_max,
        ZebraError::TreasuryBpsAboveMax
    );

    let fee_split = &mut ctx.accounts.fee_split;
    fee_split.herd_bps = herd_bps;
    fee_split.stampede_bps = stampede_bps;
    fee_split.treasury_bps = treasury_bps;
    fee_split.deployer_bps = deployer_bps;

    emit!(FeeSplitUpdated {
        herd_bps,
        stampede_bps,
        treasury_bps,
        deployer_bps,
    });

    Ok(())
}

// ---------------------------------------------------------------------
// admin_set_paused (global kill switch)
// ---------------------------------------------------------------------

#[derive(Accounts)]
pub struct AdminSetPaused<'info> {
    #[account(address = global_config.admin @ ZebraError::UnauthorizedAdmin)]
    pub admin: Signer<'info>,

    #[account(mut, seeds = [CONFIG_SEED], bump = global_config.bump)]
    pub global_config: Account<'info, GlobalConfig>,
}

pub fn admin_set_paused(ctx: Context<AdminSetPaused>, paused: bool) -> Result<()> {
    ctx.accounts.global_config.paused = paused;
    emit!(GlobalPausedSet { paused });
    Ok(())
}

// ---------------------------------------------------------------------
// admin_set_herd_paused (per-coin kill switch)
// ---------------------------------------------------------------------

#[derive(Accounts)]
pub struct AdminSetHerdPaused<'info> {
    #[account(address = global_config.admin @ ZebraError::UnauthorizedAdmin)]
    pub admin: Signer<'info>,

    #[account(seeds = [CONFIG_SEED], bump = global_config.bump)]
    pub global_config: Account<'info, GlobalConfig>,

    pub coin_mint: Account<'info, Mint>,

    #[account(
        mut,
        seeds = [HERD_SEED, coin_mint.key().as_ref()],
        bump = herd.bump,
        has_one = coin_mint @ ZebraError::MintMismatch,
    )]
    pub herd: Account<'info, Herd>,
}

pub fn admin_set_herd_paused(ctx: Context<AdminSetHerdPaused>, paused: bool) -> Result<()> {
    ctx.accounts.herd.paused = paused;
    emit!(HerdPausedSet {
        coin_mint: ctx.accounts.coin_mint.key(),
        paused,
    });
    Ok(())
}

// ---------------------------------------------------------------------
// admin_recover_paused — drains a PAUSED herd's zec_vault to the single
// fixed recovery_address in GlobalConfig. Never an arbitrary destination,
// never callable on a herd that isn't paused. This is the only path in the
// whole program that moves herd_zec_vault funds without a burn happening.
// ---------------------------------------------------------------------

#[derive(Accounts)]
pub struct AdminRecoverPaused<'info> {
    #[account(address = global_config.admin @ ZebraError::UnauthorizedAdmin)]
    pub admin: Signer<'info>,

    #[account(seeds = [CONFIG_SEED], bump = global_config.bump)]
    pub global_config: Account<'info, GlobalConfig>,

    pub coin_mint: Account<'info, Mint>,

    #[account(
        mut,
        seeds = [HERD_SEED, coin_mint.key().as_ref()],
        bump = herd.bump,
        has_one = coin_mint @ ZebraError::MintMismatch,
    )]
    pub herd: Account<'info, Herd>,

    #[account(
        mut,
        constraint = herd_zec_vault.key() == herd.zec_vault @ ZebraError::MintMismatch,
    )]
    pub herd_zec_vault: Account<'info, TokenAccount>,

    /// Must be the ATA of (wZEC, global_config.recovery_address) — checked
    /// via `address`, never accepted as caller-supplied data.
    #[account(
        mut,
        address = anchor_spl::associated_token::get_associated_token_address(
            &global_config.recovery_address,
            &global_config.wzec_mint,
        ) @ ZebraError::RecoveryAddressMismatch,
    )]
    pub recovery_zec_ata: Account<'info, TokenAccount>,

    pub token_program: Program<'info, Token>,
}

pub fn admin_recover_paused(ctx: Context<AdminRecoverPaused>) -> Result<()> {
    require!(ctx.accounts.herd.paused, ZebraError::HerdNotPaused);

    let amount = ctx.accounts.herd_zec_vault.amount;
    require!(amount > 0, ZebraError::DustPayout);

    let coin_mint_key = ctx.accounts.coin_mint.key();
    let bump_arr = [ctx.accounts.herd.bump];
    let signer_seeds: &[&[&[u8]]] = &[&[HERD_SEED, coin_mint_key.as_ref(), &bump_arr]];

    token::transfer(
        CpiContext::new_with_signer(
            ctx.accounts.token_program.to_account_info(),
            Transfer {
                from: ctx.accounts.herd_zec_vault.to_account_info(),
                to: ctx.accounts.recovery_zec_ata.to_account_info(),
                authority: ctx.accounts.herd.to_account_info(),
            },
            signer_seeds,
        ),
        amount,
    )?;

    // Recovery drains the whole payout pool for this herd; zero it so
    // harvest_burn's pro-rata math (which divides by total_zec_deposited)
    // can't be called against a vault that no longer backs it.
    ctx.accounts.herd.total_zec_deposited = 0;

    emit!(HerdRecovered {
        coin_mint: coin_mint_key,
        amount,
        destination: ctx.accounts.global_config.recovery_address,
    });

    Ok(())
}

// ---------------------------------------------------------------------
// admin_set_zebra_herd — one-time wiring of GlobalConfig.zebra_herd once
// initialize_herd has been called for the $ZEBRA mint itself.
// ---------------------------------------------------------------------

#[derive(Accounts)]
pub struct AdminSetZebraHerd<'info> {
    #[account(address = global_config.admin @ ZebraError::UnauthorizedAdmin)]
    pub admin: Signer<'info>,

    #[account(mut, seeds = [CONFIG_SEED], bump = global_config.bump)]
    pub global_config: Account<'info, GlobalConfig>,

    #[account(constraint = herd.coin_mint == global_config.zebra_mint @ ZebraError::ZebraHerdMintMismatch)]
    pub herd: Account<'info, Herd>,
}

pub fn admin_set_zebra_herd(ctx: Context<AdminSetZebraHerd>) -> Result<()> {
    ctx.accounts.global_config.zebra_herd = ctx.accounts.herd.key();
    emit!(ZebraHerdSet {
        herd: ctx.accounts.herd.key(),
    });
    Ok(())
}

// ---------------------------------------------------------------------
// admin_set_swap_program — rotate the allowlisted AMM for
// execute_stampede_swap_and_burn without needing a program upgrade.
// ---------------------------------------------------------------------

#[derive(Accounts)]
pub struct AdminSetSwapProgram<'info> {
    #[account(address = global_config.admin @ ZebraError::UnauthorizedAdmin)]
    pub admin: Signer<'info>,

    #[account(mut, seeds = [CONFIG_SEED], bump = global_config.bump)]
    pub global_config: Account<'info, GlobalConfig>,
}

pub fn admin_set_swap_program(
    ctx: Context<AdminSetSwapProgram>,
    new_swap_program: Pubkey,
) -> Result<()> {
    ctx.accounts.global_config.approved_swap_program = new_swap_program;
    Ok(())
}
