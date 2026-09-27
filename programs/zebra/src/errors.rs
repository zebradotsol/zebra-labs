use anchor_lang::prelude::*;

#[error_code]
pub enum ZebraError {
    #[msg("Signer is not the configured admin")]
    UnauthorizedAdmin,
    #[msg("Signer is not the configured keeper")]
    UnauthorizedKeeper,
    #[msg("Protocol is globally paused")]
    GlobalPaused,
    #[msg("This herd is paused")]
    HerdPaused,
    #[msg("This herd is not paused, recovery is not permitted")]
    HerdNotPaused,
    #[msg("Fee split buckets must sum to FEE_SPLIT_DENOMINATOR (200)")]
    InvalidFeeSplitSum,
    #[msg("herd_bps cannot be pushed below the configured floor")]
    HerdBpsBelowMin,
    #[msg("treasury_bps cannot be pushed above the configured ceiling")]
    TreasuryBpsAboveMax,
    #[msg("Computed payout rounds to zero, burn amount too small relative to supply")]
    DustPayout,
    #[msg("Burn amount must be greater than zero")]
    ZeroBurnAmount,
    #[msg("Deposit amount must be greater than zero")]
    ZeroDepositAmount,
    #[msg("Herd's ZEC vault does not hold enough to cover this payout")]
    InsufficientHerdVault,
    #[msg("Checked arithmetic overflowed")]
    MathOverflow,
    #[msg("Division by zero (mint supply or denominator was zero)")]
    DivideByZero,
    #[msg("Deposit would push herd's cumulative deposits above its cap")]
    HerdCapExceeded,
    #[msg("payee_wallet is required when payee_mode is Wallet")]
    MissingPayeeWallet,
    #[msg("payee_wallet must be omitted unless payee_mode is Wallet")]
    UnexpectedPayeeWallet,
    #[msg("This fee's source mint does not match the herd's coin mint")]
    MintMismatch,
    #[msg("This token account's mint does not match the configured wZEC mint")]
    WzecMintMismatch,
    #[msg("Swap program is not the one approved in GlobalConfig")]
    UnapprovedSwapProgram,
    #[msg("holders and amounts arrays must be the same, non-zero length")]
    HolderListLengthMismatch,
    #[msg("Batch size exceeds MAX_HOLDERS_PER_SETTLE_BATCH")]
    HolderBatchTooLarge,
    #[msg("Remaining accounts must supply exactly one token account per holder")]
    HolderAccountsMismatch,
    #[msg("Sum of holder payouts exceeds herd.holders_accrual")]
    HolderPayoutExceedsAccrual,
    #[msg("Recovery destination does not match GlobalConfig.recovery_address")]
    RecoveryAddressMismatch,
    #[msg("mint_authority signer does not match coin_mint's on-chain mint authority")]
    InvalidMintAuthority,
    #[msg("coin_mint has no mint authority set, cannot verify initializer")]
    MissingMintAuthority,
    #[msg("Zcash address exceeds MAX_ZCASH_ADDRESS_LEN or is empty")]
    InvalidZcashAddressLength,
    #[msg("sweep_deployer_payout only applies to PayeeMode::Me or PayeeMode::Wallet herds")]
    WrongPayeeModeForSweep,
    #[msg("keeper_settle_holders only applies to PayeeMode::Holders herds")]
    WrongPayeeModeForSettle,
    #[msg("herd already initialized for this coin mint")]
    HerdAlreadyInitialized,
    #[msg("zebra_herd in GlobalConfig must point at a Herd whose coin_mint is the $ZEBRA mint")]
    ZebraHerdMintMismatch,
}
