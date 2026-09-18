/**
 * Encrypted storage for the recovery phrase.
 *
 * The phrase is encrypted with a key derived from the user's password
 * (PBKDF2-HMAC-SHA-256, 600 000 rounds, WebCrypto) and sealed with AES-256-GCM.
 * Only the encrypted blob and public data (addresses, labels, settings) ever
 * reach localStorage; the decrypted phrase and the keys derived from it live in
 * memory while the wallet is unlocked and are wiped on lock.
 *
 * The node never sees any of this: it only ever receives signed transactions.
 */
import { accountFromSeed, mnemonicToSeedBytes, wipe } from "./thecoin.js";

const STORAGE_KEY = "thecoin.wallet.v1";
const SETTINGS_KEY = "thecoin.settings.v1";
const PBKDF2_ROUNDS = 600000;
const VAULT_VERSION = 1;

const te = new TextEncoder();
const td = new TextDecoder();

function toBase64(bytes) {
  let s = "";
  for (const b of bytes) s += String.fromCharCode(b);
  return btoa(s);
}

function fromBase64(text) {
  const raw = atob(text);
  const out = new Uint8Array(raw.length);
  for (let i = 0; i < raw.length; i++) out[i] = raw.charCodeAt(i);
  return out;
}

async function deriveKey(password, salt, rounds) {
  const material = await crypto.subtle.importKey(
    "raw",
    te.encode(password),
    "PBKDF2",
    false,
    ["deriveKey"],
  );
  return crypto.subtle.deriveKey(
    { name: "PBKDF2", hash: "SHA-256", salt, iterations: rounds },
    material,
    { name: "AES-GCM", length: 256 },
    false,
    ["encrypt", "decrypt"],
  );
}

/** How strong a password looks, for the meter on the create screen. */
export function passwordStrength(password) {
  const p = String(password);
  let score = 0;
  if (p.length >= 8) score++;
  if (p.length >= 12) score++;
  if (/[a-z]/.test(p) && /[A-Z]/.test(p)) score++;
  if (/\d/.test(p)) score++;
  if (/[^A-Za-z0-9]/.test(p)) score++;
  if (p.length < 8)
    return { score: 0, label: "Too short", hint: "Use at least 8 characters." };
  return {
    score: Math.min(score, 5),
    label: ["Weak", "Weak", "Fair", "Good", "Strong", "Strong"][
      Math.min(score, 5)
    ],
    hint: score < 3 ? "Mix upper and lower case, numbers or symbols." : "",
  };
}

/** The wallet as stored in the browser. Public data only, plus the sealed phrase. */
function readStored() {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    return raw ? JSON.parse(raw) : null;
  } catch {
    return null;
  }
}

function writeStored(value) {
  localStorage.setItem(STORAGE_KEY, JSON.stringify(value));
}

export function walletExists() {
  const stored = readStored();
  return Boolean(stored && stored.cipher);
}

export function readSettings() {
  const defaults = {
    network: "testnet",
    nodeUrl: "https://testnet-seed1.the-coin.cloud",
    autoLockMinutes: 15,
    priority: "normal",
  };
  try {
    return {
      ...defaults,
      ...JSON.parse(localStorage.getItem(SETTINGS_KEY) || "{}"),
    };
  } catch {
    return defaults;
  }
}

export function writeSettings(settings) {
  localStorage.setItem(
    SETTINGS_KEY,
    JSON.stringify({ ...readSettings(), ...settings }),
  );
}

/** Everything the wallet keeps about its accounts, without any secret. */
export function readAccounts() {
  const stored = readStored();
  return stored && Array.isArray(stored.accounts)
    ? stored.accounts
    : [{ index: 0, label: "Account 1" }];
}

export function writeAccounts(accounts) {
  const stored = readStored();
  if (!stored) return;
  writeStored({ ...stored, accounts });
}

export function readContacts() {
  const stored = readStored();
  return stored && Array.isArray(stored.contacts) ? stored.contacts : [];
}

export function writeContacts(contacts) {
  const stored = readStored();
  if (!stored) return;
  writeStored({ ...stored, contacts });
}

/** Signed transactions this wallet sent, so history works on pruned nodes. */
export function readSent() {
  const stored = readStored();
  return stored && Array.isArray(stored.sent) ? stored.sent : [];
}

export function recordSent(entry) {
  const stored = readStored();
  if (!stored) return;
  const sent = [entry, ...readSent()].slice(0, 200);
  writeStored({ ...stored, sent });
}

/** Encrypts the phrase under a password and replaces whatever was stored. */
export async function createVault({ mnemonic, password, accounts }) {
  const salt = crypto.getRandomValues(new Uint8Array(16));
  const iv = crypto.getRandomValues(new Uint8Array(12));
  const key = await deriveKey(password, salt, PBKDF2_ROUNDS);
  const plaintext = te.encode(JSON.stringify({ mnemonic, passphrase: "" }));
  const sealed = new Uint8Array(
    await crypto.subtle.encrypt({ name: "AES-GCM", iv }, key, plaintext),
  );
  wipe(plaintext);
  writeStored({
    version: VAULT_VERSION,
    createdAt: new Date().toISOString(),
    kdf: {
      name: "pbkdf2-sha256",
      iterations: PBKDF2_ROUNDS,
      salt: toBase64(salt),
    },
    cipher: {
      name: "aes-256-gcm",
      iv: toBase64(iv),
      ciphertext: toBase64(sealed),
    },
    accounts: accounts || [{ index: 0, label: "Account 1" }],
    contacts: readContacts(),
    sent: [],
  });
}

/** Returns the recovery phrase, or throws if the password is wrong. */
export async function openVault(password) {
  const stored = readStored();
  if (!stored || !stored.cipher)
    throw new Error("There is no wallet in this browser.");
  const key = await deriveKey(
    password,
    fromBase64(stored.kdf.salt),
    stored.kdf.iterations,
  );
  let plaintext;
  try {
    plaintext = new Uint8Array(
      await crypto.subtle.decrypt(
        { name: "AES-GCM", iv: fromBase64(stored.cipher.iv) },
        key,
        fromBase64(stored.cipher.ciphertext),
      ),
    );
  } catch {
    throw new Error("Wrong password.");
  }
  const data = JSON.parse(td.decode(plaintext));
  wipe(plaintext);
  return data.mnemonic;
}

/** Re-encrypts the stored phrase under a new password. */
export async function changePassword(oldPassword, newPassword) {
  const mnemonic = await openVault(oldPassword);
  await createVaultKeepingData(mnemonic, newPassword);
}

async function createVaultKeepingData(mnemonic, password) {
  const accounts = readAccounts();
  const contacts = readContacts();
  const sent = readSent();
  await createVault({ mnemonic, password, accounts });
  const stored = readStored();
  writeStored({ ...stored, contacts, sent });
}

/** Deletes the wallet from this browser. The phrase is the only way back. */
export function forgetWallet() {
  localStorage.removeItem(STORAGE_KEY);
}

/**
 * An unlocked wallet: the derived accounts live here, in memory only.
 * `lock()` wipes every key it derived.
 */
export class UnlockedWallet {
  constructor(mnemonic, seed, network) {
    this.mnemonic = mnemonic;
    this.seed = seed;
    this.network = network;
    this.derived = new Map();
  }

  static async open(mnemonic, network) {
    const seed = await mnemonicToSeedBytes(mnemonic);
    return new UnlockedWallet(mnemonic, seed, network);
  }

  /** Derives (and caches) one account of this wallet. */
  account(index) {
    const cacheKey = `${this.network}:${index}`;
    if (!this.derived.has(cacheKey)) {
      this.derived.set(
        cacheKey,
        accountFromSeed(this.seed, this.network, index),
      );
    }
    return this.derived.get(cacheKey);
  }

  /** Same phrase, other network: addresses change, the keys do not. */
  setNetwork(network) {
    this.network = network;
  }

  lock() {
    for (const account of this.derived.values()) wipe(account.secretKey);
    this.derived.clear();
    wipe(this.seed);
    this.mnemonic = null;
  }
}
