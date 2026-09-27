/**
 * Storage layer for the indexer.
 *
 * A flat, append-only JSON-Lines event log (`events.jsonl` by default) is
 * the source of truth; an in-memory `Map` rebuilt from it at startup (and
 * kept live as new events arrive) serves /explore and /coin/:mint without
 * re-scanning the file on every request.
 *
 * This is a skeleton-appropriate stand-in for "a real database" (see
 * spec: "пишет в БД, отдаёт /explore и /coin/[mint] real данные"). It is
 * NOT safe for multiple indexer processes writing concurrently, has no
 * indexing beyond the one in-memory Map, and reloads the whole log into
 * memory on startup — fine for a single coin-launchpad's worth of activity
 * during development, not for production scale. Swapping in Postgres/
 * SQLite means replacing this file; nothing else in the indexer should
 * need to change (server.ts and index.ts only call the functions exported
 * here).
 */

import { appendFileSync, existsSync, readFileSync } from "node:fs";
import { config } from "./config.js";
import type { DecodedEvent } from "./events.js";

export interface StoredEvent extends DecodedEvent {
  signature: string;
  slot: number;
  blockTime: number | null;
  seq: number;
}

export interface CoinState {
  coinMint: string;
  herd: string | null;
  payeeMode: string | null;
  cap: string | null;
  /** Mirrors Herd.total_zec_deposited on-chain (best-effort, from
   * FeeSplitDeposited/HarvestBurned/HerdRecovered events only — a
   * reorg or a missed log leaves this out of sync with the real account;
   * a production indexer should periodically reconcile against
   * getAccountInfo). */
  totalZecDepositedEstimate: string;
  lifetimeZecDeposited: string;
  burnCount: number;
  lifetimePayoutZec: string;
  paused: boolean;
  firstSeenSlot: number;
  lastActivitySlot: number;
  lastActivityBlockTime: number | null;
  recentEvents: StoredEvent[];
}

const coins = new Map<string, CoinState>();
let nextSeq = 0;

function bigIntAdd(a: string, b: string): string {
  return (BigInt(a) + BigInt(b)).toString();
}

function ensureCoin(mint: string, slot: number): CoinState {
  let coin = coins.get(mint);
  if (!coin) {
    coin = {
      coinMint: mint,
      herd: null,
      payeeMode: null,
      cap: null,
      totalZecDepositedEstimate: "0",
      lifetimeZecDeposited: "0",
      burnCount: 0,
      lifetimePayoutZec: "0",
      paused: false,
      firstSeenSlot: slot,
      lastActivitySlot: slot,
      lastActivityBlockTime: null,
      recentEvents: [],
    };
    coins.set(mint, coin);
  }
  return coin;
}

function pushRecent(coin: CoinState, ev: StoredEvent): void {
  coin.recentEvents.unshift(ev);
  if (coin.recentEvents.length > config.maxRecentEventsPerCoin) {
    coin.recentEvents.length = config.maxRecentEventsPerCoin;
  }
}

/** Applies one decoded event to in-memory state. Does NOT persist —
 * callers append the raw event to the log themselves (see
 * `recordEvent`), and `applyEvent` is also what rebuilds state from the
 * log on startup. */
function applyEvent(stored: StoredEvent): void {
  const { name, data } = stored;
  const d = data as Record<string, any>;

  switch (name) {
    case "HerdInitialized": {
      const coin = ensureCoin(d.coinMint, stored.slot);
      coin.herd = d.herd;
      coin.payeeMode = d.payeeMode;
      coin.cap = d.cap;
      break;
    }
    case "FeeSplitDeposited": {
      const coin = ensureCoin(d.coinMint, stored.slot);
      coin.totalZecDepositedEstimate = bigIntAdd(coin.totalZecDepositedEstimate, d.herdShare);
      coin.lifetimeZecDeposited = bigIntAdd(coin.lifetimeZecDeposited, d.herdShare);
      break;
    }
    case "HarvestBurned": {
      const coin = ensureCoin(d.coinMint, stored.slot);
      coin.totalZecDepositedEstimate = (
        BigInt(coin.totalZecDepositedEstimate) - BigInt(d.payoutZec)
      ).toString();
      coin.burnCount += 1;
      coin.lifetimePayoutZec = bigIntAdd(coin.lifetimePayoutZec, d.payoutZec);
      break;
    }
    case "HerdPausedSet": {
      const coin = ensureCoin(d.coinMint, stored.slot);
      coin.paused = d.paused;
      break;
    }
    case "HerdRecovered": {
      const coin = ensureCoin(d.coinMint, stored.slot);
      coin.totalZecDepositedEstimate = "0";
      break;
    }
    default:
      // Program-wide events (ConfigInitialized, FeeSplitUpdated,
      // GlobalPausedSet, ZebraHerdSet) and the stampede leg
      // (StampedeBurned, which is $ZEBRA-specific, not per-coin) don't map
      // onto a single coin's state; they're still recorded in the raw log
      // for /explore's activity feed, just not folded into a CoinState.
      return;
  }

  const coin = coins.get(d.coinMint);
  if (coin) {
    coin.lastActivitySlot = stored.slot;
    coin.lastActivityBlockTime = stored.blockTime;
    pushRecent(coin, stored);
  }
}

export function recordEvent(input: {
  signature: string;
  slot: number;
  blockTime: number | null;
  event: DecodedEvent;
}): void {
  const stored: StoredEvent = { ...input.event, signature: input.signature, slot: input.slot, blockTime: input.blockTime, seq: nextSeq++ };
  appendFileSync(config.eventLogFile, JSON.stringify(stored) + "\n", "utf8");
  applyEvent(stored);
}

export function loadFromDisk(): void {
  if (!existsSync(config.eventLogFile)) return;
  const raw = readFileSync(config.eventLogFile, "utf8");
  for (const line of raw.split("\n")) {
    if (!line.trim()) continue;
    try {
      const stored = JSON.parse(line) as StoredEvent;
      nextSeq = Math.max(nextSeq, stored.seq + 1);
      applyEvent(stored);
    } catch (err) {
      console.error("[db] skipping malformed line in event log:", err);
    }
  }
  console.log(`[db] rebuilt state for ${coins.size} coin(s) from ${config.eventLogFile}`);
}

export function listCoins(): CoinState[] {
  return [...coins.values()].sort((a, b) => b.lastActivitySlot - a.lastActivitySlot);
}

export function getCoin(mint: string): CoinState | undefined {
  return coins.get(mint);
}
