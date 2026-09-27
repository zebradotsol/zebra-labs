<div align="center">

# Zebra

### Herd · Stampede · Harvest — burn-to-earn fee-split for Solana tokens, stamped onto Zcash

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](#license)
[![Solana](https://img.shields.io/badge/Solana-Anchor%200.30-14F195?logo=solana&logoColor=white)](#stack)
[![Rust](https://img.shields.io/badge/Rust-2021-orange?logo=rust)](#stack)
[![Status: unaudited](https://img.shields.io/badge/status-unaudited%2C%20undeployed-red)](#status)
[![PRs Welcome](https://img.shields.io/badge/PRs-welcome-brightgreen.svg)](#contributing)

</div>

---

This repository holds the Solana Anchor source for **Zebra**: a fee-split
and burn-to-earn mechanism for launchpad-style tokens, plus the two
offchain services that make it a full loop.

Every coin that opts in gets a **herd** — a wZEC payout pool funded by a
slice of that coin's trading fees. Burning supply pays the burner back out
of that pool, pro-rata, atomically, in the same instruction as the burn.
A slice of every fee also buys and burns Zebra's own token (the
**stampede**), and — optionally — a burn can carry a Zcash address, in
which case an offchain **keeper** turns the burn into a real ZEC payment
and stamps the link between the two on-chain.

**Read [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) before anything
else — especially before pointing real funds at any of this.** The short
version: the Solana program is trustless; the Zcash payout is not, because
Zcash has no smart contracts, and the wZEC this program moves around is
only as good as whatever bridge mints it. This README covers what's in the
repo and how to build it; ARCHITECTURE.md covers what it actually
guarantees and what it doesn't.

## Table of contents

- [Deployment status](#deployment-status)
- [Components at a glance](#components-at-a-glance)
- [How it works](#how-it-works)
- [The Solana program](#the-solana-program)
- [The offchain services](#the-offchain-services)
- [Stack](#stack)
- [Repository layout](#repository-layout)
- [Building and testing](#building-and-testing)
- [Instructions reference](#instructions-reference)
- [Accounts (PDAs)](#accounts-pdas)
- [Design notes](#design-notes)
- [Security](#security)
- [Contributing](#contributing)
- [License](#license)

## Deployment status

| | |
|---|---|
| Program ID | `Fg6PaFpoGXkYsidMpWTK6W2BeZ7FEfcYkg476zPFsLnS` — **placeholder**, not a real deployment |
| Network | none yet — no devnet or mainnet deployment exists |
| wZEC bridge | unimplemented by design — see [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md#the-wzec-bridge) |
| Audit | none |

There is no "verify this address on chain" section here yet, unlike a
launchpad that's already live — because nothing has been deployed. Treat
everything below as source under active development, not a description of
a running system.

## Components at a glance

Zebra is not one trustless system spanning two chains — it's a trustless
Solana program wired to a trusted offchain keeper over a custodial bridge.
Knowing which piece is which matters more here than for a single-chain
launchpad:

| | Solana program | Keeper | wZEC bridge |
|---|---|---|---|
| **What it does** | Fee-split accounting, herd vaults, burn-for-payout, pausing, recovery | Watches burn memos, sends real ZEC on Zcash | Mints/burns the wZEC SPL token the program moves around |
| **Trust model** | Trustless — verifiable on-chain, no admin backdoor to user funds except one fixed recovery path | Trusted, centralized — holds real ZEC, no on-chain way to verify it acted correctly | Custodial by default (someone holds real ZEC 1:1 against wZEC) unless a real trustless bridge exists for Zcash |
| **Where** | `programs/zebra/` | `services/keeper/` | not in this repo — you provide `wzec_mint` |
| **Failure mode** | Reverts cleanly, no partial state | Offline keeper = delayed real-ZEC payout, burn itself still settles on Solana | Bridge insolvency/dishonesty = broken peg, no on-chain proof either way |

## How it works

A fixed 2.00% fee, once collected in wZEC and handed to
`deposit_fee_split`, splits four ways per the coin's herd:

| Bucket | Default | Goes to |
|---|---|---|
| **herd** | 0.50% | this coin's herd vault — the pool `harvest_burn` pays out of |
| **stampede** | 0.50% | parked, then swapped into $ZEBRA and burned (`execute_stampede_swap_and_burn`) |
| **treasury** | 0.20% | the protocol treasury |
| **deployer** | 0.80% | the coin's deployer — direct (`Me`), a fixed wallet (`Wallet`), or pro-rata across holders (`Holders`) |

*(All four bucket values are admin-tunable via `admin_set_fee_split`, bounded by `herd_bps_min` / `treasury_bps_max`, and always sum to exactly 200 — see [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md#program-level-invariants-enforced-in-programszebrasrc).)*

Anyone can then call `harvest_burn`: burn some of that coin's supply and
get paid, atomically in the same instruction, a pro-rata share of its herd
vault — computed against the coin's supply as it stood **before** the
burn, not after. Optionally, the burner attaches a Zcash address;
`harvest_burn` writes an on-chain Memo recording the burn and intended
payout, and the keeper (`services/keeper/`) watches for that memo and
sends the matching amount of real ZEC. See
[`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md#what-the-stamp-actually-proves)
for exactly what that stamp does and doesn't prove — the short version is
that a transparent Zcash address can't carry a memo at the protocol level,
so the only public, structured record of *why* a payment happened lives on
Solana, not on Zcash.

## The Solana program

`programs/zebra/` — an Anchor program, no admin path to user funds beyond
one fixed recovery address on an already-paused herd.

Key properties

- Every value-moving arithmetic path uses `u128` intermediates and
  `checked_*` operations exclusively — no raw `+`/`*`/`-` on real amounts
  anywhere in the program.
- `harvest_burn`'s payout formula reads the coin's supply **before**
  calling burn, never after — getting this ordering backwards is the
  easiest way to silently overpay every subsequent burner.
- `initialize_herd` can only be called by a coin's actual on-chain mint
  authority — checked against `coin_mint.mint_authority`, not assumed
  from naming or convention.
- `execute_stampede_swap_and_burn` only ever CPIs into the single
  allowlisted swap program in `GlobalConfig`, and verifies the swap's
  result by diffing a token balance before/after — never by trusting the
  swap instruction's own data.
- `admin_recover_paused` can only run on an already-paused herd, and can
  only ever send to the one fixed `recovery_address` baked into
  `GlobalConfig` — never a caller-supplied destination.

Core files

| File | Role |
|---|---|
| `src/lib.rs` | `#[program]` entrypoints — thin wrappers over `instructions/*` |
| `src/state.rs` | `GlobalConfig` / `Herd` / `FeeSplitConfig` account layouts |
| `src/events.rs` | `#[event]` structs the indexer decodes |
| `src/errors.rs` | `ZebraError` |
| `src/constants.rs` | PDA seeds, the 200-bps fee-split denominator, the Memo program ID |
| `src/instructions/initialize_config.rs` | Admin bootstrap: keys, mints, allowlisted swap program, shared stampede vault |
| `src/instructions/initialize_herd.rs` | Plants a herd for one coin — mint-authority gated |
| `src/instructions/deposit_fee_split.rs` | Splits a collected wZEC fee into the four buckets, capped and dust-safe |
| `src/instructions/execute_stampede_swap_and_burn.rs` | Permissionless crank: swap parked wZEC → $ZEBRA → burn |
| `src/instructions/harvest_burn.rs` | Burn a coin, get paid pro-rata from its herd, atomically |
| `src/instructions/sweep_deployer_payout.rs` | Permissionless crank: pay a `Me`/`Wallet` herd's deployer bucket |
| `src/instructions/keeper_settle_holders.rs` | Keeper-only batch payout for a `Holders` herd |
| `src/instructions/admin.rs` | Fee-split tuning, pause switches, fixed-address recovery, swap-program rotation |

## The offchain services

Zebra's on-chain half is trustless; these two are not part of that
guarantee — see [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) before
running either against real funds or relying on the indexer as a source of
truth.

**`services/keeper/`** — watches the Zebra program's logs for
`harvest_burn` memos that include a Zcash address, shape-validates the
address, converts the wZEC payout into a ZEC amount, and sends it via
`zcash-cli` (`sendtoaddress` for transparent destinations, `z_sendmany`
with a memo for shielded/unified ones). Runs in a safe **detect-only mode**
by default — it logs exactly what it would send and why, and never calls
`zcash-cli`, until real credentials are explicitly configured.

**`services/indexer/`** — subscribes to the same program's logs, hand-
decodes every `#[event]` Anchor emits (Borsh, no IDL dependency — see that
service's README for why), and serves the result over a small HTTP API:
`GET /explore` (every coin with indexed activity, most recent first) and
`GET /coin/:mint` (one coin's aggregated herd state + recent event feed).

## Stack

| Item | Value |
|---|---|
| Language | Rust (Anchor 0.30.1), edition 2021 |
| Chain | Solana |
| Offchain services | TypeScript / Node.js (ESM), `@solana/web3.js` |
| Access control | Admin/keeper keys in `GlobalConfig`, PDA-derived vault authorities |
| Safety | `checked_*` u128 math throughout, `has_one`/`address`/`seeds` constraints on every CPI target |
| Liquidity/bridge | External — this repo defines `wzec_mint` as a config value, it does not implement a bridge |

## Repository layout

```
.
├── README.md
├── docs/
│   └── ARCHITECTURE.md        # trust model, invariants, deployment checklist — read first
├── Anchor.toml
├── Cargo.toml
├── package.json                # root: anchor test tooling (mocha/chai)
├── tests/
│   └── zebra.ts                 # happy-path integration test scaffold
├── programs/
│   └── zebra/
│       ├── Cargo.toml
│       └── src/
│           ├── lib.rs
│           ├── state.rs
│           ├── events.rs
│           ├── errors.rs
│           ├── constants.rs
│           └── instructions/
│               ├── initialize_config.rs
│               ├── initialize_herd.rs
│               ├── deposit_fee_split.rs
│               ├── execute_stampede_swap_and_burn.rs
│               ├── harvest_burn.rs
│               ├── sweep_deployer_payout.rs
│               ├── keeper_settle_holders.rs
│               └── admin.rs
└── services/
    ├── keeper/                  # Solana burn-memo -> real Zcash payment
    │   ├── src/
    │   └── README.md
    └── indexer/                 # Solana events -> /explore + /coin/:mint HTTP API
        ├── src/
        └── README.md
```

## Building and testing

Requires the [Anchor CLI](https://www.anchor-lang.com/docs/installation)
(0.30.x) and the Solana CLI toolchain, in addition to Rust/cargo and
Node.js for the offchain services.

```bash
# Program: type-checks and borrow-checks with plain cargo (no Solana
# toolchain needed for this step); building the deployable .so needs
# `anchor build` / `cargo build-sbf`.
cargo check -p zebra

anchor build
anchor test              # local validator + tests/zebra.ts

# Offchain services
cd services/keeper  && npm install && cp .env.example .env && npm run dev
cd services/indexer && npm install && cp .env.example .env && npm run dev
```

## Instructions reference

| Instruction | Caller | What it does |
|---|---|---|
| `initialize_config` | anyone (once) | Bootstraps `GlobalConfig` + `FeeSplitConfig` + the shared stampede vault |
| `initialize_herd` | coin's mint authority | Plants a herd for one coin, sets its payee mode + deposit cap |
| `deposit_fee_split` | fee source's authority | Splits a collected wZEC fee into herd / stampede / treasury / deployer |
| `execute_stampede_swap_and_burn` | anyone (crank) | Swaps parked wZEC into $ZEBRA via the allowlisted AMM, burns the result |
| `harvest_burn` | any coin holder | Burns coin, gets paid pro-rata from the herd, atomically |
| `sweep_deployer_payout` | anyone (crank) | Pays a `Me`/`Wallet` herd's deployer bucket to its fixed payee |
| `keeper_settle_holders` | keeper only | Batch-pays a `Holders` herd's deployer bucket pro-rata across holders |
| `admin_set_fee_split` | admin | Retunes the four bps buckets within `herd_bps_min` / `treasury_bps_max` |
| `admin_set_paused` / `admin_set_herd_paused` | admin | Global / per-coin kill switch |
| `admin_recover_paused` | admin | Drains an already-**paused** herd's vault to the one fixed recovery address |
| `admin_set_zebra_herd` | admin | Binds `GlobalConfig.zebra_herd` once $ZEBRA's own herd exists |
| `admin_set_swap_program` | admin | Rotates the AMM `execute_stampede_swap_and_burn` is allowed to CPI into |

## Accounts (PDAs)

| Account | Seeds | Holds |
|---|---|---|
| `GlobalConfig` | `["config"]` | Admin/keeper/treasury keys, $ZEBRA + wZEC mints, allowlisted swap program, fixed recovery address, shared stampede vault, admin-tunable bps bounds |
| `Herd` | `["herd", coin_mint]` | One per coin: wZEC vault, running deposit total, cap, payee mode/wallet/vault, pause flag |
| `FeeSplitConfig` | `["fee_split"]` | The current four bps values — always summing to 200 |

## Design notes

- **Deposit and swap are decoupled.** `deposit_fee_split` only ever parks
  wZEC; `execute_stampede_swap_and_burn` is a separate, permissionless
  crank. Coupling a fee deposit (which must always succeed) to a live AMM
  swap (which can fail for reasons entirely outside this program's
  control) would let a flaky pool block unrelated instructions.
- **Payout math is supply-before-burn, always.** See `harvest_burn.rs` —
  this ordering is the single most load-bearing line in the program.
- **Every bucket lands somewhere real.** Herd-cap overflow and fee-split
  rounding dust both route to the treasury bucket explicitly — nothing
  about a deposited amount silently disappears.
- **The swap target is allowlisted, not caller-chosen.** A generic,
  trust-minimized CPI passthrough forwards to whatever AMM route the
  caller quoted off-chain, but only ever to the one program address fixed
  in `GlobalConfig`, and the result is checked by balance delta, never by
  trusting the swap's own instruction data.
- **Recovery is narrow on purpose.** The only path that moves a herd's
  vault without a matching burn requires the herd to already be paused,
  and can only ever target one fixed address — never anything from
  instruction data.

## Security

- Every CPI target account is checked via `has_one`, `address`, or PDA
  `seeds` constraints — never taken on the caller's word.
- Every value-moving arithmetic path uses checked `u128` math; see
  [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md#program-level-invariants-enforced-in-programszebrasrc)
  for the full invariant list.
- `admin_recover_paused` cannot target an arbitrary address.
- **This program has not been audited and is not deployed anywhere.** The
  `declare_id!` in `lib.rs` / `Anchor.toml` is a placeholder keypair-shaped
  string, not a real program ID — do not point real funds at it.
- The keeper and the wZEC bridge are trusted/centralized components by
  necessity, not by oversight — read
  [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) for exactly what that
  means before running either against real value.
- If you find a security issue, please report it privately rather than
  opening a public issue.

## Contributing

Issues and pull requests are welcome. If you're touching
`programs/zebra/src/events.rs`, update `services/indexer/src/events.ts`'s
hand-written schemas in the same change — nothing enforces that the two
stay in sync automatically (see that service's README).

## License

MIT — see individual file headers as they're added; no third-party vendor
code is currently bundled in this repository (unlike a project that
vendors OpenZeppelin/Uniswap sources directly, Zebra depends on
`anchor-lang` / `anchor-spl` as ordinary Cargo dependencies).

---

<div align="center">

If this project is useful to you, consider starring the repository.

</div>
# zebra-labs
# zebra-labs
# zebra-labs
