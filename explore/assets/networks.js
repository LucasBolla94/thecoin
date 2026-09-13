/*
 * explore.the-coin.cloud — networks shown by the explorer.
 *
 * Each network lists the public API bases of its seed nodes (tried in order,
 * with automatic failover). The seed nodes must allow this origin in
 * `[rpc] cors_origins` ("https://explore.the-coin.cloud").
 *
 * When the mainnet launches: set `launched: true` below and redeploy. Nothing
 * else changes.
 *
 * Loaded before api.js: it picks the network (?network=devnet|mainnet, then
 * the last choice saved in this browser, then DEFAULT_NETWORK) and sets
 * window.THECOIN_API for it.
 */
(function () {
  "use strict";

  var NETWORKS = {
    devnet: {
      label: "DevNet",
      // Node software name of this network (reported by /api/v1/status).
      node: "testnet",
      hrp: "tct",
      launched: true,
      description: "Public test network. Coins have no value and the chain may be reset.",
      apis: [
        "https://testnet-seed1.the-coin.cloud/api/v1",
        // "https://testnet-seed2.the-coin.cloud/api/v1",
      ],
    },
    mainnet: {
      label: "Mainnet",
      node: "mainnet",
      hrp: "tc",
      launched: false,
      description: "The main The Coin network.",
      apis: ["https://seed1.the-coin.cloud/api/v1", "https://seed2.the-coin.cloud/api/v1"],
    },
  };
  var DEFAULT_NETWORK = "devnet";
  var LS_KEY = "thecoin_explore_network";

  var chosen = null;
  try {
    var q = new URLSearchParams(window.location.search).get("network");
    if (q && NETWORKS[q]) {
      chosen = q;
      localStorage.setItem(LS_KEY, q);
    } else {
      var saved = localStorage.getItem(LS_KEY);
      if (saved && NETWORKS[saved]) chosen = saved;
    }
  } catch (err) {
    /* storage unavailable: use the default */
  }
  chosen = chosen || DEFAULT_NETWORK;

  var net = NETWORKS[chosen];
  window.THECOIN_NETWORKS = NETWORKS;
  window.THECOIN_NETWORK_ID = chosen;
  window.THECOIN_NETWORK_INFO = net;
  window.THECOIN_API = net.apis.slice();
  window.THECOIN_REPO = "https://github.com/LucasBolla94/thecoin";

  /* Switching networks reloads the page on the same route. */
  window.THECOIN_SWITCH_NETWORK = function (id) {
    if (!NETWORKS[id] || id === chosen) return;
    try {
      localStorage.setItem(LS_KEY, id);
    } catch (err) {
      /* ignore */
    }
    var url = new URL(window.location.href);
    url.searchParams.set("network", id);
    url.hash = "#/";
    window.location.href = url.toString();
  };
})();
