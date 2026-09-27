/**
 * Event schemas mirroring programs/zebra/src/events.rs field-for-field, in
 * declaration order (Borsh serializes struct fields in the order they're
 * declared — order here MUST match the Rust source exactly, or every field
 * after the first mismatch decodes garbage).
 */

import { createHash } from "node:crypto";
import { BorshReader } from "./borsh.js";

const PAYEE_MODE_VARIANTS = ["Me", "Wallet", "Holders"] as const;

export type DecodedEvent = { name: string; data: Record<string, unknown> };

interface EventSchema {
  /** Exact Rust struct name, as written after `#[event] pub struct`. This
   * is the discriminator preimage ("event:" + this name) AND, by Anchor
   * convention, also the IDL event name — the two must be identical. */
  name: string;
  decode(r: BorshReader): Record<string, unknown>;
}

const SCHEMAS: EventSchema[] = [
  {
    name: "ConfigInitialized",
    decode: (r) => ({
      admin: r.readPubkey(),
      keeper: r.readPubkey(),
      zebraMint: r.readPubkey(),
      wzecMint: r.readPubkey(),
    }),
  },
  {
    name: "ZebraHerdSet",
    decode: (r) => ({ herd: r.readPubkey() }),
  },
  {
    name: "HerdInitialized",
    decode: (r) => ({
      coinMint: r.readPubkey(),
      herd: r.readPubkey(),
      payeeMode: r.readEnumTag(PAYEE_MODE_VARIANTS),
      cap: r.readU64().toString(),
    }),
  },
  {
    name: "FeeSplitDeposited",
    decode: (r) => ({
      coinMint: r.readPubkey(),
      herd: r.readPubkey(),
      zecAmount: r.readU64().toString(),
      herdShare: r.readU64().toString(),
      stampedeShare: r.readU64().toString(),
      treasuryShare: r.readU64().toString(),
      deployerShare: r.readU64().toString(),
      herdCapOverflowed: r.readBool(),
    }),
  },
  {
    name: "StampedeBurned",
    decode: (r) => ({
      zebraMint: r.readPubkey(),
      zecSwapped: r.readU64().toString(),
      zebraBurned: r.readU64().toString(),
    }),
  },
  {
    name: "HarvestBurned",
    decode: (r) => ({
      coinMint: r.readPubkey(),
      herd: r.readPubkey(),
      burner: r.readPubkey(),
      burnAmount: r.readU64().toString(),
      payoutZec: r.readU64().toString(),
      supplyBeforeBurn: r.readU64().toString(),
      zcashAddress: r.readOption(() => r.readString()),
    }),
  },
  {
    name: "FeeSplitUpdated",
    decode: (r) => ({
      herdBps: r.readU16(),
      stampedeBps: r.readU16(),
      treasuryBps: r.readU16(),
      deployerBps: r.readU16(),
    }),
  },
  {
    name: "GlobalPausedSet",
    decode: (r) => ({ paused: r.readBool() }),
  },
  {
    name: "HerdPausedSet",
    decode: (r) => ({ coinMint: r.readPubkey(), paused: r.readBool() }),
  },
  {
    name: "HerdRecovered",
    decode: (r) => ({
      coinMint: r.readPubkey(),
      amount: r.readU64().toString(),
      destination: r.readPubkey(),
    }),
  },
  {
    name: "DeployerPayoutSwept",
    decode: (r) => ({
      coinMint: r.readPubkey(),
      destination: r.readPubkey(),
      amount: r.readU64().toString(),
    }),
  },
  {
    name: "HoldersSettled",
    decode: (r) => ({
      coinMint: r.readPubkey(),
      totalPaid: r.readU64().toString(),
      holderCount: r.readU32(),
    }),
  },
];

function discriminator(name: string): Buffer {
  return createHash("sha256").update(`event:${name}`).digest().subarray(0, 8);
}

const BY_DISCRIMINATOR = new Map<string, EventSchema>(
  SCHEMAS.map((schema) => [discriminator(schema.name).toString("hex"), schema]),
);

/** Decodes one `Program data: <base64>` payload. Returns null if its
 * 8-byte discriminator doesn't match any known Zebra event (e.g. it came
 * from a different program's CPI, or a Zebra program version this indexer
 * doesn't know about yet). */
export function decodeEventData(base64Data: string): DecodedEvent | null {
  const buf = Buffer.from(base64Data, "base64");
  if (buf.length < 8) return null;
  const discHex = buf.subarray(0, 8).toString("hex");
  const schema = BY_DISCRIMINATOR.get(discHex);
  if (!schema) return null;
  const reader = new BorshReader(buf.subarray(8));
  return { name: schema.name, data: schema.decode(reader) };
}
