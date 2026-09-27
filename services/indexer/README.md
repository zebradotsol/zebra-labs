# Zebra indexer

Subscribes to the Zebra Solana program's logs, decodes every `#[event]` it
emits, persists them to an append-only JSON-Lines log, and serves the
result over a small HTTP API:

- `GET /explore` — every coin the indexer has seen activity for, most
  recently active first.
- `GET /coin/:mint` — one coin's aggregated state (herd, payee mode, cap,
  running deposit/burn totals) plus its recent event feed.
- `GET /healthz` — liveness check.

This is what a frontend's `/explore` and `/coin/[mint]` pages should call
instead of `MOCK_COINS` (see spec).

## Running

```bash
cp .env.example .env    # fill in SOLANA_RPC_URL / ZEBRA_PROGRAM_ID
npm install
npm run dev              # or: npm run build && npm start
```

## How event decoding works

Anchor's `emit!` macro logs events as `Program data: <base64>` lines via
the `sol_log_data` syscall — a raw Borsh encoding of an 8-byte
discriminator (`sha256("event:<EventName>")[..8]`) followed by the
struct's fields in declaration order. `src/events.ts` hand-decodes these
against schemas that mirror `programs/zebra/src/events.rs` field-for-field
— see that file's header comment for why this indexer doesn't use
`@coral-xyz/anchor`'s IDL-driven event parser instead.

**If you add or change a field on any `#[event]` struct in the Rust
program, update its schema in `src/events.ts` in the same change.** Nothing
enforces that the two stay in sync automatically — the whole reason
`docs/ARCHITECTURE.md` calls this out is that a drift here fails silently
(fields decode as garbage, not as an error) rather than loudly.

## Known gaps in this skeleton

- Storage is a flat JSON-Lines file rebuilt into memory on startup (see
  `src/db.ts`'s header comment) — fine for development, not for
  production scale or multi-process deployment. Swap in Postgres/SQLite by
  replacing that one file.
- `blockTime` is always `null` — getting a real timestamp needs an extra
  `getBlockTime(slot)` RPC call per event, deliberately left out to avoid
  hammering the RPC endpoint in this skeleton.
- No reconciliation pass: if a log delivery is missed (RPC hiccup,
  websocket reconnect gap), that event is simply never indexed. A
  production indexer should periodically diff against
  `getSignaturesForAddress(programId)` and backfill.
- No pagination on `/explore` or on a coin's `recentEvents`
  (`INDEXER_MAX_RECENT_EVENTS` caps memory use per coin, not the API
  response).
