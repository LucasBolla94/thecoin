/**
 * The Coin — protocol core for the browser wallet.
 *
 * Keys, addresses, transaction encoding, signing, fees, amounts and payment
 * request URIs. Everything here mirrors the Rust reference implementation and
 * is covered by wallet/tests against the published vectors
 * (`cargo test -p thecoin-wallet --test vectors`).
 *
 * Amounts are always BigInt motes (1 TCN = 100 000 000 motes). Never floats.
 */
import {
  blake3,
  sha512,
  hmac,
  ed25519,
  bech32m,
  generateMnemonic,
  validateMnemonic,
  mnemonicToSeed,
  englishWordlist,
} from "./vendor/crypto.js";

export const COIN = 100000000n;
export const DECIMALS = 8;
export const TX_VERSION = 1;
export const ADDRESS_LEN = 20;
export const MAX_MEMO_BYTES = 256;
export const MAX_TX_BYTES = 16384;
export const MAX_BATCH_OUTPUTS = 128;
export const COIN_TYPE = 7333;
export const FLAG_REPLACEABLE = 1;

export const NETWORKS = {
  mainnet: { hrp: "tc", chainId: 0x54430001, blockSeconds: 15 },
  testnet: { hrp: "tct", chainId: 0x54430002, blockSeconds: 15 },
  regtest: { hrp: "tcr", chainId: 0x54430003, blockSeconds: 60 },
};

export function networkOf(name) {
  const n = NETWORKS[name];
  if (!n) throw new Error(`unknown network: ${name}`);
  return n;
}

/* ---------------------------------------------------------------- bytes --- */

const te = new TextEncoder();
const td = new TextDecoder();

export function utf8(s) {
  return te.encode(s);
}

export function fromUtf8(bytes) {
  return td.decode(bytes);
}

export function concatBytes(...parts) {
  let len = 0;
  for (const p of parts) len += p.length;
  const out = new Uint8Array(len);
  let at = 0;
  for (const p of parts) {
    out.set(p, at);
    at += p.length;
  }
  return out;
}

export function toHex(bytes) {
  let s = "";
  for (const b of bytes) s += b.toString(16).padStart(2, "0");
  return s;
}

export function fromHex(hex) {
  const clean = hex.trim().toLowerCase();
  if (clean.length % 2 !== 0 || /[^0-9a-f]/.test(clean)) {
    throw new Error("not hexadecimal");
  }
  const out = new Uint8Array(clean.length / 2);
  for (let i = 0; i < out.length; i++) {
    out[i] = parseInt(clean.slice(i * 2, i * 2 + 2), 16);
  }
  return out;
}

/** Overwrites secret material in place. Best effort: JS may keep copies. */
export function wipe(...arrays) {
  for (const a of arrays) if (a && a.fill) a.fill(0);
}

/* ----------------------------------------------------------------- hash --- */

/**
 * BLAKE3("TheCoin:" || tag || 0x00 || parts…) — the chain's domain-separated
 * hash (crates/core/src/hash.rs). Plain mode, 32-byte output.
 */
export function taggedHash(tag, ...parts) {
  return blake3(
    concatBytes(utf8("TheCoin:"), utf8(tag), Uint8Array.of(0), ...parts),
  );
}

/* -------------------------------------------------------------- addresses - */

/** address = taggedHash("address", publicKey)[0..20] */
export function addressFromPublicKey(publicKey) {
  if (publicKey.length !== 32) throw new Error("public key must be 32 bytes");
  return taggedHash("address", publicKey).slice(0, ADDRESS_LEN);
}

export function encodeAddress(address20, network) {
  if (address20.length !== ADDRESS_LEN)
    throw new Error("address must be 20 bytes");
  return bech32m.encode(networkOf(network).hrp, bech32m.toWords(address20));
}

/**
 * Decodes a bech32m address and checks it belongs to `network`.
 * Throws with a message meant to be shown to the user.
 */
export function decodeAddress(text, network) {
  const trimmed = String(text).trim();
  if (trimmed !== trimmed.toLowerCase() && trimmed !== trimmed.toUpperCase()) {
    throw new Error("addresses are never mixed case");
  }
  let decoded;
  try {
    decoded = bech32m.decode(trimmed.toLowerCase());
  } catch {
    throw new Error("this is not a valid address (checksum failed)");
  }
  const want = networkOf(network).hrp;
  if (decoded.prefix !== want) {
    const other = Object.entries(NETWORKS).find(
      ([, n]) => n.hrp === decoded.prefix,
    );
    throw new Error(
      other
        ? `this is a ${other[0]} address; this wallet is on ${network}`
        : `this address is not from ${network}`,
    );
  }
  const bytes = Uint8Array.from(bech32m.fromWords(decoded.words));
  if (bytes.length !== ADDRESS_LEN)
    throw new Error("address has the wrong length");
  return bytes;
}

export function isValidAddress(text, network) {
  try {
    decodeAddress(text, network);
    return true;
  } catch {
    return false;
  }
}

/** The address with 20 zero bytes: coins sent there can never be spent. */
export function isBurnAddress(address20) {
  return address20.every((b) => b === 0);
}

export function shortAddress(text) {
  const s = String(text);
  return s.length <= 20 ? s : `${s.slice(0, 10)}…${s.slice(-6)}`;
}

/* ------------------------------------------------------------------ keys --- */

export function newMnemonic(words = 12) {
  if (words !== 12 && words !== 24) throw new Error("use 12 or 24 words");
  return generateMnemonic(englishWordlist, words === 24 ? 256 : 128);
}

/** Collapses whitespace and lower-cases, like the reference wallet. */
export function normalizeMnemonic(phrase) {
  return String(phrase).trim().split(/\s+/).join(" ").toLowerCase();
}

export function isValidMnemonic(phrase) {
  const normalized = normalizeMnemonic(phrase);
  const count = normalized.split(" ").length;
  if (count !== 12 && count !== 24) return false;
  return validateMnemonic(normalized, englishWordlist);
}

/** BIP-39 seed (64 bytes). Slow on purpose: 2048 PBKDF2-HMAC-SHA512 rounds. */
export async function mnemonicToSeedBytes(phrase, passphrase = "") {
  const normalized = normalizeMnemonic(phrase);
  if (!validateMnemonic(normalized, englishWordlist)) {
    throw new Error("this recovery phrase is not valid");
  }
  return await mnemonicToSeed(normalized, passphrase);
}

/** SLIP-0010 ed25519: every index is hardened. */
export function deriveEd25519(seed, path) {
  let i = hmac(sha512, utf8("ed25519 seed"), seed);
  let key = i.slice(0, 32);
  let chain = i.slice(32);
  for (const index of path) {
    const hardened = new Uint8Array(4);
    new DataView(hardened.buffer).setUint32(
      0,
      (index | 0x80000000) >>> 0,
      false,
    );
    i = hmac(sha512, chain, concatBytes(Uint8Array.of(0), key, hardened));
    key = i.slice(0, 32);
    chain = i.slice(32);
  }
  return key;
}

/** m/44'/7333'/account'/0'/index' — the path of the reference wallet. */
export function accountPath(account, index) {
  return [44, COIN_TYPE, account, 0, index];
}

/** Derives one account: secret key, public key and address text. */
export function accountFromSeed(seed, network, index = 0, account = 0) {
  const secretKey = deriveEd25519(seed, accountPath(account, index));
  const publicKey = ed25519.getPublicKey(secretKey);
  const address20 = addressFromPublicKey(publicKey);
  return {
    index,
    account,
    secretKey,
    publicKey,
    address20,
    address: encodeAddress(address20, network),
  };
}

export function sign(message, secretKey) {
  return ed25519.sign(message, secretKey);
}

export function verify(signature, message, publicKey) {
  return ed25519.verify(signature, message, publicKey);
}

/* ---------------------------------------------------------------- borsh --- */

class Writer {
  constructor() {
    this.parts = [];
  }
  u8(v) {
    this.parts.push(Uint8Array.of(v & 0xff));
    return this;
  }
  u32(v) {
    const b = new Uint8Array(4);
    new DataView(b.buffer).setUint32(0, Number(v) >>> 0, true);
    this.parts.push(b);
    return this;
  }
  u64(v) {
    const b = new Uint8Array(8);
    new DataView(b.buffer).setBigUint64(0, BigInt(v), true);
    this.parts.push(b);
    return this;
  }
  bytes(v) {
    this.parts.push(v);
    return this;
  }
  vecU8(v) {
    return this.u32(v.length).bytes(v);
  }
  finish() {
    return concatBytes(...this.parts);
  }
}

/* ----------------------------------------------------------- transactions - */

/**
 * A transfer body. `to` is 20 bytes, amounts are BigInt motes, `memo` bytes.
 * Borsh layout: version u8 | chain_id u32 | flags u8 | nonce u64 | fee u64 |
 * expiry u64 | action 0x00 | to[20] | amount u64 | memo Vec<u8>.
 */
export function encodeTransferBody({
  network,
  flags = 0,
  nonce,
  fee,
  expiryHeight = 0n,
  to,
  amount,
  memo,
}) {
  const memoBytes = memo ?? new Uint8Array(0);
  if (memoBytes.length > MAX_MEMO_BYTES)
    throw new Error("the memo is too long (256 bytes maximum)");
  if (BigInt(amount) <= 0n)
    throw new Error("the amount must be greater than zero");
  if (to.length !== ADDRESS_LEN)
    throw new Error("destination must be 20 bytes");
  return new Writer()
    .u8(TX_VERSION)
    .u32(networkOf(network).chainId)
    .u8(flags)
    .u64(nonce)
    .u64(fee)
    .u64(expiryHeight)
    .u8(0)
    .bytes(to)
    .u64(amount)
    .vecU8(memoBytes)
    .finish();
}

export function signingHash(bodyBytes) {
  return taggedHash("tx-sign", bodyBytes);
}

/** Signs a body: the message is the 32-byte signing hash, not the body. */
export function signBody(bodyBytes, secretKey, publicKey) {
  const signature = sign(signingHash(bodyBytes), secretKey);
  const tx = concatBytes(bodyBytes, publicKey, signature);
  return { tx, txid: toHex(taggedHash("txid", tx)), signature };
}

export function txidOf(txBytes) {
  return toHex(taggedHash("txid", txBytes));
}

/* ------------------------------------------------------------------ fees --- */

export function ceilDiv(a, b) {
  return (BigInt(a) + BigInt(b) - 1n) / BigInt(b);
}

/**
 * Minimum fee the node accepts for a transaction of `size` bytes
 * (crates/core/src/execution.rs::required_fee). Parameters come from
 * GET /api/v1/fees — never hardcode them, governance can change them.
 */
export function minimumFee(
  { base_fee, fee_per_kb, fee_per_kfuel, congestion_bp },
  size,
  maxFuel = 0n,
) {
  const base =
    BigInt(base_fee) +
    ceilDiv(BigInt(size) * BigInt(fee_per_kb), 1000n) +
    ceilDiv(BigInt(maxFuel) * BigInt(fee_per_kfuel), 1000n);
  const congestion =
    BigInt(congestion_bp) < 10000n ? 10000n : BigInt(congestion_bp);
  return ceilDiv(base * congestion, 10000n);
}

export function feeForPriority(minimum, priorityBp) {
  const bp = BigInt(priorityBp) < 10000n ? 10000n : BigInt(priorityBp);
  return ceilDiv(BigInt(minimum) * bp, 10000n);
}

/** A replacement must pay at least 25% more than the transaction it replaces. */
export function minimumReplacementFee(oldFee) {
  const f = BigInt(oldFee);
  return f + f / 4n;
}

/**
 * Builds and signs a transfer, measuring the real size first (the fee field is
 * fixed width, so signing twice gives the exact size the node will charge for).
 */
export function buildTransfer({
  account,
  network,
  to,
  amount,
  memo,
  nonce,
  expiryHeight,
  feeParams,
  priorityBp,
  replaceable = false,
}) {
  const flags = replaceable ? FLAG_REPLACEABLE : 0;
  const common = { network, flags, nonce, expiryHeight, to, amount, memo };
  const probe = signBody(
    encodeTransferBody({ ...common, fee: 0n }),
    account.secretKey,
    account.publicKey,
  );
  const fee = feeForPriority(
    minimumFee(feeParams, probe.tx.length),
    priorityBp ?? 10000n,
  );
  const body = encodeTransferBody({ ...common, fee });
  const signed = signBody(body, account.secretKey, account.publicKey);
  if (signed.tx.length > MAX_TX_BYTES)
    throw new Error("this transaction is too large");
  return { ...signed, fee, size: signed.tx.length, hex: toHex(signed.tx) };
}

/* --------------------------------------------------------------- amounts --- */

/** "1.5" → 150000000n. Rejects anything the chain would reject. */
export function parseAmount(text) {
  const s = String(text).trim().replace(",", ".");
  if (!/^\d*\.?\d*$/.test(s) || s === "" || s === ".")
    throw new Error("type an amount, such as 12.5");
  const [whole, frac = ""] = s.split(".");
  if (frac.length > DECIMALS)
    throw new Error("TCN has at most 8 decimal places");
  const motes =
    BigInt(whole || "0") * COIN +
    BigInt((frac + "0".repeat(DECIMALS)).slice(0, DECIMALS) || "0");
  if (motes > 0xffffffffffffffffn) throw new Error("amount is too large");
  return motes;
}

/** 150000000n → "1.5" (trailing zeros trimmed), like the Rust formatter. */
export function formatAmount(motes) {
  const v = BigInt(motes);
  const negative = v < 0n;
  const abs = negative ? -v : v;
  const whole = abs / COIN;
  const frac = (abs % COIN)
    .toString()
    .padStart(DECIMALS, "0")
    .replace(/0+$/, "");
  return `${negative ? "-" : ""}${whole}${frac ? "." + frac : ""}`;
}

/** Groups thousands for display only: 1234.5 → "1,234.5". */
export function formatTCN(motes, { withTicker = true } = {}) {
  const [whole, frac] = formatAmount(motes).split(".");
  const grouped = whole.replace(/\B(?=(\d{3})+(?!\d))/g, ",");
  return `${grouped}${frac ? "." + frac : ""}${withTicker ? " TCN" : ""}`;
}

/* ------------------------------------------------ payment request URIs ----- */

const URI_UNRESERVED = /[A-Za-z0-9\-_.~]/;

function percentEncode(text) {
  let out = "";
  for (const byte of utf8(text)) {
    const ch = String.fromCharCode(byte);
    out += URI_UNRESERVED.test(ch)
      ? ch
      : `%${byte.toString(16).toUpperCase().padStart(2, "0")}`;
  }
  return out;
}

function percentDecode(text) {
  const bytes = [];
  for (let i = 0; i < text.length; i++) {
    const ch = text[i];
    if (ch === "+") bytes.push(32);
    else if (ch === "%" && i + 2 < text.length + 1) {
      bytes.push(parseInt(text.slice(i + 1, i + 3), 16));
      i += 2;
    } else bytes.push(ch.charCodeAt(0));
  }
  return fromUtf8(Uint8Array.from(bytes));
}

/** thecoin:<address>?amount=<TCN>&memo=&label= */
export function buildPaymentUri({ address, amount, memo, label }) {
  const params = [];
  if (amount !== undefined && amount !== null && amount !== "")
    params.push(`amount=${formatAmount(amount)}`);
  if (memo) params.push(`memo=${percentEncode(memo)}`);
  if (label) params.push(`label=${percentEncode(label)}`);
  return `thecoin:${address}${params.length ? "?" + params.join("&") : ""}`;
}

/** Parses a payment request. Unknown parameters are ignored, as in the CLI. */
export function parsePaymentUri(uri, network) {
  const text = String(uri).trim();
  if (!/^thecoin:/i.test(text))
    throw new Error("this is not a The Coin payment request");
  const rest = text.slice("thecoin:".length);
  const [addressPart, queryPart = ""] = rest.split("?");
  const address = addressPart.trim();
  decodeAddress(address, network); // throws on wrong network or bad checksum
  const out = { address, amount: null, memo: null, label: null };
  for (const pair of queryPart.split("&")) {
    if (!pair) continue;
    const eq = pair.indexOf("=");
    const key = (eq < 0 ? pair : pair.slice(0, eq)).toLowerCase();
    const value = eq < 0 ? "" : percentDecode(pair.slice(eq + 1));
    if (key === "amount") out.amount = parseAmount(value);
    else if (key === "memo") out.memo = value;
    else if (key === "label") out.label = value;
  }
  return out;
}

/* ----------------------------------------------------------------- misc --- */

/** Rough time for a number of blocks, for messages like "about 6 hours". */
export function blocksToText(blocks, network) {
  const seconds = Number(blocks) * networkOf(network).blockSeconds;
  if (seconds < 90) return `${Math.round(seconds)} seconds`;
  if (seconds < 5400) return `${Math.round(seconds / 60)} minutes`;
  if (seconds < 172800)
    return `${(seconds / 3600).toFixed(1).replace(/\.0$/, "")} hours`;
  return `${(seconds / 86400).toFixed(1).replace(/\.0$/, "")} days`;
}
