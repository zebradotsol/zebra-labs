/**
 * Zebra indexer: subscribes to the Zebra program's Solana logs, decodes
 * every `#[event]` it emits (see events.ts), and serves the result over a
 * tiny HTTP API (/explore, /coin/:mint) — replacing the frontend's
 * MOCK_COINS with real on-chain activity.
 */

import { Connection, PublicKey, type Logs, type Context } from "@solana/web3.js";
import { config } from "./config.js";
import { decodeEventData } from "./events.js";
import { loadFromDisk, recordEvent } from "./db.js";
import { startServer } from "./server.js";

const PROGRAM_DATA_PREFIX = "Program data: ";

async function main(): Promise<void> {
  loadFromDisk();
  startServer();

  const programId = new PublicKey(config.zebraProgramId);
  const connection = new Connection(config.solanaRpcUrl, {
    commitment: "confirmed",
    wsEndpoint: config.solanaWsUrl,
  });

  console.log(`[indexer] watching program ${programId.toBase58()} on ${config.solanaRpcUrl}`);

  connection.onLogs(
    programId,
    (logs: Logs, context: Context) => {
      if (logs.err) return;

      for (const line of logs.logs) {
        if (!line.startsWith(PROGRAM_DATA_PREFIX)) continue;
        const base64Data = line.slice(PROGRAM_DATA_PREFIX.length).trim();
        const decoded = decodeEventData(base64Data);
        if (!decoded) continue;

        recordEvent({
          signature: logs.signature,
          slot: context.slot,
          // A precise timestamp needs a separate getBlockTime(slot) RPC
          // call per event; left as `null` here to keep this skeleton from
          // hammering the RPC endpoint. Wire it up if /explore needs real
          // timestamps rather than slot-ordering.
          blockTime: null,
          event: decoded,
        });

        console.log(
          `[indexer] sig=${logs.signature} slot=${context.slot} event=${decoded.name}`,
          decoded.data,
        );
      }
    },
    "confirmed",
  );

  await new Promise(() => {});
}

main().catch((err) => {
  console.error("[indexer] fatal:", err);
  process.exit(1);
});
