//! Keeper-only batch payout for `PayeeMode::Holders` herds. The keeper
//! builds a holder snapshot off-chain (see services/indexer — walking
//! `getProgramAccounts` for every coin_mint holder on-chain, per call, is
//! neither cheap nor reliable enough to do inside an instruction) and
//! settles it in bounded batches so no single call can blow the tx
//! account-lock limit or compute budget.
//!
//! `amounts[i]` pays out to `ctx.remaining_accounts[i]`, which must be that
//! holder's own wZEC token account — passed as plain accounts (not named
//! fields) precisely because the holder set is dynamic and keeper-supplied,
//! not something callers can be individually typed against.

use anchor_lang::prelude::*;
use anchor_spl::token::{self, Mint, Token, TokenAccount, Transfer};

use crate::constants::*;
use crate::errors::ZebraError;
use crate::events::HoldersSettled;
use crate::state::{GlobalConfig, Herd, PayeeMode};

#[derive(Accounts)]
pub struct KeeperSettleHolders<'info> {
    #[account(address = global_config.keeper @ ZebraError::UnauthorizedKeeper)]
    pub keeper: Signer<'info>,

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

    #[account(seeds = [HERD_DEPLOYER_SEED, coin_mint.key().as_ref()], bump)]
    pub deployer_authority: SystemAccount<'info>,

    #[account(
        mut,
        constraint = deployer_vault.key() == herd.deployer_vault @ ZebraError::MintMismatch,
    )]
    pub deployer_vault: Account<'info, TokenAccount>,

    pub token_program: Program<'info, Token>,
    // remaining_accounts: one wZEC token account per entry in `amounts`,
    // same order. Each is deserialized and mint-checked by hand below,
    // since a dynamic-length holder list can't be declared as named,
    // individually typed fields.
}

pub fn handler<'info>(
    ctx: Context<'_, '_, 'info, 'info, KeeperSettleHolders<'info>>,
    amounts: Vec<u64>,
) -> Result<()> {
    require!(!ctx.accounts.global_config.paused, ZebraError::GlobalPaused);
    require!(!ctx.accounts.herd.paused, ZebraError::HerdPaused);
    require!(
        ctx.accounts.herd.payee_mode == PayeeMode::Holders,
        ZebraError::WrongPayeeModeForSettle
    );

    let n = amounts.len();
    require!(n > 0, ZebraError::HolderListLengthMismatch);
    require!(n <= MAX_HOLDERS_PER_SETTLE_BATCH, ZebraError::HolderBatchTooLarge);
    require!(
        ctx.remaining_accounts.len() == n,
        ZebraError::HolderAccountsMismatch
    );

    let mut total: u64 = 0;
    for amount in amounts.iter() {
        require!(*amount > 0, ZebraError::ZeroDepositAmount);
        total = total.checked_add(*amount).ok_or(ZebraError::MathOverflow)?;
    }
    require!(
        total <= ctx.accounts.herd.holders_accrual,
        ZebraError::HolderPayoutExceedsAccrual
    );
    require!(
        total <= ctx.accounts.deployer_vault.amount,
        ZebraError::InsufficientHerdVault
    );

    let coin_mint_key = ctx.accounts.coin_mint.key();
    let bump_arr = [ctx.bumps.deployer_authority];
    let signer_seeds: &[&[&[u8]]] =
        &[&[HERD_DEPLOYER_SEED, coin_mint_key.as_ref(), &bump_arr]];

    let wzec_mint = ctx.accounts.global_config.wzec_mint;
    let token_program_info = ctx.accounts.token_program.to_account_info();
    let deployer_vault_info = ctx.accounts.deployer_vault.to_account_info();
    let deployer_authority_info = ctx.accounts.deployer_authority.to_account_info();

    for (amount, holder_account_info) in amounts.iter().zip(ctx.remaining_accounts.iter()) {
        let holder_token_account = Account::<TokenAccount>::try_from(holder_account_info)
            .map_err(|_| ZebraError::HolderAccountsMismatch)?;
        require_keys_eq!(
            holder_token_account.mint,
            wzec_mint,
            ZebraError::WzecMintMismatch
        );

        token::transfer(
            CpiContext::new_with_signer(
                token_program_info.clone(),
                Transfer {
                    from: deployer_vault_info.clone(),
                    to: holder_account_info.clone(),
                    authority: deployer_authority_info.clone(),
                },
                signer_seeds,
            ),
            *amount,
        )?;
    }

    let herd = &mut ctx.accounts.herd;
    herd.holders_accrual = herd
        .holders_accrual
        .checked_sub(total)
        .ok_or(ZebraError::MathOverflow)?;

    emit!(HoldersSettled {
        coin_mint: coin_mint_key,
        total_paid: total,
        holder_count: n as u32,
    });

    Ok(())
}
