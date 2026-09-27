/**
 * Zebra keeper: subscribes to Solana logs for the Zebra program, looks for
 * the ZEBRA_BURN memo `harvest_burn` writes when a burner supplies a Zcash
 * address, and (if configured — see zcashClient.ts) sends the matching ZEC
 * payout on the Zcash chain.
 *
 * This process is intentionally simple/sequential (one log subscription,
 * one send at a time) — see docs/ARCHITECTURE.md for why a keeper is a
 * single point of trust/failure by construction, and what running more
 * than one of these would and wouldn't fix.
 */

import { Connection, PublicKey, type Logs } from "@solana/web3.js";
import { config, canSendRealZec } from "./config.js";
import { checkZcashAddress } from "./zcashAddress.js";
import { sendZec, payoutBaseUnitsToZec, validateAddressOnNode } from "./zcashClient.js";
import { isProcessed, markProcessed } from "./state.js";

const MEMO_LOG_RE = /Program log: Memo \(len \d+\): "((?:[^"\\]|\\.)*)"/;

interface ParsedBurnMemo {
  mint: string;
  burnAmount: bigint;
  payoutZec: bigint;
  zcashAddr: string;
  ts: number;
}

/** Unescapes the `{:?}` Rust Debug-formatted string spl-memo logs (Debug
 * for &str escapes `"` and `\`; this reverses just those two, which is all
 * that matters for the plain-ASCII payload harvest_burn writes). */
function unescapeRustDebugString(s: string): string {
  return s.replace(/\\(["\\])/g, "$1");
}

function parseBurnMemo(memoTextRaw: string): ParsedBurnMemo | null {
  const memoText = unescapeRustDebugString(memoTextRaw);
  if (!memoText.startsWith("ZEBRA_BURN|")) return null;

  const fields = new Map<string, string>();
  for (const part of memoText.split("|").slice(1)) {
    const eq = part.indexOf("=");
    if (eq === -1) continue;
    fields.set(part.slice(0, eq), part.slice(eq + 1));
  }

  const mint = fields.get("mint");
  const burnAmount = fields.get("burn_amount");
  const payoutZec = fields.get("payout_zec");
  const zcashAddr = fields.get("zcash_addr");
  const ts = fields.get("ts");
  if (!mint || !burnAmount || !payoutZec || !zcashAddr || !ts) return null;

  try {
    return {
      mint,
      burnAmount: BigInt(burnAmount),
      payoutZec: BigInt(payoutZec),
      zcashAddr,
      ts: Number(ts),
    };
  } catch {
    return null;
  }
}

async function handleBurnMemo(signature: string, memo: ParsedBurnMemo): Promise<void> {
  const check = checkZcashAddress(memo.zcashAddr);
  if (!check.isLikely) {
    console.warn(
      `[keeper] sig=${signature} mint=${memo.mint}: zcash_addr "${memo.zcashAddr}" ` +
        "fails shape validation, skipping. The Solana program itself does not " +
        "validate Zcash address checksums (it can't), so this can legitimately " +
        "happen for a typo'd address — funds stay in the herd vault, nothing " +
        "is lost, but this burn's payout will not be delivered automatically.",
    );
    return;
  }

  if (check.isTransparent) {
    console.log(
      `[keeper] sig=${signature}: destination ${memo.zcashAddr} is transparent — ` +
        "no memo can be attached on the Zcash side. The only public record of " +
        "*why* this payment happened is the Solana memo itself; see " +
        "docs/ARCHITECTURE.md \"What the stamp actually proves\".",
    );
  }

  const zecAmount = payoutBaseUnitsToZec(memo.payoutZec);
  console.log(
    `[keeper] sig=${signature} mint=${memo.mint} burn_amount=${memo.burnAmount} ` +
      `payout_zec(base units)=${memo.payoutZec} -> ${zecAmount} ZEC to ${memo.zcashAddr}`,
  );

  if (!canSendRealZec) {
    console.log(
      "[keeper] detect-only mode (no Zcash CLI credentials configured) — " +
        "not sending. Configure ZCASH_CLI_* in .env to enable real sends.",
    );
    return;
  }

  const nodeSaysValid = await validateAddressOnNode(memo.zcashAddr).catch(() => false);
  if (!nodeSaysValid) {
    console.error(
      `[keeper] sig=${signature}: node rejected address ${memo.zcashAddr} via ` +
        "its own validateaddress/z_validateaddress RPC — refusing to send.",
    );
    return;
  }

  const fromAddress = process.env.ZCASH_PAYOUT_SOURCE_ADDRESS;
  if (!fromAddress) {
    console.error(
      "[keeper] ZCASH_PAYOUT_SOURCE_ADDRESS is not set — refusing to send " +
        "without an explicit source address configured.",
    );
    return;
  }

  const memoUtf8 = check.isTransparent
    ? undefined
    : `zebra-burn-stamp:${memo.mint}:${signature}`;

  const { txid } = await sendZec({
    address: memo.zcashAddr,
    isTransparent: check.isTransparent,
    zecAmount,
    memoUtf8,
    fromAddress,
  });

  console.log(`[keeper] sig=${signature}: sent, zcash txid/opid=${txid}`);
}

async function main(): Promise<void> {
  const programId = new PublicKey(config.zebraProgramId);
  const connection = new Connection(config.solanaRpcUrl, {
    commitment: "confirmed",
    wsEndpoint: config.solanaWsUrl,
  });

  console.log(`[keeper] watching program ${programId.toBase58()} on ${config.solanaRpcUrl}`);
  console.log(
    canSendRealZec
      ? "[keeper] Zcash sends ENABLED (ZCASH_CLI_* configured)"
      : "[keeper] Zcash sends DISABLED — running in detect-only mode",
  );

  connection.onLogs(
    programId,
    async (logs: Logs) => {
      if (logs.err) return; // failed tx, nothing to do
      const signature = logs.signature;
      if (isProcessed(signature)) return;

      for (const line of logs.logs) {
        const match = MEMO_LOG_RE.exec(line);
        if (!match) continue;
        const memo = parseBurnMemo(match[1]);
        if (!memo) continue;

        try {
          await handleBurnMemo(signature, memo);
        } catch (err) {
          console.error(`[keeper] sig=${signature}: error handling burn memo:`, err);
          // Deliberately do NOT markProcessed on failure — a transient RPC
          // or zcash-cli error should be retried on the next log delivery
          // for this signature, not silently dropped. A production keeper
          // should instead push this into a durable retry queue; logs are
          // not guaranteed to be redelivered by the RPC provider.
          return;
        }
        markProcessed(signature);
      }
    },
    "confirmed",
  );

  // Keep the process alive; onLogs is subscription-based.
  await new Promise(() => {});
}

main().catch((err) => {
  console.error("[keeper] fatal:", err);
  process.exit(1);
});
