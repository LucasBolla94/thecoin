// Everything the wallet needs from audited libraries, re-exported as one module.
// Bundled into wallet/assets/vendor/crypto.js by `npm run build`; nothing is
// fetched from a CDN at runtime (the site's CSP only allows our own origin).
export { blake3 } from "@noble/hashes/blake3";
export { sha512 } from "@noble/hashes/sha512";
export { sha256 } from "@noble/hashes/sha256";
export { hmac } from "@noble/hashes/hmac";
export { pbkdf2Async } from "@noble/hashes/pbkdf2";
export { ed25519 } from "@noble/curves/ed25519";
export { bech32m } from "@scure/base";
export {
  generateMnemonic,
  validateMnemonic,
  mnemonicToSeed,
  entropyToMnemonic,
  mnemonicToEntropy,
} from "@scure/bip39";
export { wordlist as englishWordlist } from "@scure/bip39/wordlists/english";

// QR codes for the receive screen (MIT, no dependencies).
import qrcode from "qrcode-generator";
export { qrcode };
