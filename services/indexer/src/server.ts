/** Minimal HTTP API — no framework dependency on purpose, this is two
 * routes. Swap in Express/Fastify if this grows beyond that. */

import { createServer } from "node:http";
import { config } from "./config.js";
import { getCoin, listCoins } from "./db.js";

function sendJson(res: import("node:http").ServerResponse, status: number, body: unknown): void {
  const payload = JSON.stringify(body, null, 2);
  res.writeHead(status, {
    "content-type": "application/json",
    "access-control-allow-origin": "*",
  });
  res.end(payload);
}

export function startServer(): void {
  const server = createServer((req, res) => {
    const url = new URL(req.url ?? "/", "http://localhost");

    if (url.pathname === "/explore") {
      sendJson(res, 200, { coins: listCoins() });
      return;
    }

    const coinMatch = /^\/coin\/([^/]+)$/.exec(url.pathname);
    if (coinMatch) {
      const mint = coinMatch[1];
      const coin = getCoin(mint);
      if (!coin) {
        sendJson(res, 404, { error: `no indexed activity for mint ${mint}` });
        return;
      }
      sendJson(res, 200, coin);
      return;
    }

    if (url.pathname === "/healthz") {
      sendJson(res, 200, { ok: true });
      return;
    }

    sendJson(res, 404, { error: "not found", routes: ["/explore", "/coin/:mint", "/healthz"] });
  });

  server.listen(config.httpPort, () => {
    console.log(`[server] listening on http://localhost:${config.httpPort}`);
  });
}
