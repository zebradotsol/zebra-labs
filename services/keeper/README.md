# Zebra keeper

Watches the Zebra Solana program's logs for `harvest_burn` calls that
included a Zcash address, and sends the matching ZEC payout on the Zcash
chain.

This is the offchain half of the "burn-stamp" — see
[`../../docs/ARCHITECTURE.md`](../../docs/ARCHITECTURE.md) before running
this against real funds. Short version: **this process is a single point of
trust.** Whoever holds its configured Zcash wallet key controls every ZEC in
it; the Solana program has no way to verify a payout actually happened, no
slashing, no fallback if this process is offline. Treat the key like an
exchange hot wallet.

## Running

```bash
cp .env.example .env    # fill in SOLANA_RPC_URL / ZEBRA_PROGRAM_ID at minimum
npm install
npm run dev              # or: npm run build && npm start
```

With no `ZCASH_CLI_*` credentials set, the keeper runs in **detect-only
mode**: it logs every burn-stamp it sees, the ZEC amount it would send, and
why (address shape, transparent vs. shielded), but never calls `zcash-cli`.
This is the safe default and is enough to verify the pipeline end-to-end
before wiring up real funds.

To enable real sends, point `ZCASH_CLI_PATH` at a `zcash-cli` binary
talking to a synced `zcashd`/`zebrad` node whose wallet you control, fill in
the RPC credentials, and set `ZCASH_PAYOUT_SOURCE_ADDRESS` (env var, not in
`.env.example` on purpose — see below) to the shielded address funds should
be spent from.

## What it does, precisely

1. Subscribes to `onLogs` for the Zebra program ID.
2. For each confirmed transaction's logs, looks for a line matching
   `Program log: Memo (len N): "ZEBRA_BURN|..."` — this is the Memo CPI
   `harvest_burn` makes when a burner supplies a Zcash address.
3. Parses `mint`, `burn_amount`, `payout_zec`, `zcash_addr`, `ts` out of the
   memo text.
4. Shape-validates `zcash_addr` (`src/zcashAddress.ts` — charset/prefix
   only, not a real checksum check; the node's own `validateaddress` /
   `z_validateaddress` RPC is the real gate before any send).
5. Converts `payout_zec` (wZEC base units) to a ZEC amount via
   `WZEC_TO_ZEC_RATE` / `WZEC_DECIMALS`.
6. Sends: `sendtoaddress` for a transparent destination (no memo possible),
   `z_sendmany` with a memo for a shielded/unified one.
7. Records the transaction signature in `KEEPER_STATE_FILE` so a restart or
   a redelivered log never double-pays the same burn.

## Known gaps in this skeleton (see docs/ARCHITECTURE.md for the ones that
are architectural, not just unfinished code)

- No retry queue: a transient RPC or `zcash-cli` failure is logged and the
  signature is deliberately **not** marked processed, but nothing re-drives
  the retry — the next log delivery for an unrelated signature won't
  retrigger it. A production keeper needs a durable queue (e.g. backed by
  the same DB as `services/indexer`), not the flat-file dedupe set here.
- `z_sendmany` is asynchronous (returns an operation id); this skeleton
  does not poll `z_getoperationstatus` to confirm the send actually landed.
- Single RPC endpoint, no failover, no health checks, no metrics.
- No handling of Solana RPC log-subscription gaps (a dropped websocket can
  silently miss transactions between reconnects) — a production deployment
  should periodically reconcile against `getSignaturesForAddress` /
  `getTransaction` rather than relying on `onLogs` alone.
