//! Zebra: a Solana Anchor program implementing the herd / stampede / harvest
//! fee-split described in docs/ARCHITECTURE.md, plus the burn-stamp memo
//! that the offchain keeper (services/keeper) turns into a real Zcash
//! transaction. This program owns everything that can be made
//! deterministic and trustless on Solana; it does not, and cannot, touch
//! Zcash itself — see docs/ARCHITECTURE.md for the full trust model,
//! especially around wZEC and what the "stamp" does and does not prove.

use anchor_lang::prelude::*;

pub mod constants;
pub mod errors;
pub mod events;
pub mod instructions;
pub mod state;

use instructions::*;
use state::PayeeMode;

// Placeholder program ID — replace with the real deployed address (update
// this and Anchor.toml together) before any devnet/mainnet deployment.
declare_id!("Fg6PaFpoGXkYsidMpWTK6W2BeZ7FEfcYkg476zPFsLnS");

#[program]
pub mod zebra {
    use super::*;

    /// One-time program bootstrap: sets admin/keeper/treasury keys, the
    /// $ZEBRA and wZEC mints, the allowlisted swap program, the fixed
    /// recovery address, and the default fee split (50/50/20/80, summing
    /// to the fixed 200-bps total). Also creates the shared stampede
    /// vault.
    pub fn initialize_config(
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
        instructions::initialize_config::handler(
            ctx,
            admin,
            keeper,
            protocol_treasury,
            zebra_mint,
            approved_swap_program,
            recovery_address,
            herd_bps_min,
            treasury_bps_max,
        )
    }

    /// Plants a herd for `coin_mint`. Must be signed by coin_mint's actual
    /// mint authority — this is what makes "only the page that planted the
    /// coin can declare its herd" true. `cap` bounds how much wZEC this
    /// herd's payout pool can ever hold at once (further deposits past it
    /// spill to the protocol treasury instead of being dropped).
    pub fn initialize_herd(
        ctx: Context<InitializeHerd>,
        payee_mode: PayeeMode,
        payee_wallet: Option<Pubkey>,
        cap: u64,
    ) -> Result<()> {
        instructions::initialize_herd::handler(ctx, payee_mode, payee_wallet, cap)
    }

    /// Splits an already-collected wZEC fee (`zec_amount`) into the herd,
    /// stampede, treasury and deployer buckets per the current
    /// FeeSplitConfig, capping the herd bucket at `herd.cap` and routing
    /// dust/overflow to the treasury. Does not itself swap or burn
    /// anything — see `execute_stampede_swap_and_burn` and
    /// `sweep_deployer_payout` / `keeper_settle_holders` for what drains
    /// the buckets this parks funds into.
    pub fn deposit_fee_split<'info>(
        ctx: Context<'_, '_, '_, 'info, DepositFeeSplit<'info>>,
        zec_amount: u64,
    ) -> Result<()> {
        instructions::deposit_fee_split::handler(ctx, zec_amount)
    }

    /// Permissionless crank: forwards `amount_in` wZEC from the shared
    /// stampede vault into the allowlisted AMM (via a caller-supplied,
    /// off-chain-quoted route in `swap_instruction_data` /
    /// `remaining_accounts`), then burns whatever $ZEBRA actually landed
    /// in the scratch account — verified by balance delta, not by trusting
    /// the swap's own instruction data.
    pub fn execute_stampede_swap_and_burn<'info>(
        ctx: Context<'_, '_, '_, 'info, ExecuteStampedeSwapAndBurn<'info>>,
        amount_in: u64,
        swap_instruction_data: Vec<u8>,
    ) -> Result<()> {
        instructions::execute_stampede_swap_and_burn::handler(ctx, amount_in, swap_instruction_data)
    }

    /// Burns `burn_amount` of `coin_mint` and, atomically in the same
    /// instruction, pays the burner a pro-rata share of that coin's herd
    /// vault (computed against supply as it stood *before* the burn).
    /// Optionally writes a Memo linking the burn to a Zcash address, which
    /// the keeper watches for.
    pub fn harvest_burn(
        ctx: Context<HarvestBurn>,
        burn_amount: u64,
        zcash_address: Option<String>,
    ) -> Result<()> {
        instructions::harvest_burn::handler(ctx, burn_amount, zcash_address)
    }

    /// Permissionless crank: pays a `PayeeMode::Me` / `PayeeMode::Wallet`
    /// herd's entire deployer_vault balance to its fixed payee_wallet.
    pub fn sweep_deployer_payout(ctx: Context<SweepDeployerPayout>) -> Result<()> {
        instructions::sweep_deployer_payout::handler(ctx)
    }

    /// Keeper-only batch payout for `PayeeMode::Holders` herds: pays
    /// `amounts[i]` to `ctx.remaining_accounts[i]`, bounded by
    /// `herd.holders_accrual` and `MAX_HOLDERS_PER_SETTLE_BATCH`.
    pub fn keeper_settle_holders<'info>(
        ctx: Context<'_, '_, 'info, 'info, KeeperSettleHolders<'info>>,
        amounts: Vec<u64>,
    ) -> Result<()> {
        instructions::keeper_settle_holders::handler(ctx, amounts)
    }

    // ---- Admin-gated instructions (see instructions/admin.rs) ----

    pub fn admin_set_fee_split(
        ctx: Context<AdminSetFeeSplit>,
        herd_bps: u16,
        stampede_bps: u16,
        treasury_bps: u16,
        deployer_bps: u16,
    ) -> Result<()> {
        instructions::admin::admin_set_fee_split(ctx, herd_bps, stampede_bps, treasury_bps, deployer_bps)
    }

    pub fn admin_set_paused(ctx: Context<AdminSetPaused>, paused: bool) -> Result<()> {
        instructions::admin::admin_set_paused(ctx, paused)
    }

    pub fn admin_set_herd_paused(ctx: Context<AdminSetHerdPaused>, paused: bool) -> Result<()> {
        instructions::admin::admin_set_herd_paused(ctx, paused)
    }

    pub fn admin_recover_paused(ctx: Context<AdminRecoverPaused>) -> Result<()> {
        instructions::admin::admin_recover_paused(ctx)
    }

    pub fn admin_set_zebra_herd(ctx: Context<AdminSetZebraHerd>) -> Result<()> {
        instructions::admin::admin_set_zebra_herd(ctx)
    }

    pub fn admin_set_swap_program(
        ctx: Context<AdminSetSwapProgram>,
        new_swap_program: Pubkey,
    ) -> Result<()> {
        instructions::admin::admin_set_swap_program(ctx, new_swap_program)
    }
}
