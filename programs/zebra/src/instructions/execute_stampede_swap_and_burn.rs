//! Cranks the "stampede" bucket: swaps parked wZEC into $ZEBRA through
//! whichever AMM is allowlisted in `GlobalConfig.approved_swap_program`,
//! then burns whatever $ZEBRA came out.
//!
//! Deliberately NOT folded into `deposit_fee_split`. Coupling a fee deposit
//! (which must always succeed, or a legitimate trade/burn gets blocked) to a
//! live swap (which can fail for reasons entirely outside this program's
//! control — thin liquidity, a paused pool, slippage) would let a flaky AMM
//! brick unrelated instructions. Instead deposit_fee_split only ever parks
//! wZEC in `stampede_vault`; this instruction is a separate, permissionless
//! crank (anyone — typically the keeper, on a timer — can call it) that
//! sweeps that vault whenever it likes.
//!
//! This program has no first-party knowledge of any specific AMM's
//! instruction layout (pump.fun's bonding-curve swap instruction isn't a
//! stable public interface, and Jupiter route accounts are chosen
//! per-quote), so the swap step is a generic, trust-minimized passthrough:
//! the client supplies the raw instruction data and account list for
//! whatever route it already quoted off-chain, and this program:
//!   1. refuses to forward the CPI to anything but the single
//!      `approved_swap_program` fixed in GlobalConfig (never caller-chosen),
//!   2. signs the CPI as the `stampede_authority` PDA (so the swap program
//!      can move funds out of `stampede_vault`, which that PDA owns),
//!   3. verifies the *actual* result by diffing `zebra_scratch`'s token
//!      balance before/after, rather than trusting anything the swap
//!      instruction data claims about its own output.
//! Swapping in a different AMM later only ever means updating
//! `approved_swap_program` via `admin_set_swap_program` — never a program
//! upgrade to hardcode a new interface.

use anchor_lang::prelude::*;
use anchor_lang::solana_program::instruction::{AccountMeta, Instruction};
use anchor_lang::solana_program::program::invoke_signed;
use anchor_spl::associated_token::AssociatedToken;
use anchor_spl::token::{self, Burn, Mint, Token, TokenAccount};

use crate::constants::*;
use crate::errors::ZebraError;
use crate::events::StampedeBurned;
use crate::state::GlobalConfig;

#[derive(Accounts)]
pub struct ExecuteStampedeSwapAndBurn<'info> {
    #[account(mut)]
    pub payer: Signer<'info>,

    #[account(seeds = [CONFIG_SEED], bump = global_config.bump)]
    pub global_config: Account<'info, GlobalConfig>,

    /// PDA authority over both `stampede_vault` (source of the swap) and
    /// `zebra_scratch` (destination) — never a real keypair.
    #[account(seeds = [STAMPEDE_AUTHORITY_SEED], bump)]
    pub stampede_authority: SystemAccount<'info>,

    #[account(
        mut,
        address = global_config.stampede_vault @ ZebraError::MintMismatch,
    )]
    pub stampede_vault: Account<'info, TokenAccount>,

    #[account(mut, address = global_config.zebra_mint @ ZebraError::MintMismatch)]
    pub zebra_mint: Account<'info, Mint>,

    #[account(
        init_if_needed,
        payer = payer,
        associated_token::mint = zebra_mint,
        associated_token::authority = stampede_authority,
    )]
    pub zebra_scratch: Account<'info, TokenAccount>,

    /// Checked against the allowlist — this is the only thing standing
    /// between "swap into $ZEBRA" and "swap into whatever pool an attacker
    /// controls".
    #[account(address = global_config.approved_swap_program @ ZebraError::UnapprovedSwapProgram)]
    pub amm_program: UncheckedAccount<'info>,

    pub token_program: Program<'info, Token>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub system_program: Program<'info, System>,
    // Remaining accounts: the AMM route's own account list (pool, its
    // vaults, its own program-derived authorities, etc.), exactly as
    // quoted off-chain by the caller. Validated only by (a) amm_program
    // being the allowlisted program and (b) the post-swap balance check
    // below — never trusted individually.
}

pub fn handler(
    ctx: Context<ExecuteStampedeSwapAndBurn>,
    amount_in: u64,
    swap_instruction_data: Vec<u8>,
) -> Result<()> {
    require!(!ctx.accounts.global_config.paused, ZebraError::GlobalPaused);
    require!(amount_in > 0, ZebraError::ZeroDepositAmount);
    require!(
        amount_in <= ctx.accounts.stampede_vault.amount,
        ZebraError::InsufficientHerdVault
    );

    let zebra_before = ctx.accounts.zebra_scratch.amount;

    // Forward the CPI. `remaining_accounts` must include, at whatever
    // position the target AMM expects, `stampede_vault` (source, owned by
    // stampede_authority) and `zebra_scratch` (destination, same owner) —
    // the client is responsible for building a route that actually moves
    // wZEC out of stampede_vault into zebra_scratch; this program does not
    // and cannot inspect the swap's internal accounting, it only checks the
    // net result below.
    let account_metas: Vec<AccountMeta> = ctx
        .remaining_accounts
        .iter()
        .map(|acc| {
            if acc.is_writable {
                AccountMeta::new(*acc.key, acc.is_signer)
            } else {
                AccountMeta::new_readonly(*acc.key, acc.is_signer)
            }
        })
        .collect();

    let ix = Instruction {
        program_id: ctx.accounts.amm_program.key(),
        accounts: account_metas,
        data: swap_instruction_data,
    };

    let bump_arr = [ctx.bumps.stampede_authority];
    let seeds: &[&[u8]] = &[STAMPEDE_AUTHORITY_SEED, &bump_arr];

    invoke_signed(&ix, ctx.remaining_accounts, &[seeds])?;

    // Re-read the scratch account's on-chain balance post-CPI. This, not
    // the swap instruction's return data or logs, is the source of truth.
    ctx.accounts.zebra_scratch.reload()?;
    let zebra_after = ctx.accounts.zebra_scratch.amount;
    let zebra_bought = zebra_after
        .checked_sub(zebra_before)
        .ok_or(ZebraError::MathOverflow)?;
    require!(zebra_bought > 0, ZebraError::DustPayout);

    let burn_seeds: &[&[&[u8]]] = &[&[STAMPEDE_AUTHORITY_SEED, &bump_arr]];

    token::burn(
        CpiContext::new_with_signer(
            ctx.accounts.token_program.to_account_info(),
            Burn {
                mint: ctx.accounts.zebra_mint.to_account_info(),
                from: ctx.accounts.zebra_scratch.to_account_info(),
                authority: ctx.accounts.stampede_authority.to_account_info(),
            },
            burn_seeds,
        ),
        zebra_bought,
    )?;

    emit!(StampedeBurned {
        zebra_mint: ctx.accounts.zebra_mint.key(),
        zec_swapped: amount_in,
        zebra_burned: zebra_bought,
    });

    Ok(())
}
