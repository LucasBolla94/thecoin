/**
 * The node sends amounts as JSON integers, and motes can exceed what a
 * JavaScript number holds exactly (the supply cap is 10^16). These tests pin
 * the parsing so a balance can never be shown — or spent — rounded.
 */
import { test } from "node:test";
import assert from "node:assert/strict";
import { NodeApi, friendlyError } from "../assets/api.js";

function fakeFetch(body, { status = 200 } = {}) {
  globalThis.fetch = async () => ({
    ok: status >= 200 && status < 300,
    status,
    text: async () => body,
  });
}

test("large amounts survive as exact BigInt", async () => {
  fakeFetch(
    '{"address":"tct1x","balance":10000000000000001,"spendable":9007199254740993,"nonce":3}',
  );
  const account = await new NodeApi("https://node.example").account("tct1x");
  assert.equal(account.balance, 10000000000000001n);
  assert.equal(
    account.spendable,
    9007199254740993n,
    "would be 9007199254740992 as a Number",
  );
  assert.equal(account.nonce, 3, "plain counters stay numbers");
});

test("nested amounts and arrays are converted too", async () => {
  fakeFetch(
    '{"supply":{"max_supply":10000000000000000,"emitted":123},"txs":[{"fee":2630,"amount":150000000}]}',
  );
  const data = await new NodeApi("https://node.example").status();
  assert.equal(data.supply.max_supply, 10000000000000000n);
  assert.equal(data.supply.emitted, 123n);
  assert.equal(data.txs[0].fee, 2630n);
  assert.equal(data.txs[0].amount, 150000000n);
});

test("node errors become messages a person can act on", async () => {
  fakeFetch(
    '{"error":"invalid transaction: insufficient funds: need 2500002630, spendable 100"}',
    { status: 400 },
  );
  await assert.rejects(
    () => new NodeApi("https://node.example").submit("00"),
    /Not enough spendable balance/,
  );
  assert.match(
    friendlyError("invalid transaction: fee too low: minimum 2630, got 263"),
    /below the network minimum/,
  );
  assert.match(
    friendlyError("address index disabled on this node"),
    /pruned node/,
  );
  assert.equal(
    friendlyError("something new"),
    "something new",
    "unknown errors are shown as they are",
  );
});

test("an unreachable node explains itself", async () => {
  globalThis.fetch = async () => {
    throw new TypeError("failed to fetch");
  };
  await assert.rejects(
    () => new NodeApi("https://node.example").status(),
    /Could not reach the node/,
  );
});
