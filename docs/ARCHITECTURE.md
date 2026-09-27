# Zebra — architecture and trust model

Zebra is not one trustless system spanning two chains. It is a trustless
Solana program (herd / stampede / harvest fee-split, all of it enforced
on-chain, checked math throughout) wired to a **trusted offchain keeper**
that makes real payments on Zcash, over a **custodial wZEC bridge**. Anyone
evaluating or extending this repo should start from that sentence, because
it's the one thing the original one-line pitch ("burn a coin, get paid in
real ZEC, atomically") elides. This document is where the elided parts go.

## Why Zcash can't be a fourth component here

Zcash is a UTXO chain with no smart-contract layer — no PDAs, no
instructions, no program-owned state, nothing programmable. The only thing
any process can do on the Zcash side is construct and broadcast an
ordinary (transparent or shielded) transaction. So "the Zcash side of
Zebra" is not a contract; it's a payment a keeper process makes by hand,
after watching Solana. That reframes the whole system as three pieces:

1. **The Solana program** (`programs/zebra/`) — all of the actual logic:
   fee-split accounting, the herd payout pool, the burn-for-payout
   mechanism, pausing, recovery. Trustless in the normal Solana-program
   sense (anyone can verify it, no admin backdoor moves user funds except
   the one explicitly fixed recovery path — see below).
2. **The keeper** (`services/keeper/`) — an offchain process that watches
   the Solana program's logs and, when it sees a burn with a Zcash address
   attached, sends real ZEC. This is a **trusted, centralized component**.
   See "Keeper trust model" below.
3. **The wZEC bridge** — whatever mints the "wrapped ZEC" SPL token the
   Solana program actually moves around. Not implemented in this repo.
   See "The wZEC bridge" below.

## The wZEC bridge

There is no trustless, light-client-verified bridge for Zcash today —
Zcash has no compatible light-client proof system for a Wormhole-style
guardian/relayer-free bridge, unlike, say, a chain with a fast BFT
finality gadget a relayer can cheaply verify. That leaves two honest
options, and this repo does not pick one for you (`GlobalConfig.wzec_mint`
is just a Pubkey the deployer supplies):

- **Custodial wrap** (the default assumption everywhere else in this doc):
  someone — the protocol team, ideally a multisig — holds real ZEC at a
  Zcash address and mints/burns wZEC 1:1 against it on request. This is
  the same trust model as every custodially-wrapped asset (wBTC being the
  most familiar example): the peg holds exactly as long as the custodian
  is solvent and honest, and there is no on-chain proof of either. **Say
  this plainly to users** — "bridged representation" is a euphemism that
  hides the actual risk.
- **A third-party bridge**, if and when one actually has a real, audited
  ZEC route. Then wZEC is just an SPL token like any other and this
  program's only obligation is to move it around correctly, which it
  already does — the bridge's own security is out of scope for Zebra
  entirely.

Either way: **`WZEC_TO_ZEC_RATE` in the keeper's config is not a rounding
nicety, it's where the peg assumption becomes an operational number.** If
the real bridge isn't exactly 1:1, or doesn't share ZEC's 8 decimals, that
config value is the only thing standing between "correct payout" and
"burner gets paid the wrong amount of ZEC."

## Keeper trust model

The keeper (`services/keeper/`) holds — or is configured to command — the
private key(s) controlling real ZEC. It is a single point of trust and a
single point of failure, by construction, not by an implementation gap
that a future PR fixes:

- **Trust**: whoever runs the keeper with real credentials configured can
  send that ZEC anywhere, at any time, regardless of what the Solana
  program's memos say. The program has no way to check that a payout it
  requested (via memo) actually happened, and no way to check that a
  payout that *did* happen matches a real burn rather than the keeper
  operator paying themselves. This is a "trust the operator" system, same
  as every custodial exchange.
- **Failure**: if the keeper process is offline, burns still work
  (harvest_burn still burns the coin and pays the wZEC payout on Solana
  atomically, unconditionally) but the promised real-ZEC payment simply
  doesn't happen until the keeper catches up — there is no expiry, no
  retry guarantee, and (in this skeleton) no durable retry queue at all
  once a send attempt fails (see `services/keeper/README.md`).

Mitigations that are real options, not implemented here: run the keeper's
Zcash wallet as a multisig (Zcash transparent addresses support P2SH
multisig); run multiple independent keeper instances that must jointly
sign (needs custom coordination, not "just run two copies"); log every
intended payout somewhere publicly auditable *before* attempting to send,
so an operator's misbehavior is at least detectable after the fact (the
Solana memo already partially serves this — see next section — but only
for burns that specify a Zcash address).

## What the stamp actually proves

The whitepaper-level pitch is "a permanent record on Zcash, linking a burn
to a payment." What's actually true is narrower, and the gap matters for
anyone relying on this as a public verification mechanism:

- **A transparent Zcash address (`t1…`/`t3…`) has no memo field at the
  protocol level.** Full stop — this isn't a wallet limitation, it's a
  property of transparent (Bitcoin-style) outputs. A payout to a
  transparent address is *only* a plain value transfer. There is no
  structured data on the Zcash chain saying why it happened.
- **A shielded/unified recipient *can* receive a memo** (up to 512 bytes,
  encrypted to the recipient), and the keeper (`zcashClient.ts`) attaches
  one (`zebra-burn-stamp:<mint>:<solana-signature>`) when sending to one.
  But that memo is encrypted — only the recipient can read it. It is not
  a public record either; it's a private note to the person being paid.
- **The actual structured, publicly-verifiable record — mint, burn
  amount, payout amount, destination address, timestamp — lives entirely
  on Solana**, in the Memo-program CPI `harvest_burn` makes
  (`ZEBRA_BURN|mint=...|burn_amount=...|payout_zec=...|zcash_addr=...|ts=...`).

So "the stamp" is really: a public, structured fact on Solana ("this burn
happened, and it should result in this ZEC payment to this address") plus
a private fact on Zcash ("this transfer happened"). Verifying that the
second fact actually followed from the first means checking a keeper's
behavior, not reading it off the Zcash chain — because nothing about a
plain ZEC transfer (to a transparent address, especially) says *why* it
was sent. If public, on-chain-verifiable provenance for the payment reason
matters more than payer privacy, the fix is deliberate: prefer transparent
addresses precisely because it means everyone can *see* the payment
happened (even though not *why*), and publish the Solana signature
alongside it out-of-band; or accept that a shielded payout's memo is
seen by the recipient only, and the Solana memo is the only public half
of the record. This is a genuine design tradeoff, not a bug — pick based
on whether the priority is public auditability or payer privacy.

## Program-level invariants (enforced in `programs/zebra/src/`)

These are the properties the Solana program actually guarantees on-chain;
everything above this line is a Solana-side fact regardless of keeper or
bridge behavior.

- `herd_bps + stampede_bps + treasury_bps + deployer_bps` is always exactly
  `FEE_SPLIT_DENOMINATOR` (200 = 2.00%) — enforced in `admin_set_fee_split`
  and asserted against the compile-time defaults in `initialize_config`.
- `herd_bps` can never be pushed below `GlobalConfig.herd_bps_min`;
  `treasury_bps` can never be pushed above `GlobalConfig.treasury_bps_max`
  — both checked in `admin_set_fee_split`, both admin-tunable only within
  those bounds.
- `harvest_burn`'s payout formula reads `coin_mint.supply` **before**
  calling `burn`, not after — see the comment in `harvest_burn.rs`. Getting
  this ordering backwards is the single easiest way to silently overpay
  every subsequent burner from a shrunk-then-reread supply.
- Every arithmetic step in the fee-split and payout paths uses `u128`
  intermediates and `checked_*` operations exclusively — no raw `+`/`*`/`-`
  on the amounts that move real value, anywhere in the program.
- `Herd.total_zec_deposited` — the number `harvest_burn`'s pro-rata split
  divides against — only ever increases via `deposit_fee_split` and only
  ever decreases via `harvest_burn`'s payout or `admin_recover_paused`
  (which zeroes it, since recovery drains the vault it accounts for).
- `admin_recover_paused` (a) only runs on a herd whose `paused` flag is
  already `true`, and (b) can only ever send to the single fixed
  `GlobalConfig.recovery_address`'s wZEC ATA — never a caller-supplied
  destination. This is the only path in the program that moves a herd's
  vault funds without a corresponding burn.
- `execute_stampede_swap_and_burn` only ever CPIs into
  `GlobalConfig.approved_swap_program` (admin-rotatable via
  `admin_set_swap_program`, never caller-chosen per call), and verifies the
  swap's result by diffing the scratch account's *actual* token balance
  before/after — it does not trust the swap instruction's own data or logs
  about what it produced.
- `initialize_herd` can only be called by `coin_mint`'s actual on-chain
  mint authority (checked, not assumed) — this is what makes "only the
  page that planted a coin can declare its herd" true rather than a naming
  convention.

## Directory guide

```
programs/zebra/          Anchor program — the only trustless piece.
  src/
    lib.rs                #[program] entrypoints, thin wrappers over instructions/*
    state.rs               GlobalConfig / Herd / FeeSplitConfig account layouts
    events.rs              #[event] structs the indexer decodes
    errors.rs               ZebraError
    constants.rs             PDA seeds, the 200-bps denominator, the Memo program ID
    instructions/
      initialize_config.rs   admin bootstrap + shared stampede vault
      initialize_herd.rs      plant a herd for one coin_mint (mint-authority gated)
      deposit_fee_split.rs    splits an already-collected wZEC fee into 4 buckets
      execute_stampede_swap_and_burn.rs   crank: swap parked wZEC -> $ZEBRA -> burn
      harvest_burn.rs         burn a coin, get paid pro-rata from its herd, atomically
      sweep_deployer_payout.rs  crank: pay a Me/Wallet herd's deployer bucket
      keeper_settle_holders.rs  keeper-only batch payout for a Holders herd
      admin.rs                fee-split tuning, pause switches, fixed-address recovery

services/keeper/          Offchain, TRUSTED: watches burn memos, sends real ZEC.
services/indexer/         Offchain: decodes program events, serves /explore + /coin/:mint.

docs/ARCHITECTURE.md      This file.
```

## Before any real deployment

- `declare_id!` in `lib.rs` and the program IDs in `Anchor.toml` are a
  placeholder keypair-shaped string (`Fg6PaFpoGXkYsidMpWTK6W2BeZ7FEfcYkg
  476zPFsLnS`), **not** a real deployment. Generate a real program keypair
  and update both together before deploying anywhere real funds could
  reach it.
- `GlobalConfig.admin` should be a multisig (e.g. a Squads vault) from the
  very first `initialize_config` call — nothing in this program supports
  rotating a single admin key into a multisig later with any extra
  safety, so start there rather than migrating.
- Decide and document the wZEC bridge model (see above) before any real
  money crosses it — this is a product/legal decision, not a config
  value, and users should be told plainly which one is in effect.
- Read `services/keeper/README.md`'s "known gaps" before pointing a real
  Zcash wallet at this skeleton.
