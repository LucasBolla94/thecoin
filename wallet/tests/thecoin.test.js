/**
 * Checks the wallet's protocol core against the published test vectors
 * (crates/wallet/tests/vectors.rs, docs/WALLET_DEVELOPERS.md §8).
 *
 *   cd tools/wallet-build && npm test
 *
 * If one of these fails, the wallet would produce transactions the network
 * rejects — or, worse, addresses whose coins nobody can spend.
 */
import { test } from "node:test";
import assert from "node:assert/strict";
import {
  taggedHash,
  toHex,
  fromHex,
  utf8,
  addressFromPublicKey,
  encodeAddress,
  decodeAddress,
  isValidAddress,
  mnemonicToSeedBytes,
  deriveEd25519,
  accountFromSeed,
  accountPath,
  encodeTransferBody,
  signingHash,
  signBody,
  minimumFee,
  feeForPriority,
  minimumReplacementFee,
  parseAmount,
  formatAmount,
  formatTCN,
  buildPaymentUri,
  parsePaymentUri,
  newMnemonic,
  isValidMnemonic,
  normalizeMnemonic,
} from "../assets/thecoin.js";

const MNEMONIC =
  "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";

test("tagged hash matches the chain's domain separation", () => {
  assert.equal(
    toHex(taggedHash("txid")),
    "38aa34a475f8d3b9c7b15275271dce321fc9e6835e273e6672c9ec3c4b653b6a",
  );
  assert.equal(
    toHex(taggedHash("block", utf8("abc"))),
    "09bb3b792db7c48af43cdb15ba3f1b274440d964372789fadcc4d8da9c1679d8",
  );
});

test("SLIP-0010 official ed25519 vector 1", () => {
  const seed = fromHex("000102030405060708090a0b0c0d0e0f");
  assert.equal(
    toHex(deriveEd25519(seed, [])),
    "2b4be7f19ee27bbf30c667b642d5f4aa69fd169872f8fc3059c08ebae2eb19e7",
  );
  assert.equal(
    toHex(deriveEd25519(seed, [0])),
    "68e0fe46dfb67e368c75379acec591dad19df3cde26e63b93a8e704f1dade7a3",
  );
  assert.equal(
    toHex(deriveEd25519(seed, [0, 1, 2, 2, 1000000000])),
    "8f94d394a8e8fd6b1bc2f3f49f5c47e385281d5c17e65324b0f62483e37e8793",
  );
});

test("BIP-39 seed and the wallet's derivation path", async () => {
  const seed = await mnemonicToSeedBytes(MNEMONIC);
  assert.equal(
    toHex(seed),
    "5eb00bbddcf069084889a8ab9155568165f5c453ccb85e70811aaed6f6da5fc1" +
      "9a5ac40b389cd370d086206dec8aa6c43daea6690f20ad3d8d48b2d2ce9e38e4",
  );
  assert.equal(
    toHex(deriveEd25519(seed, [])),
    "560f9f3c94558b6551928bb781cf6092c6b8800b4fc544af2c9444ed126d51aa",
  );
  assert.deepEqual(accountPath(0, 0), [44, 7333, 0, 0, 0]);
});

test("accounts, public keys and addresses of the published vector", async () => {
  const seed = await mnemonicToSeedBytes(MNEMONIC);
  const a0 = accountFromSeed(seed, "mainnet", 0);
  assert.equal(
    toHex(a0.secretKey),
    "a146f5dbeff18a4189d3bb0a03c636252ee07dba1fbd6e2e954fa3feff27db97",
  );
  assert.equal(
    toHex(a0.publicKey),
    "67a6bacd6720c598f215b3d3a7cee7433a583f24f477fe7096d09a53fb5dbe06",
  );
  assert.equal(toHex(a0.address20), "4d54f2145ebf35d8509935f16fb1ebb7efa47d64");
  assert.equal(a0.address, "tc1f420y9z7hu6as5yexhcklv0tklh6glty0n9yvj");
  assert.equal(
    encodeAddress(a0.address20, "testnet"),
    "tct1f420y9z7hu6as5yexhcklv0tklh6gltyfk2we0",
  );
  assert.equal(
    encodeAddress(a0.address20, "regtest"),
    "tcr1f420y9z7hu6as5yexhcklv0tklh6glty6qra7u",
  );

  const a1 = accountFromSeed(seed, "mainnet", 1);
  assert.equal(
    toHex(a1.secretKey),
    "34e4f81414e800631d613d26011450e307870118a6ca68a99d2ff833cebbd028",
  );
  assert.equal(
    toHex(a1.publicKey),
    "c4595ca32d93a95c85896b9988ee0d2181d867b892433358314f7887f365cf5a",
  );
  assert.equal(a1.address, "tc1ggq2vlghwjy9mpmnrhktr3r58saswpz5pc78u4");
  assert.equal(
    encodeAddress(a1.address20, "testnet"),
    "tct1ggq2vlghwjy9mpmnrhktr3r58saswpz58a3dfg",
  );
});

test("address from the all-zero public key", () => {
  assert.equal(
    toHex(addressFromPublicKey(new Uint8Array(32))),
    "7b32d1267e52aebd6fde6e0bdc70e394c133183e",
  );
});

test("address decoding refuses other networks and broken checksums", () => {
  const mainnet = "tc1f420y9z7hu6as5yexhcklv0tklh6glty0n9yvj";
  assert.equal(
    toHex(decodeAddress(mainnet, "mainnet")),
    "4d54f2145ebf35d8509935f16fb1ebb7efa47d64",
  );
  assert.throws(() => decodeAddress(mainnet, "testnet"), /mainnet address/);
  assert.throws(
    () => decodeAddress(mainnet.slice(0, -1) + "q", "mainnet"),
    /checksum/,
  );
  assert.throws(
    () => decodeAddress("tc1F420y9z7hu6as5yexhcklv0tklh6glty0n9yvj", "mainnet"),
    /mixed case/,
  );
  assert.equal(isValidAddress("not an address", "mainnet"), false);
});

test("golden transfer: body, signing hash, signature, txid", async () => {
  const seed = await mnemonicToSeedBytes(MNEMONIC);
  const sender = accountFromSeed(seed, "mainnet", 0);
  const to = decodeAddress(
    "tc1ggq2vlghwjy9mpmnrhktr3r58saswpz5pc78u4",
    "mainnet",
  );
  const body = encodeTransferBody({
    network: "mainnet",
    flags: 0,
    nonce: 0n,
    fee: 2630n,
    expiryHeight: 0n,
    to,
    amount: 150000000n,
    memo: utf8("test"),
  });
  assert.equal(
    toHex(body),
    "0101004354000000000000000000460a0000000000000000000000000000004200a67d1774885d87731decb1c4743c" +
      "3b07045480d1f008000000000400000074657374",
  );
  assert.equal(body.length, 67);
  assert.equal(
    toHex(signingHash(body)),
    "e81ec27086aa0dfe1bfc204fade8dee9290eefef33b511bc865eb32770fd6fed",
  );

  const signed = signBody(body, sender.secretKey, sender.publicKey);
  assert.equal(
    toHex(signed.signature),
    "16e8dec69deadd63934267d3a321def787aa20ad7ceb09633a2440f1e67a167f" +
      "cdeb13b4e0375490de0bfa1e0d445c88babf44d205545000546376bd7c274409",
  );
  assert.equal(signed.tx.length, 163);
  assert.equal(
    signed.txid,
    "492e5c610cd3913d94a20a2cd5ddc076cec7d3f3b8abcc718923fdc9fe595d02",
  );
});

test("golden replaceable transfer", async () => {
  const seed = await mnemonicToSeedBytes(MNEMONIC);
  const sender = accountFromSeed(seed, "mainnet", 0);
  const to = decodeAddress(
    "tc1ggq2vlghwjy9mpmnrhktr3r58saswpz5pc78u4",
    "mainnet",
  );
  const signed = signBody(
    encodeTransferBody({
      network: "mainnet",
      flags: 1,
      nonce: 1n,
      fee: 2630n,
      expiryHeight: 0n,
      to,
      amount: 150000000n,
      memo: utf8("test"),
    }),
    sender.secretKey,
    sender.publicKey,
  );
  assert.equal(
    toHex(signed.tx),
    "0101004354010100000000000000460a0000000000000000000000000000004200a67d1774885d87731decb1c4743c3b0" +
      "7045480d1f00800000000040000007465737467a6bacd6720c598f215b3d3a7cee7433a583f24f477fe7096d09a53fb5" +
      "dbe06a51c9ba4cbad1ffae073abd7b0418daff5aff0359998f74f5018f80f7490bd677b801e24a7336f0df63040f6a01" +
      "ac87e952cebc913be442c088332c0f4165809",
  );
  assert.equal(
    signed.txid,
    "e3d311122d703363416d23c266dde108b19a47b756359117b3d388f9cfa7dcbe",
  );
  assert.equal(minimumReplacementFee(2630n), 3287n);
});

test("fees follow the node's formula", () => {
  const params = {
    base_fee: 1000,
    fee_per_kb: 10000,
    fee_per_kfuel: 1000,
    congestion_bp: 10000,
  };
  assert.equal(minimumFee(params, 163), 2630n);
  assert.equal(feeForPriority(2630n, 12500n), 3288n);
  assert.equal(feeForPriority(2630n, 10000n), 2630n);
  assert.equal(
    feeForPriority(2630n, 5000n),
    2630n,
    "priority below the minimum is clamped",
  );
  assert.equal(minimumFee({ ...params, congestion_bp: 20000 }, 163), 5260n);
});

test("amounts are exact, with no floating point", () => {
  assert.equal(parseAmount("1.5"), 150000000n);
  assert.equal(parseAmount("0.00000001"), 1n);
  assert.equal(parseAmount(".00000001"), 1n);
  assert.equal(parseAmount("12"), 1200000000n);
  assert.throws(() => parseAmount("1.123456789"), /8 decimal/);
  assert.throws(() => parseAmount("-1"), /amount/);
  assert.throws(() => parseAmount("1e5"), /amount/);
  assert.equal(formatAmount(150000000n), "1.5");
  assert.equal(formatAmount(1n), "0.00000001");
  assert.equal(formatAmount(0n), "0");
  assert.equal(formatAmount(10000000000000000n), "100000000");
  assert.equal(formatTCN(123456789012345n), "1,234,567.89012345 TCN");
});

test("payment request URIs", () => {
  const address = "tc1ggq2vlghwjy9mpmnrhktr3r58saswpz5pc78u4";
  const uri = buildPaymentUri({
    address,
    amount: 350000000n,
    memo: "Pedido #123",
    label: "Loja",
  });
  assert.equal(
    uri,
    `thecoin:${address}?amount=3.5&memo=Pedido%20%23123&label=Loja`,
  );
  const parsed = parsePaymentUri(uri, "mainnet");
  assert.equal(parsed.address, address);
  assert.equal(parsed.amount, 350000000n);
  assert.equal(parsed.memo, "Pedido #123");
  assert.equal(parsed.label, "Loja");
  assert.equal(
    parsePaymentUri(`thecoin:${address}?ignored=1`, "mainnet").amount,
    null,
  );
  assert.equal(
    parsePaymentUri(`thecoin:${address}?memo=a+b`, "mainnet").memo,
    "a b",
  );
  assert.throws(
    () => parsePaymentUri(`thecoin:${address}`, "testnet"),
    /mainnet address/,
  );
  assert.throws(
    () => parsePaymentUri("bitcoin:x", "mainnet"),
    /not a The Coin/,
  );
});

test("mnemonics round-trip and are validated", async () => {
  for (const words of [12, 24]) {
    const phrase = newMnemonic(words);
    assert.equal(phrase.split(" ").length, words);
    assert.ok(isValidMnemonic(phrase));
    const seed = await mnemonicToSeedBytes(phrase);
    assert.equal(seed.length, 64);
  }
  assert.equal(isValidMnemonic("abandon abandon abandon"), false);
  assert.equal(
    isValidMnemonic(MNEMONIC.replace("about", "abandon")),
    false,
    "checksum must fail",
  );
  assert.equal(normalizeMnemonic("  Abandon   ABOUT \n"), "abandon about");
  await assert.rejects(() => mnemonicToSeedBytes("not a phrase"), /not valid/);
});

test("memo and amount limits are enforced before signing", () => {
  const to = decodeAddress(
    "tc1ggq2vlghwjy9mpmnrhktr3r58saswpz5pc78u4",
    "mainnet",
  );
  const base = { network: "mainnet", nonce: 0n, fee: 0n, to, amount: 1n };
  assert.throws(
    () => encodeTransferBody({ ...base, memo: new Uint8Array(257) }),
    /memo is too long/,
  );
  assert.throws(
    () => encodeTransferBody({ ...base, amount: 0n }),
    /greater than zero/,
  );
});
