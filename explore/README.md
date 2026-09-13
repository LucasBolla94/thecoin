# explore.the-coin.cloud — block explorer

Static explorer (HTML + CSS + JavaScript, no build) for every The Coin network.
A selector switches between networks; each network talks directly to the public
HTTPS API of its seed nodes.

| Network | API | Status |
|---|---|---|
| DevNet (node `--network testnet`, addresses `tct1…`) | `https://testnet-seed1.the-coin.cloud/api/v1` | online |
| Mainnet (addresses `tc1…`) | `https://seed1.the-coin.cloud/api/v1`, `https://seed2.the-coin.cloud/api/v1` | ready, not launched |

Links: `https://explore.the-coin.cloud/?network=devnet` or `?network=mainnet`
(the last choice is remembered in the browser).

## Files

```
explore/
├── index.html
├── assets/
│   ├── networks.js     # networks, seed API URLs, launched flag  ← edit here
│   ├── explore-ui.js   # network selector
│   ├── explorer.js     # explorer (copy of website/assets/explorer.js adapted for several networks)
│   ├── api.js site.js style.css favicon.svg inter-latin.woff2
└── nginx/explore.the-coin.cloud.conf
```

## Deploy (website machine)

```bash
cd thecoin && git pull
sudo scripts/deploy-explorer.sh --setup   # first time: files + nginx site + HTTPS certificate
sudo scripts/deploy-explorer.sh           # later updates
```

Requirements: DNS `explore.the-coin.cloud` → website machine, nginx and certbot
installed, ports 80/443 open.

## Seed nodes

Each seed node must allow the explorer origin and expose its API over HTTPS:

```toml
[rpc]
cors_origins = ["https://explore.the-coin.cloud", "https://the-coin.cloud"]
```

## When the mainnet launches

1. Put the mainnet seed nodes online with their HTTPS API (`https://seed1.the-coin.cloud/api/v1`).
2. In `assets/networks.js` set `launched: true` for `mainnet` (optionally make it `DEFAULT_NETWORK`).
3. Bump `?v=` in `index.html`, commit, then `git pull && sudo scripts/deploy-explorer.sh` on the website machine.

## Local test

```bash
cd explore && python3 -m http.server 8000
# http://localhost:8000/?network=devnet&api=http://127.0.0.1:17334/api/v1   (?api=reset to clear)
```
