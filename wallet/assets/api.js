/**
 * Talks to a The Coin node (REST, /api/v1).
 *
 * Amounts on this chain can exceed what JavaScript numbers hold exactly, so
 * every integer field that carries motes is parsed as BigInt straight from the
 * response text — JSON.parse alone would silently round it.
 */

/** Fields that are amounts in motes, anywhere in a response. */
const BIGINT_FIELDS = new Set([
  "amount",
  "balance",
  "spendable",
  "locked",
  "immature",
  "fee",
  "burned",
  "value",
  "deposit",
  "max_supply",
  "emitted",
  "circulating",
  "current_block_reward",
  "value_per_block",
  "base_fee",
  "fee_per_kb",
  "fee_per_kfuel",
  "storage_deposit_per_kb",
  "proposal_deposit",
  "typical_transfer_fee",
  "subsidy",
  "reward",
  "total",
]);

/** Quotes integers too large for a JS number so no precision is lost. */
function quoteBigIntegers(text) {
  return text.replace(/:\s*(-?\d{16,})(?=\s*[,}\]])/g, ': "$1"');
}

function reviveAmounts(value, key) {
  if (Array.isArray(value)) return value.map((v) => reviveAmounts(v));
  if (value && typeof value === "object") {
    const out = {};
    for (const [k, v] of Object.entries(value)) out[k] = reviveAmounts(v, k);
    return out;
  }
  if (
    BIGINT_FIELDS.has(key) &&
    (typeof value === "number" || typeof value === "string")
  ) {
    try {
      return BigInt(value);
    } catch {
      return value;
    }
  }
  return value;
}

export class ApiError extends Error {
  constructor(message, { status, body } = {}) {
    super(message);
    this.name = "ApiError";
    this.status = status;
    this.body = body;
  }
}

/** Turns a node error string into something a person can act on. */
export function friendlyError(message) {
  const m = String(message);
  if (/bad nonce/.test(m))
    return "Your wallet is out of step with the node. Refresh and try again.";
  if (/insufficient funds/.test(m))
    return "Not enough spendable balance for this amount plus the fee.";
  if (/fee too low/.test(m))
    return "The fee is below the network minimum right now. Try a higher priority.";
  if (/already in mempool/.test(m))
    return "This exact transaction is already waiting to be mined.";
  if (/not replaceable/.test(m))
    return "Another payment with this number is already pending.";
  if (/replacement needs a fee/.test(m))
    return "To replace a payment the new fee must be at least 25% higher.";
  if (/too many pending/.test(m))
    return "Too many payments waiting from this address. Wait for one to be mined.";
  if (/mempool full/.test(m))
    return "The network is busy and this fee is too low. Try a higher priority.";
  if (/address index disabled/.test(m))
    return "This node does not keep address history (it is a pruned node).";
  if (/malformed transaction/.test(m))
    return "The node could not read this transaction. Please report this.";
  return m;
}

export class NodeApi {
  constructor(baseUrl) {
    this.setBaseUrl(baseUrl);
  }

  setBaseUrl(baseUrl) {
    this.baseUrl = String(baseUrl || "").replace(/\/+$/, "");
  }

  async request(path, { method = "GET", body, timeoutMs = 20000 } = {}) {
    const controller = new AbortController();
    const timer = setTimeout(() => controller.abort(), timeoutMs);
    let response;
    try {
      response = await fetch(`${this.baseUrl}${path}`, {
        method,
        headers: body ? { "content-type": "application/json" } : undefined,
        body: body ? JSON.stringify(body) : undefined,
        signal: controller.signal,
        // No credentials: the node allows this origin without them.
      });
    } catch (err) {
      clearTimeout(timer);
      throw new ApiError(
        err.name === "AbortError"
          ? "The node did not answer in time."
          : "Could not reach the node. Check your connection or pick another node in Settings.",
      );
    }
    clearTimeout(timer);
    const text = await response.text();
    let data = null;
    if (text) {
      try {
        data = reviveAmounts(JSON.parse(quoteBigIntegers(text)));
      } catch {
        throw new ApiError(
          "The node sent an answer this wallet could not read.",
          { status: response.status },
        );
      }
    }
    if (!response.ok) {
      const message =
        data && data.error ? data.error : `request failed (${response.status})`;
      throw new ApiError(friendlyError(message), {
        status: response.status,
        body: data,
      });
    }
    return data;
  }

  identity() {
    return this.request("/");
  }

  status() {
    return this.request("/api/v1/status");
  }

  account(address) {
    return this.request(`/api/v1/address/${encodeURIComponent(address)}`);
  }

  fees() {
    return this.request("/api/v1/fees");
  }

  history(address, { limit = 25, cursor } = {}) {
    const q = new URLSearchParams({ limit: String(limit) });
    if (cursor) q.set("cursor", cursor);
    return this.request(
      `/api/v1/address/${encodeURIComponent(address)}/txs?${q}`,
    );
  }

  transaction(txid) {
    return this.request(`/api/v1/tx/${encodeURIComponent(txid)}`);
  }

  submit(hex) {
    return this.request("/api/v1/tx", { method: "POST", body: { tx: hex } });
  }

  security(motes) {
    return this.request(`/api/v1/security?amount=${motes}`);
  }

  alerts() {
    return this.request("/api/v1/alerts");
  }
}

/** Known public nodes, offered in Settings. */
export const DEFAULT_NODES = [
  {
    label: "DevNet (testnet-seed1)",
    network: "testnet",
    url: "https://testnet-seed1.the-coin.cloud",
  },
  {
    label: "Mainnet (seed1)",
    network: "mainnet",
    url: "https://seed1.the-coin.cloud",
  },
  { label: "Local node", network: "mainnet", url: "http://127.0.0.1:7334" },
];
