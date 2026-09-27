/**
 * Lightweight, dependency-free Zcash address shape validation.
 *
 * This is a SHAPE check (charset, prefix, plausible length), not a
 * checksum/consensus validator — doing that properly needs Zcash's own
 * Base58Check + Bech32m logic (and, for unified addresses, parsing a
 * length-prefixed receiver list), which belongs in a real
 * `librustzcash`/`zcash-cli`-backed check before this keeper ever signs a
 * real transaction. Treat a `true` here as "worth attempting a send", never
 * as "guaranteed valid" — `zcashClient.ts` must still let the wallet's own
 * `validateaddress` RPC (or equivalent) be the actual gate before funds
 * move.
 *
 * Ported from the shape of `isLikelyZcashAddress` referenced in the spec
 * (zecpad's src/lib/burnClaim.ts) — kept here as its own small module so
 * the keeper has no compile-time dependency on that project.
 */

const TRANSPARENT_PREFIXES = ["t1", "t3"] as const; // P2PKH / P2SH, mainnet
const TRANSPARENT_TESTNET_PREFIXES = ["tm", "t2"] as const;
const SAPLING_PREFIXES = ["zs1", "ztestsapling1"] as const; // shielded, bech32
const UNIFIED_PREFIXES = ["u1", "utest1"] as const; // unified addresses (Orchard/Sapling/transparent receivers)

const BASE58_ALPHABET = /^[1-9A-HJ-NP-Za-km-z]+$/;
const BECH32_CHARSET = /^[qpzry9x8gf2tvdw0s3jn54khce6mua7l]+$/;

export type ZcashAddressKind = "transparent" | "shielded" | "unified" | "unknown";

export interface ZcashAddressCheck {
  isLikely: boolean;
  kind: ZcashAddressKind;
  /** True for the small set of prefixes that indicate a transparent
   * (t1/t3) address — the keeper cares about this because transparent
   * recipients cannot receive a memo at the Zcash-protocol level (see
   * docs/ARCHITECTURE.md). */
  isTransparent: boolean;
}

export function checkZcashAddress(address: string): ZcashAddressCheck {
  const addr = address.trim();

  if (addr.length < 8 || addr.length > 512) {
    return { isLikely: false, kind: "unknown", isTransparent: false };
  }

  for (const prefix of [...TRANSPARENT_PREFIXES, ...TRANSPARENT_TESTNET_PREFIXES]) {
    if (addr.startsWith(prefix)) {
      const body = addr;
      const plausibleLength = body.length >= 34 && body.length <= 36;
      return {
        isLikely: plausibleLength && BASE58_ALPHABET.test(body),
        kind: "transparent",
        isTransparent: true,
      };
    }
  }

  for (const prefix of UNIFIED_PREFIXES) {
    if (addr.startsWith(prefix)) {
      const body = addr.slice(prefix.length);
      return {
        isLikely: body.length >= 20 && BECH32_CHARSET.test(body.toLowerCase()),
        kind: "unified",
        isTransparent: false,
      };
    }
  }

  for (const prefix of SAPLING_PREFIXES) {
    if (addr.startsWith(prefix)) {
      const body = addr.slice(prefix.length);
      return {
        isLikely: body.length >= 60 && body.length <= 80 && BECH32_CHARSET.test(body.toLowerCase()),
        kind: "shielded",
        isTransparent: false,
      };
    }
  }

  return { isLikely: false, kind: "unknown", isTransparent: false };
}

export function isLikelyZcashAddress(address: string): boolean {
  return checkZcashAddress(address).isLikely;
}
