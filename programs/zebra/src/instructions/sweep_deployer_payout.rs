//! Permissionless crank: drains a herd's `deployer_vault` to
//! `herd.payee_wallet`'s wZEC ATA. Only valid for `PayeeMode::Me` /
//! `PayeeMode::Wallet` — a `PayeeMode::Holders` herd's deployer_vault is
//! drained per-holder by `keeper_settle_holders` instead.
//!
//! Anyone can call this (there is nothing to gate: the destination is fixed
//! by `herd.payee_wallet`, never supplied by the caller), so a deployer
//! doesn't have to keep polling and signing their own claim transactions —
//! anyone (the frontend, the keeper, the deployer themselves) can crank it.

use anchor_lang::prelude::*;
use anchor_spl::associated_token::AssociatedToken;
use anchor_spl::token::{self, Mint, Token, TokenAccount, Transfer};

use crate::constants::*;
use crate::errors::ZebraError;
use crate::events::DeployerPayoutSwept;
use crate::state::{GlobalConfig, Herd, PayeeMode};

#[derive(Accounts)]
pub struct SweepDeployerPayout<'info> {
    #[account(mut)]
    pub payer: Signer<'info>,

    #[account(seeds = [CONFIG_SEED], bump = global_config.bump)]
    pub global_config: Account<'info, GlobalConfig>,

    pub coin_mint: Account<'info, Mint>,

    #[account(
        seeds = [HERD_SEED, coin_mint.key().as_ref()],
        bump = herd.bump,
        has_one = coin_mint @ ZebraError::MintMismatch,
    )]
    pub herd: Account<'info, Herd>,

    #[account(seeds = [HERD_DEPLOYER_SEED, coin_mint.key().as_ref()], bump)]
    pub deployer_authority: SystemAccount<'info>,

    #[account(
        mut,
        constraint = deployer_vault.key() == herd.deployer_vault @ ZebraError::MintMismatch,
    )]
    pub deployer_vault: Account<'info, TokenAccount>,

    #[account(address = global_config.wzec_mint @ ZebraError::WzecMintMismatch)]
    pub wzec_mint: Account<'info, Mint>,

    #[account(
        init_if_needed,
        payer = payer,
        associated_token::mint = wzec_mint,
        associated_token::authority = payee_wallet,
    )]
    pub payee_zec_ata: Account<'info, TokenAccount>,

    /// Not read as data — only used to derive/verify `payee_zec_ata`'s
    /// authority. Checked against `herd.payee_wallet`, never caller-chosen.
    #[account(address = herd.payee_wallet)]
    pub payee_wallet: UncheckedAccount<'info>,

    pub token_program: Program<'info, Token>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub system_program: Program<'info, System>,
}

pub fn handler(ctx: Context<SweepDeployerPayout>) -> Result<()> {
    require!(
        ctx.accounts.herd.payee_mode == PayeeMode::Me
            || ctx.accounts.herd.payee_mode == PayeeMode::Wallet,
        ZebraError::WrongPayeeModeForSweep
    );

    let amount = ctx.accounts.deployer_vault.amount;
    require!(amount > 0, ZebraError::DustPayout);

    let coin_mint_key = ctx.accounts.coin_mint.key();
    let bump_arr = [ctx.bumps.deployer_authority];
    let signer_seeds: &[&[&[u8]]] =
        &[&[HERD_DEPLOYER_SEED, coin_mint_key.as_ref(), &bump_arr]];

    token::transfer(
        CpiContext::new_with_signer(
            ctx.accounts.token_program.to_account_info(),
            Transfer {
                from: ctx.accounts.deployer_vault.to_account_info(),
                to: ctx.accounts.payee_zec_ata.to_account_info(),
                authority: ctx.accounts.deployer_authority.to_account_info(),
            },
            signer_seeds,
        ),
        amount,
    )?;

    emit!(DeployerPayoutSwept {
        coin_mint: coin_mint_key,
        destination: ctx.accounts.payee_wallet.key(),
        amount,
    });

    Ok(())
}
