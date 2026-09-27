/**
 * Minimal Borsh reader for decoding Anchor `#[event]` log data by hand.
 *
 * Deliberately not pulling in `@coral-xyz/anchor`'s IDL-driven event
 * parser: that needs a hand-authored IDL JSON kept byte-for-byte in sync
 * with programs/zebra/src/events.rs (this repo's Anchor CLI toolchain
 * can't run `anchor build` here to generate one automatically — see
 * docs/ARCHITECTURE.md). A small manual decoder whose field layouts are
 * asserted against events.rs in one place (EVENT_SCHEMAS in events.ts) is
 * easier to audit and keep correct than a hand-maintained IDL would be.
 */

import { PublicKey } from "@solana/web3.js";

export class BorshReader {
  private offset = 0;
  constructor(private readonly buf: Buffer) {}

  get remaining(): number {
    return this.buf.length - this.offset;
  }

  readU8(): number {
    const v = this.buf.readUInt8(this.offset);
    this.offset += 1;
    return v;
  }

  readBool(): boolean {
    return this.readU8() !== 0;
  }

  readU16(): number {
    const v = this.buf.readUInt16LE(this.offset);
    this.offset += 2;
    return v;
  }

  readU32(): number {
    const v = this.buf.readUInt32LE(this.offset);
    this.offset += 4;
    return v;
  }

  readU64(): bigint {
    const v = this.buf.readBigUInt64LE(this.offset);
    this.offset += 8;
    return v;
  }

  readPubkey(): string {
    const bytes = this.buf.subarray(this.offset, this.offset + 32);
    this.offset += 32;
    return new PublicKey(bytes).toBase58();
  }

  readString(): string {
    const len = this.readU32();
    const s = this.buf.toString("utf8", this.offset, this.offset + len);
    this.offset += len;
    return s;
  }

  /** Option<T>: 1-byte tag (0 = None, 1 = Some) then T if present. */
  readOption<T>(readT: () => T): T | null {
    const tag = this.readU8();
    if (tag === 0) return null;
    return readT();
  }

  /** A Rust unit-variant enum (no associated data), Borsh-encoded as a
   * single u8 tag equal to the variant's declaration order. */
  readEnumTag(variantNames: readonly string[]): string {
    const tag = this.readU8();
    const name = variantNames[tag];
    if (name === undefined) {
      throw new Error(`enum tag ${tag} out of range for [${variantNames.join(", ")}]`);
    }
    return name;
  }
}
