pub mod admin;
pub mod deposit_fee_split;
pub mod execute_stampede_swap_and_burn;
pub mod harvest_burn;
pub mod initialize_config;
pub mod initialize_herd;
pub mod keeper_settle_holders;
pub mod sweep_deployer_payout;

// Blanket glob re-exports, matching standard Anchor project layout: the
// `#[program]` / `#[derive(Accounts)]` macros generate sibling items
// (`__client_accounts_*`, `__cpi_client_*`) alongside each Accounts struct
// that IDL/CPI codegen expects to find re-exported at `instructions::*`.
// Every instruction file's actual entry point is named `handler`, so these
// globs do re-export several same-named `handler` functions into one
// namespace — that is an "ambiguous glob re-exports" warning, not an
// error, and is harmless as long as nothing calls the bare `handler` name
// (lib.rs always calls through the full module path, e.g.
// `instructions::deposit_fee_split::handler(...)`).
pub use admin::*;
pub use deposit_fee_split::*;
pub use execute_stampede_swap_and_burn::*;
pub use harvest_burn::*;
pub use initialize_config::*;
pub use initialize_herd::*;
pub use keeper_settle_holders::*;
pub use sweep_deployer_payout::*;
