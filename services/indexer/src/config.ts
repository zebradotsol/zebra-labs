import "dotenv/config";

export const config = {
  solanaRpcUrl: process.env.SOLANA_RPC_URL ?? "https://api.devnet.solana.com",
  solanaWsUrl: process.env.SOLANA_WS_URL ?? "wss://api.devnet.solana.com",
  zebraProgramId: process.env.ZEBRA_PROGRAM_ID ?? "Fg6PaFpoGXkYsidMpWTK6W2BeZ7FEfcYkg476zPFsLnS",
  eventLogFile: process.env.INDEXER_EVENT_LOG ?? "./events.jsonl",
  httpPort: Number(process.env.INDEXER_HTTP_PORT ?? "8787"),
  maxRecentEventsPerCoin: Number(process.env.INDEXER_MAX_RECENT_EVENTS ?? "50"),
} as const;
