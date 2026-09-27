import "dotenv/config";

function required(name: string): string {
  const v = process.env[name];
  if (!v) {
    throw new Error(`Missing required env var ${name} (see .env.example)`);
  }
  return v;
}

export const config = {
  solanaRpcUrl: process.env.SOLANA_RPC_URL ?? "https://api.devnet.solana.com",
  solanaWsUrl: process.env.SOLANA_WS_URL ?? "wss://api.devnet.solana.com",
  zebraProgramId: process.env.ZEBRA_PROGRAM_ID ?? "Fg6PaFpoGXkYsidMpWTK6W2BeZ7FEfcYkg476zPFsLnS",
  stateFile: process.env.KEEPER_STATE_FILE ?? "./keeper-state.json",

  zcashCliPath: process.env.ZCASH_CLI_PATH || null,
  zcashCliDatadir: process.env.ZCASH_CLI_DATADIR || null,
  zcashCliRpcUser: process.env.ZCASH_CLI_RPCUSER || null,
  zcashCliRpcPassword: process.env.ZCASH_CLI_RPCPASSWORD || null,
  zcashCliRpcPort: process.env.ZCASH_CLI_RPCPORT ?? "8232",

  wzecToZecRate: Number(process.env.WZEC_TO_ZEC_RATE ?? "1"),
  wzecDecimals: Number(process.env.WZEC_DECIMALS ?? "8"),
} as const;

/** True once every Zcash-side credential needed to actually broadcast a
 * payment is present. When false, the keeper runs in detect-only mode: it
 * logs what it would send and why, but never calls zcash-cli. See
 * zcashClient.ts and docs/ARCHITECTURE.md. */
export const canSendRealZec = Boolean(
  config.zcashCliPath && config.zcashCliRpcUser && config.zcashCliRpcPassword,
);

export { required };
