/*
 * The Coin website — API configuration.
 *
 * List of API bases (each ending in /api/v1), tried in order with automatic
 * failover. In production nginx proxies same-origin /api/ to the seed nodes,
 * so the default is enough.
 *
 * For local testing you can override without editing this file:
 *   http://localhost:8000/explorer.html?api=http://127.0.0.1:7334/api/v1
 * (the value is remembered in localStorage; use ?api=reset to clear it).
 */
window.THECOIN_API = [
  "/api/v1",
  // "https://seed1.the-coin.cloud/api/v1",
  // "https://seed2.the-coin.cloud/api/v1",
];

/* Network shown by default in texts and address hints. */
window.THECOIN_NETWORK = "mainnet";

/* Repository with the source code and documentation. */
window.THECOIN_REPO = "https://github.com/LucasBolla94/thecoin";
