use anchor_lang::prelude::*;
use anchor_spl::associated_token::AssociatedToken;
use anchor_spl::token::{Mint, Token, TokenAccount};

use crate::constants::*;
use crate::errors::ZebraError;
use crate::events::HerdInitialized;
use crate::state::{GlobalConfig, Herd, PayeeMode};

#[derive(Accounts)]
pub struct InitializeHerd<'info> {
    #[account(mut)]
    pub payer: Signer<'info>,

    /// Must equal coin_mint's on-chain mint authority. This is the property
    /// the spec relies on for "only the page that planted the coin can
    /// declare its herd" — checked below, not just implied by naming.
    pub mint_authority: Signer<'info>,

    pub coin_mint: Account<'info, Mint>,

    #[account(seeds = [CONFIG_SEED], bump = global_config.bump)]
    pub global_config: Account<'info, GlobalConfig>,

    #[account(
        init,
        payer = payer,
        space = Herd::LEN,
        seeds = [HERD_SEED, coin_mint.key().as_ref()],
        bump,
    )]
    pub herd: Account<'info, Herd>,

    #[account(address = global_config.wzec_mint @ ZebraError::WzecMintMismatch)]
    pub wzec_mint: Account<'info, Mint>,

    #[account(
        init,
        payer = payer,
        associated_token::mint = wzec_mint,
        associated_token::authority = herd,
    )]
    pub zec_vault: Account<'info, TokenAccount>,

    /// PDA used purely as a token-account authority for this herd's
    /// deployer-bucket vault, seeds = [HERD_DEPLOYER_SEED, coin_mint].
    /// Deliberately distinct from `herd` itself: `herd`'s ATA in wZEC is
    /// already `zec_vault` (one ATA per owner+mint pair), so the
    /// deployer-bucket vault needs its own PDA to get its own address.
    #[account(seeds = [HERD_DEPLOYER_SEED, coin_mint.key().as_ref()], bump)]
    pub deployer_authority: SystemAccount<'info>,

    #[account(
        init,
        payer = payer,
        associated_token::mint = wzec_mint,
        associated_token::authority = deployer_authority,
    )]
    pub deployer_vault: Account<'info, TokenAccount>,

    pub token_program: Program<'info, Token>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub system_program: Program<'info, System>,
}

pub fn handler(
    ctx: Context<InitializeHerd>,
    payee_mode: PayeeMode,
    payee_wallet: Option<Pubkey>,
    cap: u64,
) -> Result<()> {
    // Ordering note: Anchor's `init` constraint on `herd` already runs the
    // PDA derivation + creation before this handler body executes, so by
    // the time we get here the herd is guaranteed fresh (init fails if the
    // account already exists) — no separate "already initialized" check
    // needed.

    let authority = ctx
        .accounts
        .coin_mint
        .mint_authority
        .ok_or(ZebraError::MissingMintAuthority)?;
    require_keys_eq!(
        authority,
        ctx.accounts.mint_authority.key(),
        ZebraError::InvalidMintAuthority
    );

    match payee_mode {
        PayeeMode::Wallet => {
            require!(payee_wallet.is_some(), ZebraError::MissingPayeeWallet);
        }
        PayeeMode::Me | PayeeMode::Holders => {
            require!(payee_wallet.is_none(), ZebraError::UnexpectedPayeeWallet);
        }
    }

    let herd = &mut ctx.accounts.herd;
    herd.coin_mint = ctx.accounts.coin_mint.key();
    herd.zec_vault = ctx.accounts.zec_vault.key();
    herd.total_zec_deposited = 0;
    herd.lifetime_zec_deposited = 0;
    herd.cap = cap;
    herd.payee_mode = payee_mode;
    herd.payee_wallet = match payee_mode {
        PayeeMode::Wallet => payee_wallet.unwrap(),
        PayeeMode::Me => ctx.accounts.mint_authority.key(),
        PayeeMode::Holders => Pubkey::default(),
    };
    herd.deployer_vault = ctx.accounts.deployer_vault.key();
    herd.holders_accrual = 0;
    herd.paused = false;
    herd.bump = ctx.bumps.herd;

    emit!(HerdInitialized {
        coin_mint: herd.coin_mint,
        herd: herd.key(),
        payee_mode,
        cap,
    });

    Ok(())
}
