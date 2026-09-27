/**
 * Thin wrapper around `zcash-cli` (or a `zebrad`/`zcashd` node exposing the
 * same RPC surface). Deliberately shells out to the CLI rather than
 * speaking the JSON-RPC wire protocol directly — this is a single
 * low-throughput keeper, not a high-frequency trading bot, and shelling
 * out is the easiest thing to audit and to swap for `librustzcash` later.
 *
 * TRUST NOTE (see docs/ARCHITECTURE.md "Keeper trust model" for the full
 * writeup): whichever machine runs this process, with these credentials
 * configured, can move every ZEC in the configured wallet. There is no
 * on-chain check-and-balance for this anywhere in the Zebra program — the
 * Solana program only ever emits a memo saying a payout *should* happen;
 * it has no way to verify one actually did, and no way to claw back a
 * double-spend or a keeper that goes offline. Treat this key with the
 * same care as an exchange hot wallet, and prefer a multisig zcashd wallet
 * (Zcash supports P2SH multisig on transparent addresses) over a single
 * key the moment this handles non-trivial value.
 */

import { execFile } from "node:child_process";
import { promisify } from "node:util";
import { canSendRealZec, config } from "./config.js";

const execFileAsync = promisify(execFile);

export interface SendResult {
  txid: string;
}

function buildArgs(rpcMethod: string, params: unknown[]): string[] {
  const args: string[] = [];
  if (config.zcashCliDatadir) args.push(`-datadir=${config.zcashCliDatadir}`);
  args.push(`-rpcuser=${config.zcashCliRpcUser}`);
  args.push(`-rpcpassword=${config.zcashCliRpcPassword}`);
  args.push(`-rpcport=${config.zcashCliRpcPort}`);
  args.push(rpcMethod);
  args.push(...params.map((p) => (typeof p === "string" ? p : JSON.stringify(p))));
  return args;
}

async function callZcashCli<T>(rpcMethod: string, params: unknown[] = []): Promise<T> {
  if (!config.zcashCliPath) {
    throw new Error(
      "ZCASH_CLI_PATH is not configured — refusing to call zcash-cli. " +
        "See .env.example and docs/ARCHITECTURE.md before enabling real sends.",
    );
  }
  const { stdout } = await execFileAsync(config.zcashCliPath, buildArgs(rpcMethod, params));
  const trimmed = stdout.trim();
  try {
    return JSON.parse(trimmed) as T;
  } catch {
    // zcash-cli returns bare strings (like a txid) unquoted for some RPCs.
    return trimmed as unknown as T;
  }
}

/** Converts a wZEC payout amount (base units, per WZEC_DECIMALS) into a
 * ZEC amount (decimal ZEC, per WZEC_TO_ZEC_RATE) suitable for
 * `sendtoaddress` / `z_sendmany`. This is where the custodial-peg
 * assumption becomes a concrete number — see docs/ARCHITECTURE.md. */
export function payoutBaseUnitsToZec(payoutBaseUnits: bigint): number {
  const divisor = 10 ** config.wzecDecimals;
  const wzecAmount = Number(payoutBaseUnits) / divisor;
  return wzecAmount * config.wzecToZecRate;
}

/**
 * Sends `zecAmount` ZEC to `address`.
 *
 * - Transparent destination (t1/t3): uses `sendtoaddress`. No memo is
 *   possible — this is a plain value transfer, full stop.
 * - Shielded/unified destination: uses `z_sendmany` with a single
 *   recipient and, if `memoHex` is supplied, attaches it as the
 *   Zcash-native memo field (this is the ONLY way a memo actually lands on
 *   the Zcash chain itself — see docs/ARCHITECTURE.md "What the stamp
 *   actually proves").
 *
 * Throws (never silently no-ops) if `canSendRealZec` is false, so a
 * misconfigured deployment fails loudly instead of pretending to pay
 * people.
 */
export async function sendZec(params: {
  address: string;
  isTransparent: boolean;
  zecAmount: number;
  memoUtf8?: string;
  /** The wallet address/account to send FROM. For z_sendmany this must be
   * a shielded address the node's wallet controls. */
  fromAddress: string;
}): Promise<SendResult> {
  if (!canSendRealZec) {
    throw new Error(
      "Keeper is running in detect-only mode (Zcash CLI credentials not " +
        "configured) — refusing to send. This is the correct, safe default; " +
        "see .env.example.",
    );
  }

  if (params.isTransparent) {
    // No memo possible on a transparent recipient at the protocol level.
    const txid = await callZcashCli<string>("sendtoaddress", [
      params.address,
      params.zecAmount,
    ]);
    return { txid };
  }

  const memoHex = params.memoUtf8 ? Buffer.from(params.memoUtf8, "utf8").toString("hex") : undefined;
  const opid = await callZcashCli<string>("z_sendmany", [
    params.fromAddress,
    [
      {
        address: params.address,
        amount: params.zecAmount,
        ...(memoHex ? { memo: memoHex } : {}),
      },
    ],
  ]);

  // z_sendmany is async: it returns an operation id, not a txid. A real
  // deployment should poll z_getoperationstatus until it settles — left as
  // a TODO here to keep this skeleton's happy-path readable.
  return { txid: opid };
}

export async function validateAddressOnNode(address: string): Promise<boolean> {
  try {
    const result = await callZcashCli<{ isvalid: boolean }>("z_validateaddress", [address]);
    if (typeof result === "object" && "isvalid" in result) return result.isvalid;
  } catch {
    /* fall through to transparent check below */
  }
  try {
    const result = await callZcashCli<{ isvalid: boolean }>("validateaddress", [address]);
    return typeof result === "object" && "isvalid" in result ? result.isvalid : false;
  } catch {
    return false;
  }
}
