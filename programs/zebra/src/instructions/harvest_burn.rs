//! The core burner-facing instruction: burn some of a coin's supply, get
//! paid a pro-rata share of that coin's herd vault, atomically, in one
//! instruction. Optionally records the burn + a Zcash payout address in an
//! on-chain Memo, which the keeper (services/keeper) watches for and acts
//! on by sending real ZEC on the Zcash chain — see docs/ARCHITECTURE.md for
//! exactly what that memo does and does not prove.

use anchor_lang::prelude::*;
use anchor_lang::solana_program::instruction::Instruction;
use anchor_lang::solana_program::program::invoke;
use anchor_spl::token::{self, Burn, Mint, Token, TokenAccount, Transfer};

use crate::constants::*;
use crate::errors::ZebraError;
use crate::events::HarvestBurned;
use crate::state::{GlobalConfig, Herd};

#[derive(Accounts)]
pub struct HarvestBurn<'info> {
    #[account(mut)]
    pub burner: Signer<'info>,

    #[account(seeds = [CONFIG_SEED], bump = global_config.bump)]
    pub global_config: Account<'info, GlobalConfig>,

    #[account(mut)]
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

    #[account(
        mut,
        token::mint = coin_mint,
        token::authority = burner,
    )]
    pub burner_coin_account: Account<'info, TokenAccount>,

    #[account(
        mut,
        token::mint = global_config.wzec_mint,
        token::authority = burner,
    )]
    pub burner_zec_ata: Account<'info, TokenAccount>,

    pub token_program: Program<'info, Token>,
    // memo_program is intentionally NOT a typed Program<'info, Memo> account
    // here: the memo CPI below is unconditional-instruction-existence but
    // conditional-on-caller-intent (only fires if zcash_address is Some),
    // and Anchor requires every account in the struct to be present on
    // every call. We invoke the Memo program directly by its well-known ID
    // instead (see MEMO_PROGRAM_ID) so callers who pass zcash_address=None
    // don't need to supply it at all.
}

pub fn handler(
    ctx: Context<HarvestBurn>,
    burn_amount: u64,
    zcash_address: Option<String>,
) -> Result<()> {
    require!(!ctx.accounts.global_config.paused, ZebraError::GlobalPaused);
    require!(!ctx.accounts.herd.paused, ZebraError::HerdPaused);
    require!(burn_amount > 0, ZebraError::ZeroBurnAmount);

    if let Some(addr) = &zcash_address {
        require!(
            !addr.is_empty() && addr.len() <= MAX_ZCASH_ADDRESS_LEN,
            ZebraError::InvalidZcashAddressLength
        );
    }

    // ---- Step 1: read supply BEFORE burning. ----
    // This ordering is load-bearing: burn_checked below reduces
    // coin_mint.supply, so reading it after would understate the
    // denominator and overpay every burner from that point on. See spec:
    // "supply читается до burn CPI, иначе burn уменьшит supply и формула
    // даст неверный (завышенный) payout самому себе."
    let supply_before_burn = ctx.accounts.coin_mint.supply;
    require!(supply_before_burn > 0, ZebraError::DivideByZero);
    require!(burn_amount <= supply_before_burn, ZebraError::MathOverflow);

    // ---- Step 2: pro-rata payout, checked u128 math, floor division. ----
    let herd = &ctx.accounts.herd;
    let payout_u128 = (herd.total_zec_deposited as u128)
        .checked_mul(burn_amount as u128)
        .ok_or(ZebraError::MathOverflow)?
        .checked_div(supply_before_burn as u128)
        .ok_or(ZebraError::DivideByZero)?;
    let payout = u64::try_from(payout_u128).map_err(|_| ZebraError::MathOverflow)?;

    require!(payout > 0, ZebraError::DustPayout);
    require!(
        payout <= herd.total_zec_deposited,
        ZebraError::InsufficientHerdVault
    );
    require!(
        payout <= ctx.accounts.herd_zec_vault.amount,
        ZebraError::InsufficientHerdVault
    );

    // ---- Step 3: burn the coin (CPI #1). ----
    token::burn(
        CpiContext::new(
            ctx.accounts.token_program.to_account_info(),
            Burn {
                mint: ctx.accounts.coin_mint.to_account_info(),
                from: ctx.accounts.burner_coin_account.to_account_info(),
                authority: ctx.accounts.burner.to_account_info(),
            },
        ),
        burn_amount,
    )?;

    // ---- Step 4: pay out from the herd vault (CPI #2), signed by the
    // herd PDA. ----
    let coin_mint_key = ctx.accounts.coin_mint.key();
    let bump_arr = [ctx.accounts.herd.bump];
    let signer_seeds: &[&[&[u8]]] = &[&[HERD_SEED, coin_mint_key.as_ref(), &bump_arr]];

    token::transfer(
        CpiContext::new_with_signer(
            ctx.accounts.token_program.to_account_info(),
            Transfer {
                from: ctx.accounts.herd_zec_vault.to_account_info(),
                to: ctx.accounts.burner_zec_ata.to_account_info(),
                authority: ctx.accounts.herd.to_account_info(),
            },
            signer_seeds,
        ),
        payout,
    )?;

    // ---- Step 5: update herd accounting. ----
    let herd = &mut ctx.accounts.herd;
    herd.total_zec_deposited = herd
        .total_zec_deposited
        .checked_sub(payout)
        .ok_or(ZebraError::MathOverflow)?;

    // ---- Step 6: optional Memo CPI (CPI #3), same transaction. ----
    // See docs/ARCHITECTURE.md "What the stamp actually proves" — this
    // memo is the structured half of the burn-stamp; the Zcash-side
    // transaction the keeper later sends is the other half, and neither
    // one alone is the full public record the whitepaper describes.
    if let Some(addr) = &zcash_address {
        let memo_text = format!(
            "ZEBRA_BURN|mint={}|burn_amount={}|payout_zec={}|zcash_addr={}|ts={}",
            coin_mint_key,
            burn_amount,
            payout,
            addr,
            Clock::get()?.unix_timestamp,
        );
        let ix = Instruction {
            program_id: MEMO_PROGRAM_ID,
            accounts: vec![],
            data: memo_text.into_bytes(),
        };
        invoke(&ix, &[])?;
    }

    emit!(HarvestBurned {
        coin_mint: coin_mint_key,
        herd: herd.key(),
        burner: ctx.accounts.burner.key(),
        burn_amount,
        payout_zec: payout,
        supply_before_burn,
        zcash_address,
    });

    Ok(())
}
