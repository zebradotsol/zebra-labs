/** Tiny file-backed dedupe set, so a keeper restart (or an RPC replaying a
 * log it already delivered) never pays the same burn-stamp twice. A real
 * deployment should use a real database (see services/indexer, which
 * already needs one) shared with this process instead of a flat file —
 * this is a skeleton-appropriate stand-in, not a concurrency-safe store. */

import { readFileSync, writeFileSync, existsSync } from "node:fs";
import { config } from "./config.js";

let processed: Set<string> = new Set();
let loaded = false;

function load(): void {
  if (loaded) return;
  loaded = true;
  if (existsSync(config.stateFile)) {
    try {
      const raw = JSON.parse(readFileSync(config.stateFile, "utf8"));
      if (Array.isArray(raw)) processed = new Set(raw);
    } catch (err) {
      console.error(`[state] failed to read ${config.stateFile}, starting empty:`, err);
    }
  }
}

function persist(): void {
  writeFileSync(config.stateFile, JSON.stringify([...processed]), "utf8");
}

export function isProcessed(signature: string): boolean {
  load();
  return processed.has(signature);
}

export function markProcessed(signature: string): void {
  load();
  processed.add(signature);
  persist();
}
