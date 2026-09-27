use anchor_lang::prelude::*;
use anchor_spl::associated_token::AssociatedToken;
use anchor_spl::token::{self, Mint, Token, TokenAccount, Transfer};

use crate::constants::*;
use crate::errors::ZebraError;
use crate::events::FeeSplitDeposited;
use crate::state::{FeeSplitConfig, GlobalConfig, Herd, PayeeMode};

#[derive(Accounts)]
pub struct DepositFeeSplit<'info> {
    #[account(mut)]
    pub payer: Signer<'info>,

    #[account(seeds = [CONFIG_SEED], bump = global_config.bump)]
    pub global_config: Account<'info, GlobalConfig>,

    #[account(seeds = [FEE_SPLIT_SEED], bump = fee_split.bump)]
    pub fee_split: Account<'info, FeeSplitConfig>,

    pub coin_mint: Account<'info, Mint>,

    #[account(
        mut,
        seeds = [HERD_SEED, coin_mint.key().as_ref()],
        bump = herd.bump,
        has_one = coin_mint @ ZebraError::MintMismatch,
    )]
    pub herd: Account<'info, Herd>,

    /// Whoever is authorized to move `zec_amount` out of `fee_source`. In
    /// production this is the keeper (or a pool-fee hook's PDA, if the pool
    /// program CPIs in directly) — this instruction only trusts that this
    /// signer actually owns/controls fee_source, it does not itself decide
    /// who is allowed to call deposit_fee_split.
    pub fee_authority: Signer<'info>,

    #[account(
        mut,
        token::mint = global_config.wzec_mint,
        token::authority = fee_authority,
    )]
    pub fee_source: Account<'info, TokenAccount>,

    #[account(
        mut,
        constraint = herd_zec_vault.key() == herd.zec_vault @ ZebraError::MintMismatch,
    )]
    pub herd_zec_vault: Account<'info, TokenAccount>,

    #[account(
        mut,
        constraint = deployer_vault.key() == herd.deployer_vault @ ZebraError::MintMismatch,
    )]
    pub deployer_vault: Account<'info, TokenAccount>,

    #[account(
        mut,
        address = global_config.stampede_vault @ ZebraError::MintMismatch,
    )]
    pub stampede_vault: Account<'info, TokenAccount>,

    /// Not read as data, only used to derive/verify `treasury_zec_ata`'s
    /// authority — the real destination of record is
    /// `global_config.protocol_treasury`, never caller-supplied.
    #[account(address = global_config.protocol_treasury)]
    pub protocol_treasury: UncheckedAccount<'info>,

    #[account(
        init_if_needed,
        payer = payer,
        associated_token::mint = wzec_mint,
        associated_token::authority = protocol_treasury,
    )]
    pub treasury_zec_ata: Account<'info, TokenAccount>,

    #[account(address = global_config.wzec_mint @ ZebraError::WzecMintMismatch)]
    pub wzec_mint: Account<'info, Mint>,

    pub token_program: Program<'info, Token>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub system_program: Program<'info, System>,
}

pub fn handler<'info>(
    ctx: Context<'_, '_, '_, 'info, DepositFeeSplit<'info>>,
    zec_amount: u64,
) -> Result<()> {
    require!(!ctx.accounts.global_config.paused, ZebraError::GlobalPaused);
    require!(!ctx.accounts.herd.paused, ZebraError::HerdPaused);
    require!(zec_amount > 0, ZebraError::ZeroDepositAmount);

    let fee_split = &ctx.accounts.fee_split;
    let denom = FEE_SPLIT_DENOMINATOR as u128;
    let amt = zec_amount as u128;

    // ---- 1. Compute the four shares in u128, floor division ----
    let raw_herd = amt
        .checked_mul(fee_split.herd_bps as u128)
        .ok_or(ZebraError::MathOverflow)?
        .checked_div(denom)
        .ok_or(ZebraError::DivideByZero)?;
    let raw_stampede = amt
        .checked_mul(fee_split.stampede_bps as u128)
        .ok_or(ZebraError::MathOverflow)?
        .checked_div(denom)
        .ok_or(ZebraError::DivideByZero)?;
    let raw_treasury = amt
        .checked_mul(fee_split.treasury_bps as u128)
        .ok_or(ZebraError::MathOverflow)?
        .checked_div(denom)
        .ok_or(ZebraError::DivideByZero)?;
    let raw_deployer = amt
        .checked_mul(fee_split.deployer_bps as u128)
        .ok_or(ZebraError::MathOverflow)?
        .checked_div(denom)
        .ok_or(ZebraError::DivideByZero)?;

    // Floor division on four buckets can leave up to 3 lamports of wZEC
    // (base units) unallocated to rounding. Route that dust to the
    // treasury bucket rather than silently dropping it — nothing about
    // zec_amount may vanish.
    let allocated = raw_herd
        .checked_add(raw_stampede)
        .and_then(|s| s.checked_add(raw_treasury))
        .and_then(|s| s.checked_add(raw_deployer))
        .ok_or(ZebraError::MathOverflow)?;
    let dust = amt.checked_sub(allocated).ok_or(ZebraError::MathOverflow)?;
    let raw_treasury = raw_treasury.checked_add(dust).ok_or(ZebraError::MathOverflow)?;

    let mut herd_share = u64::try_from(raw_herd).map_err(|_| ZebraError::MathOverflow)?;
    let stampede_share = u64::try_from(raw_stampede).map_err(|_| ZebraError::MathOverflow)?;
    let mut treasury_share = u64::try_from(raw_treasury).map_err(|_| ZebraError::MathOverflow)?;
    let deployer_share = u64::try_from(raw_deployer).map_err(|_| ZebraError::MathOverflow)?;

    // ---- 2. Cap the herd bucket; redirect overflow to treasury ----
    // (bind to a temporary, not `ctx.accounts.herd` directly yet, so the
    // token-transfer CPIs below can still borrow other `ctx.accounts`
    // fields without fighting a live mutable borrow of `herd`)
    let herd_total_before = ctx.accounts.herd.total_zec_deposited;
    let herd_cap = ctx.accounts.herd.cap;
    let projected = herd_total_before
        .checked_add(herd_share)
        .ok_or(ZebraError::MathOverflow)?;
    let herd_cap_overflowed = projected > herd_cap;
    if herd_cap_overflowed {
        let allowed = herd_cap.saturating_sub(herd_total_before);
        let overflow = herd_share.checked_sub(allowed).ok_or(ZebraError::MathOverflow)?;
        herd_share = allowed;
        treasury_share = treasury_share
            .checked_add(overflow)
            .ok_or(ZebraError::MathOverflow)?;
    }

    // ---- 3. Move the tokens ----
    // Snapshot the AccountInfos we need first: this keeps every CPI call
    // below free of any live borrow of `ctx.accounts.herd`, so step 4 can
    // take a plain `&mut` on it afterwards with no borrow-checker fights.
    let token_program_info = ctx.accounts.token_program.to_account_info();
    let fee_source_info = ctx.accounts.fee_source.to_account_info();
    let fee_authority_info = ctx.accounts.fee_authority.to_account_info();
    let herd_zec_vault_info = ctx.accounts.herd_zec_vault.to_account_info();
    let stampede_vault_info = ctx.accounts.stampede_vault.to_account_info();
    let treasury_zec_ata_info = ctx.accounts.treasury_zec_ata.to_account_info();
    let deployer_vault_info = ctx.accounts.deployer_vault.to_account_info();

    let do_transfer = |to: AccountInfo<'info>, amount: u64| -> Result<()> {
        token::transfer(
            CpiContext::new(
                token_program_info.clone(),
                Transfer {
                    from: fee_source_info.clone(),
                    to,
                    authority: fee_authority_info.clone(),
                },
            ),
            amount,
        )
    };

    if herd_share > 0 {
        do_transfer(herd_zec_vault_info, herd_share)?;
    }
    if stampede_share > 0 {
        do_transfer(stampede_vault_info, stampede_share)?;
    }
    if treasury_share > 0 {
        do_transfer(treasury_zec_ata_info, treasury_share)?;
    }
    if deployer_share > 0 {
        do_transfer(deployer_vault_info, deployer_share)?;
    }

    // ---- 4. Update herd accounting ----
    let herd = &mut ctx.accounts.herd;
    herd.total_zec_deposited = herd
        .total_zec_deposited
        .checked_add(herd_share)
        .ok_or(ZebraError::MathOverflow)?;
    herd.lifetime_zec_deposited = herd
        .lifetime_zec_deposited
        .checked_add(herd_share)
        .ok_or(ZebraError::MathOverflow)?;
    if herd.payee_mode == PayeeMode::Holders {
        herd.holders_accrual = herd
            .holders_accrual
            .checked_add(deployer_share)
            .ok_or(ZebraError::MathOverflow)?;
    }

    emit!(FeeSplitDeposited {
        coin_mint: herd.coin_mint,
        herd: herd.key(),
        zec_amount,
        herd_share,
        stampede_share,
        treasury_share,
        deployer_share,
        herd_cap_overflowed,
    });

    Ok(())
}
