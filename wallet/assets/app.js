/**
 * The Coin wallet — screens and flows.
 *
 * Keys are derived and used only in this page; the node only ever receives a
 * signed transaction. Everything that touches money goes through thecoin.js,
 * which is checked against the chain's published test vectors.
 */
import {
  COIN,
  NETWORKS,
  buildTransfer,
  buildPaymentUri,
  decodeAddress,
  encodeAddress,
  formatAmount,
  formatTCN,
  isBurnAddress,
  minimumFee,
  newMnemonic,
  isValidMnemonic,
  normalizeMnemonic,
  parseAmount,
  parsePaymentUri,
  shortAddress,
  utf8,
  blocksToText,
  MAX_MEMO_BYTES,
} from "./thecoin.js";
import { NodeApi, DEFAULT_NODES, ApiError } from "./api.js";
import {
  UnlockedWallet,
  createVault,
  openVault,
  changePassword,
  forgetWallet,
  passwordStrength,
  readAccounts,
  writeAccounts,
  readContacts,
  writeContacts,
  readSent,
  recordSent,
  readSettings,
  writeSettings,
  walletExists,
} from "./vault.js";
import { qrcode } from "./vendor/crypto.js";

const view = document.getElementById("view");
const tabs = document.getElementById("tabs");
const chip = document.getElementById("network-chip");
const lockBtn = document.getElementById("lock-btn");

const state = {
  settings: readSettings(),
  api: null,
  wallet: null,
  accountIndex: 0,
  status: null,
  fees: null,
  account: null,
  tab: "home",
  history: [],
  historyError: null,
  pending: null, // a dapp request waiting for approval
  lockTimer: null,
  draft: {},
};

state.api = new NodeApi(state.settings.nodeUrl);

/* ------------------------------------------------------------- helpers --- */

const e = (text) =>
  String(text ?? "").replace(
    /[&<>"']/g,
    (c) =>
      ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[
        c
      ],
  );

function render(html, { tab = true } = {}) {
  view.innerHTML = html;
  tabs.hidden = !tab || !state.wallet;
  lockBtn.hidden = !state.wallet;
  for (const button of tabs.querySelectorAll("button")) {
    button.setAttribute(
      "aria-current",
      String(button.dataset.tab === state.tab),
    );
  }
  view.scrollTop = 0;
}

function toast(message, ms = 2600) {
  document.querySelector(".toast")?.remove();
  const node = document.createElement("div");
  node.className = "toast";
  node.textContent = message;
  node.setAttribute("role", "status");
  document.body.append(node);
  setTimeout(() => node.remove(), ms);
}

function sheet(html) {
  document.querySelector(".sheet")?.remove();
  const node = document.createElement("div");
  node.className = "sheet";
  node.innerHTML = `<div class="inner" role="dialog" aria-modal="true">${html}</div>`;
  node.addEventListener("click", (event) => {
    if (event.target === node) node.remove();
  });
  document.body.append(node);
  return node;
}

const closeSheet = () => document.querySelector(".sheet")?.remove();

function $(selector) {
  return view.querySelector(selector) || document.querySelector(selector);
}

function copy(text, what = "Copied") {
  navigator.clipboard?.writeText(text).then(
    () => toast(what),
    () => toast("Could not copy"),
  );
}

function explorerLink(txid) {
  // The explorer calls the testnet "devnet" in its network selector.
  const network = state.settings.network === "mainnet" ? "mainnet" : "devnet";
  return `https://explore.the-coin.cloud/?network=${network}#/tx/${txid}`;
}

function qrSvg(text) {
  const qr = qrcode(0, "M");
  qr.addData(text, "Byte");
  qr.make();
  return qr.createSvgTag({ cellSize: 4, margin: 2, scalable: true });
}

/* ---------------------------------------------------------- lock timer --- */

function touchActivity() {
  clearTimeout(state.lockTimer);
  const minutes = Number(state.settings.autoLockMinutes || 15);
  if (!state.wallet || minutes <= 0) return;
  state.lockTimer = setTimeout(
    () => lock("Locked after inactivity."),
    minutes * 60000,
  );
}

function lock(message) {
  state.wallet?.lock();
  state.wallet = null;
  state.account = null;
  state.history = [];
  clearTimeout(state.lockTimer);
  screenUnlock(message);
}

for (const event of ["click", "keydown", "touchstart"]) {
  document.addEventListener(event, touchActivity, { passive: true });
}
lockBtn.addEventListener("click", () => lock("Wallet locked."));
tabs.addEventListener("click", (event) => {
  const tab = event.target.closest("button")?.dataset.tab;
  if (!tab || !state.wallet) return;
  state.tab = tab;
  ({
    home: screenHome,
    send: screenSend,
    receive: screenReceive,
    settings: screenSettings,
  })[tab]();
});

/* ------------------------------------------------------------ network ---- */

async function refresh({ quiet = false } = {}) {
  if (!state.wallet) return;
  const account = state.wallet.account(state.accountIndex);
  try {
    const [status, fees, info] = await Promise.all([
      state.api.status(),
      state.api.fees(),
      state.api.account(account.address),
    ]);
    state.status = status;
    state.fees = fees;
    state.account = info;
    chip.className = "chip live";
    chip.textContent = `${status.network} · ${Number(status.height).toLocaleString()}`;
    if (status.network !== state.settings.network) {
      chip.className = "chip down";
      chip.textContent = `node is on ${status.network}`;
    }
  } catch (err) {
    chip.className = "chip down";
    chip.textContent = "node unreachable";
    if (!quiet) toast(err.message);
  }
}

async function loadHistory() {
  if (!state.wallet) return;
  const account = state.wallet.account(state.accountIndex);
  try {
    state.history = await state.api.history(account.address, { limit: 25 });
    state.historyError = null;
  } catch (err) {
    state.history = [];
    state.historyError = err.message;
  }
}

/* --------------------------------------------------------- onboarding ---- */

function screenWelcome() {
  state.tab = "home";
  render(
    `<div class="screen">
      <div class="card" style="align-items:center;text-align:center">
        <div class="mark" style="width:54px;height:54px;border-radius:50%;background:var(--accent);color:var(--accent-ink);display:grid;place-items:center;font-size:20px;font-weight:700">tc</div>
        <h1>Your TCN, in your browser</h1>
        <p class="muted">Hold, send and receive The Coin. Your recovery phrase is encrypted on this device and never sent anywhere.</p>
      </div>
      <button class="primary full" id="go-create">Create a new wallet</button>
      <button class="secondary full" id="go-import">I already have a recovery phrase</button>
      <p class="muted">New to The Coin? A wallet is just a key. Whoever has the phrase has the coins — nobody can reset it for you.</p>
    </div>`,
    { tab: false },
  );
  $("#go-create").onclick = () =>
    screenCreate(1, { mnemonic: newMnemonic(12) });
  $("#go-import").onclick = () => screenImport();
}

function stepBar(step, total = 3) {
  return `<div class="steps">${Array.from({ length: total }, (_, i) => `<span class="${i < step ? "on" : ""}"></span>`).join("")}</div>`;
}

function screenCreate(step, data) {
  if (step === 1) {
    render(
      `<div class="screen">${stepBar(1)}
        <h1>Choose a password</h1>
        <p class="muted">It unlocks the wallet on this device. It is not your recovery phrase and it cannot restore your coins elsewhere.</p>
        <div><label for="pw">Password</label><input type="password" id="pw" autocomplete="new-password" />
          <div class="strength"><span id="meter" style="width:0"></span></div>
          <div class="hint" id="pw-hint">At least 8 characters.</div></div>
        <div><label for="pw2">Repeat the password</label><input type="password" id="pw2" autocomplete="new-password" /></div>
        <div class="field-error" id="err" hidden></div>
        <button class="primary full" id="next">Continue</button>
        <button class="ghost full" id="back">Back</button>
      </div>`,
      { tab: false },
    );
    const meter = $("#meter");
    $("#pw").oninput = (event) => {
      const strength = passwordStrength(event.target.value);
      meter.style.width = `${(strength.score / 5) * 100}%`;
      $("#pw-hint").textContent = `${strength.label}. ${strength.hint}`;
    };
    $("#back").onclick = screenWelcome;
    $("#next").onclick = () => {
      const password = $("#pw").value;
      const error = $("#err");
      error.hidden = true;
      if (passwordStrength(password).score === 0) {
        error.textContent = "Use at least 8 characters.";
        error.hidden = false;
        return;
      }
      if (password !== $("#pw2").value) {
        error.textContent = "The two passwords are different.";
        error.hidden = false;
        return;
      }
      screenCreate(2, { ...data, password });
    };
    return;
  }

  if (step === 2) {
    const words = data.mnemonic.split(" ");
    render(
      `<div class="screen">${stepBar(2)}
        <h1>Write down these 12 words</h1>
        <div class="notice warn">Anyone with these words can spend your coins. Never type them into a website or send them to anyone — not even to support.</div>
        <div class="words">${words.map((w, i) => `<span class="word"><b>${i + 1}</b>${e(w)}</span>`).join("")}</div>
        <button class="secondary full" id="copy">Copy the phrase</button>
        <p class="muted">Paper is safer than a screenshot: a photo in the cloud is a copy of your money.</p>
        <button class="primary full" id="next">I wrote them down</button>
      </div>`,
      { tab: false },
    );
    $("#copy").onclick = () =>
      copy(
        data.mnemonic,
        "Phrase copied — paste it somewhere safe, then clear the clipboard",
      );
    $("#next").onclick = () => screenCreate(3, data);
    return;
  }

  const words = data.mnemonic.split(" ");
  const asked = [2, 6, 10].map((n) => n - 1);
  render(
    `<div class="screen">${stepBar(3)}
      <h1>Check the phrase</h1>
      <p class="muted">Type the words below to confirm you saved them.</p>
      ${asked
        .map(
          (i) =>
            `<div><label for="w${i}">Word ${i + 1}</label><input type="text" id="w${i}" autocapitalize="off" autocomplete="off" spellcheck="false" /></div>`,
        )
        .join("")}
      <div class="field-error" id="err" hidden></div>
      <button class="primary full" id="done">Create the wallet</button>
      <button class="ghost full" id="back">Show the words again</button>
    </div>`,
    { tab: false },
  );
  $("#back").onclick = () => screenCreate(2, data);
  $("#done").onclick = async () => {
    const error = $("#err");
    const wrong = asked.some(
      (i) => $(`#w${i}`).value.trim().toLowerCase() !== words[i],
    );
    if (wrong) {
      error.textContent = "Those words do not match. Check your notes.";
      error.hidden = false;
      return;
    }
    await finishSetup(data.mnemonic, data.password);
  };
}

function screenImport() {
  render(
    `<div class="screen">
      <h1>Restore your wallet</h1>
      <p class="muted">Type your 12 or 24 word recovery phrase. It works with the phrase from the command line wallet too.</p>
      <div><label for="phrase">Recovery phrase</label>
        <textarea id="phrase" autocapitalize="off" autocomplete="off" spellcheck="false" placeholder="word one, word two, …"></textarea>
        <div class="hint">Separate the words with spaces. Order matters.</div></div>
      <div><label for="pw">New password for this device</label><input type="password" id="pw" autocomplete="new-password" /></div>
      <div><label for="pw2">Repeat the password</label><input type="password" id="pw2" autocomplete="new-password" /></div>
      <div class="field-error" id="err" hidden></div>
      <button class="primary full" id="restore">Restore</button>
      <button class="ghost full" id="back">Back</button>
    </div>`,
    { tab: false },
  );
  $("#back").onclick = screenWelcome;
  $("#restore").onclick = async () => {
    const error = $("#err");
    error.hidden = true;
    const phrase = normalizeMnemonic($("#phrase").value);
    if (!isValidMnemonic(phrase)) {
      error.textContent =
        "That phrase is not valid. Check the spelling and the order of the words.";
      error.hidden = false;
      return;
    }
    const password = $("#pw").value;
    if (passwordStrength(password).score === 0) {
      error.textContent = "Use a password of at least 8 characters.";
      error.hidden = false;
      return;
    }
    if (password !== $("#pw2").value) {
      error.textContent = "The two passwords are different.";
      error.hidden = false;
      return;
    }
    await finishSetup(phrase, password);
  };
}

async function finishSetup(mnemonic, password) {
  render(
    `<div class="screen"><p class="muted"><span class="spinner"></span> Encrypting your wallet…</p></div>`,
    {
      tab: false,
    },
  );
  await createVault({
    mnemonic,
    password,
    accounts: [{ index: 0, label: "Account 1" }],
  });
  state.wallet = await UnlockedWallet.open(mnemonic, state.settings.network);
  state.accountIndex = 0;
  touchActivity();
  await refresh();
  toast("Wallet ready");
  screenHome();
}

function screenUnlock(message) {
  state.tab = "home";
  render(
    `<div class="screen">
      <h1>Welcome back</h1>
      ${message ? `<div class="notice">${e(message)}</div>` : ""}
      <div><label for="pw">Password</label><input type="password" id="pw" autocomplete="current-password" autofocus /></div>
      <div class="field-error" id="err" hidden></div>
      <button class="primary full" id="unlock">Unlock</button>
      <button class="ghost full" id="forgot">I lost my password</button>
    </div>`,
    { tab: false },
  );
  const submit = async () => {
    const error = $("#err");
    error.hidden = true;
    const button = $("#unlock");
    button.disabled = true;
    button.innerHTML = '<span class="spinner"></span> Unlocking…';
    try {
      const mnemonic = await openVault($("#pw").value);
      state.wallet = await UnlockedWallet.open(
        mnemonic,
        state.settings.network,
      );
      state.accountIndex = readAccounts()[0]?.index ?? 0;
      touchActivity();
      await refresh();
      if (state.pending) return screenApprove(state.pending);
      screenHome();
    } catch (err) {
      error.textContent = err.message;
      error.hidden = false;
      button.disabled = false;
      button.textContent = "Unlock";
    }
  };
  $("#unlock").onclick = submit;
  $("#pw").onkeydown = (event) => {
    if (event.key === "Enter") submit();
  };
  $("#forgot").onclick = () =>
    sheet(`<h2>Your password cannot be recovered</h2>
      <p class="muted">Nobody holds a copy — not this site, not the network. If you have your 12 or 24 word recovery phrase you can restore the wallet and choose a new password. Without the phrase the coins cannot be reached.</p>
      <button class="secondary full" id="to-import">Restore from the phrase</button>
      <button class="ghost full" id="close">Back</button>`);
}

document.addEventListener("click", (event) => {
  if (event.target.id === "close") closeSheet();
  if (event.target.id === "to-import") {
    closeSheet();
    screenImport();
  }
});

/* ---------------------------------------------------------------- home --- */

function balanceLines() {
  const info = state.account;
  if (!info)
    return `<div class="balance"><div class="amount">—</div><div class="sub">loading…</div></div>`;
  const extras = [];
  if (info.locked > 0n)
    extras.push(
      `${formatTCN(info.locked, { withTicker: false })} locked by a vote`,
    );
  if (info.immature > 0n)
    extras.push(
      `${formatTCN(info.immature, { withTicker: false })} mining rewards maturing`,
    );
  return `<div class="balance">
      <div class="amount">${formatTCN(info.spendable)}</div>
      <div class="sub">spendable${extras.length ? ` · ${extras.map(e).join(" · ")}` : ""}</div>
    </div>`;
}

function accountPill() {
  const account = state.wallet.account(state.accountIndex);
  const label =
    readAccounts().find((a) => a.index === state.accountIndex)?.label ||
    `Account ${state.accountIndex + 1}`;
  return `<button class="account-pill" id="account-switch" title="Switch account">
      <strong>${e(label)}</strong><span class="muted mono">${e(shortAddress(account.address))}</span><span>▾</span>
    </button>`;
}

function txRow(tx, myAddress) {
  const action = tx.action || {};
  const incoming = action.type === "transfer" && action.to === myAddress;
  const amount =
    action.amount !== undefined
      ? formatTCN(action.amount, { withTicker: false })
      : "—";
  const who = incoming
    ? `from ${shortAddress(tx.sender)}`
    : `to ${shortAddress(action.to || "—")}`;
  const pending = tx.in_mempool;
  const failed = tx.success === false;
  return `<button class="tx" data-txid="${e(tx.txid)}">
      <span class="dir">${failed ? "⚠" : incoming ? "↙" : "↗"}</span>
      <span class="body">
        <span class="row between"><strong>${incoming ? "Received" : "Sent"}</strong>
          <span class="value ${incoming ? "in" : ""}">${incoming ? "+" : "−"}${e(amount)}</span></span>
        <span class="row between"><span class="who">${e(who)}</span>
          ${pending ? '<span class="badge warn">pending</span>' : failed ? '<span class="badge bad">failed</span>' : `<span class="badge ok">${e(tx.confirmations)} conf.</span>`}</span>
      </span>
    </button>`;
}

async function screenHome() {
  state.tab = "home";
  const account = state.wallet.account(state.accountIndex);
  render(`<div class="screen">
      <div class="row between">${accountPill()}<button class="icon-btn" id="refresh" title="Refresh">↻</button></div>
      ${balanceLines()}
      <div class="actions">
        <button class="secondary" data-go="send"><span class="glyph">↗</span>Send</button>
        <button class="secondary" data-go="receive"><span class="glyph">↙</span>Receive</button>
        <button class="secondary" id="scan"><span class="glyph">🔗</span>Paste request</button>
      </div>
      <div class="card"><div class="row between"><h2>Activity</h2><span class="muted" id="act-note"></span></div>
        <div id="activity"><p class="muted"><span class="spinner"></span> loading…</p></div>
      </div>
    </div>`);
  $("#refresh").onclick = async () => {
    await refresh();
    screenHome();
  };
  $("#account-switch").onclick = openAccountSheet;
  $("#scan").onclick = openRequestSheet;
  for (const button of view.querySelectorAll("[data-go]")) {
    button.onclick = () => {
      state.tab = button.dataset.go;
      button.dataset.go === "send" ? screenSend() : screenReceive();
    };
  }

  await Promise.all([refresh({ quiet: true }), loadHistory()]);
  const list = $("#activity");
  if (!list) return;
  if (state.historyError) {
    const sent = readSent().slice(0, 10);
    list.innerHTML =
      `<p class="muted">${e(state.historyError)}</p>` +
      (sent.length
        ? `<p class="muted">Payments sent from this browser:</p>` +
          sent
            .map(
              (s) =>
                `<button class="tx" data-txid="${e(s.txid)}"><span class="dir">↗</span><span class="body"><span class="row between"><strong>Sent</strong><span class="value">−${e(formatTCN(BigInt(s.amount), { withTicker: false }))}</span></span><span class="who">to ${e(shortAddress(s.to))}</span></span></button>`,
            )
            .join("")
        : "");
  } else if (!state.history.length) {
    list.innerHTML = `<p class="muted">No payments yet. Receive some TCN to get started.</p>`;
  } else {
    list.innerHTML = state.history
      .map((tx) => txRow(tx, account.address))
      .join("");
  }
  list.onclick = (event) => {
    const txid = event.target.closest("[data-txid]")?.dataset.txid;
    if (txid) openTxSheet(txid);
  };
  const info = document.getElementById("act-note");
  if (info)
    info.textContent =
      state.account?.mempool_txs > 0
        ? `${state.account.mempool_txs} pending`
        : "";
}

function openAccountSheet() {
  const accounts = readAccounts();
  const node = sheet(`<h2>Accounts</h2>
    <div class="stack">${accounts
      .map((a) => {
        const address = state.wallet.account(a.index).address;
        return `<button class="card soft" data-index="${a.index}" style="text-align:left">
            <div class="row between"><strong>${e(a.label)}</strong>${a.index === state.accountIndex ? '<span class="badge ok">current</span>' : ""}</div>
            <div class="mono muted">${e(shortAddress(address))}</div>
          </button>`;
      })
      .join("")}</div>
    <button class="secondary full" id="add-account">Add another account</button>
    <p class="muted">Every account comes from the same recovery phrase, so one backup covers them all.</p>
    <button class="ghost full" id="close">Close</button>`);
  node.querySelector("#add-account").onclick = () => {
    const accounts = readAccounts();
    const index = Math.max(...accounts.map((a) => a.index)) + 1;
    writeAccounts([...accounts, { index, label: `Account ${index + 1}` }]);
    state.accountIndex = index;
    closeSheet();
    screenHome();
  };
  node.onclick = (event) => {
    const index = event.target.closest("[data-index]")?.dataset.index;
    if (index === undefined) return;
    state.accountIndex = Number(index);
    closeSheet();
    screenHome();
  };
}

function openRequestSheet() {
  const node = sheet(`<h2>Paste a payment request</h2>
    <p class="muted">A <span class="mono">thecoin:</span> link or an address. The shop or game gives you one.</p>
    <textarea id="uri" placeholder="thecoin:tct1…?amount=2.5"></textarea>
    <div class="field-error" id="err" hidden></div>
    <button class="primary full" id="use">Continue</button>
    <button class="ghost full" id="close">Cancel</button>`);
  node.querySelector("#use").onclick = () => {
    const error = node.querySelector("#err");
    const text = node.querySelector("#uri").value.trim();
    try {
      const request = /^thecoin:/i.test(text)
        ? parsePaymentUri(text, state.settings.network)
        : { address: text, amount: null, memo: null, label: null };
      decodeAddress(request.address, state.settings.network);
      closeSheet();
      state.draft = {
        to: request.address,
        amount: request.amount ? formatAmount(request.amount) : "",
        memo: request.memo || "",
        label: request.label || "",
      };
      state.tab = "send";
      screenSend();
    } catch (err) {
      error.textContent = err.message;
      error.hidden = false;
    }
  };
}

/* ---------------------------------------------------------------- send --- */

function screenSend() {
  state.tab = "send";
  const draft = state.draft || {};
  const contacts = readContacts();
  render(`<div class="screen">
      <h1>Send TCN</h1>
      ${balanceLines()}
      <div>
        <label for="to">To</label>
        <input type="text" id="to" placeholder="${e(NETWORKS[state.settings.network].hrp)}1…" value="${e(draft.to || "")}" autocapitalize="off" spellcheck="false" />
        ${contacts.length ? `<div class="hint">Contacts: ${contacts.map((c, i) => `<a href="#" data-contact="${i}">${e(c.label)}</a>`).join(" · ")}</div>` : ""}
        <div class="field-error" id="to-err" hidden></div>
      </div>
      <div>
        <label for="amount">Amount</label>
        <div class="row"><input type="text" id="amount" inputmode="decimal" placeholder="0.00" value="${e(draft.amount || "")}" />
          <button class="secondary" id="max" type="button">Max</button></div>
        <div class="field-error" id="amount-err" hidden></div>
      </div>
      <div>
        <label for="memo">Note (optional, public)</label>
        <input type="text" id="memo" maxlength="256" placeholder="Order #123" value="${e(draft.memo || "")}" />
        <div class="hint">Written on the blockchain for everyone to see. Up to 256 bytes.</div>
      </div>
      <div>
        <label for="priority">Priority</label>
        <select id="priority">
          <option value="low">Economy — cheapest</option>
          <option value="normal">Normal — recommended</option>
          <option value="high">Fast</option>
          <option value="urgent">Urgent</option>
        </select>
        <div class="hint" id="fee-hint">Fee is calculated from the network's live parameters.</div>
      </div>
      <button class="primary full" id="review">Review payment</button>
    </div>`);
  $("#priority").value = state.settings.priority || "normal";
  view.querySelectorAll("[data-contact]").forEach((link) => {
    link.onclick = (event) => {
      event.preventDefault();
      $("#to").value = contacts[Number(link.dataset.contact)].address;
    };
  });
  $("#max").onclick = () => {
    if (!state.account || !state.fees) return toast("Waiting for the node…");
    const estimate = minimumFee(feeParams(), 200n) * 2n;
    const max =
      state.account.spendable > estimate
        ? state.account.spendable - estimate
        : 0n;
    $("#amount").value = formatAmount(max);
    toast("A little is kept back for the fee");
  };
  $("#priority").onchange = (event) =>
    writeSettings({ priority: event.target.value });
  $("#review").onclick = reviewPayment;
}

function feeParams() {
  const fees = state.fees || {};
  return {
    base_fee: fees.base_fee ?? 1000n,
    fee_per_kb: fees.fee_per_kb ?? 10000n,
    fee_per_kfuel: fees.fee_per_kfuel ?? 1000n,
    congestion_bp: fees.congestion_bp ?? 10000,
  };
}

function priorityBp(level) {
  const p = state.fees?.priority || {};
  return BigInt(
    {
      low: p.low_bp ?? 10000,
      normal: p.normal_bp ?? 12500,
      high: p.high_bp ?? 20000,
      urgent: p.urgent_bp ?? 40000,
    }[level] ?? 10000,
  );
}

async function reviewPayment() {
  const network = state.settings.network;
  const toField = $("#to");
  const amountField = $("#amount");
  const toError = $("#to-err");
  const amountError = $("#amount-err");
  toError.hidden = amountError.hidden = true;

  let to;
  try {
    to = decodeAddress(toField.value, network);
  } catch (err) {
    toError.textContent = err.message;
    toError.hidden = false;
    return;
  }
  let amount;
  try {
    amount = parseAmount(amountField.value);
    if (amount <= 0n) throw new Error("Type an amount greater than zero.");
  } catch (err) {
    amountError.textContent = err.message;
    amountError.hidden = false;
    return;
  }
  const memo = $("#memo").value.trim();
  if (utf8(memo).length > MAX_MEMO_BYTES) {
    toast("The note is too long.");
    return;
  }
  state.draft = { to: toField.value.trim(), amount: amountField.value, memo };

  await refresh({ quiet: true });
  if (!state.status || !state.account || !state.fees) {
    toast("The node is not answering; cannot send safely.");
    return;
  }
  if (state.status.network !== network) {
    toast(
      `This node is on ${state.status.network}. Change the node in Settings.`,
    );
    return;
  }

  const level = $("#priority").value;
  const account = state.wallet.account(state.accountIndex);
  const expiry = BigInt(state.status.height) + 1440n;
  let signed;
  try {
    signed = buildTransfer({
      account,
      network,
      to,
      amount,
      memo: utf8(memo),
      nonce: BigInt(state.account.next_nonce),
      expiryHeight: expiry,
      feeParams: feeParams(),
      priorityBp: priorityBp(level),
    });
  } catch (err) {
    toast(err.message);
    return;
  }
  const total = amount + signed.fee;
  const enough = total <= state.account.spendable;
  const burning = isBurnAddress(to);

  const node = sheet(`<h2>Confirm this payment</h2>
    ${burning ? '<div class="notice bad">This is the burn address. Coins sent there are destroyed and can never be recovered.</div>' : ""}
    ${!enough ? `<div class="notice bad">Not enough spendable balance: you need ${e(formatTCN(total))} and have ${e(formatTCN(state.account.spendable))}.</div>` : ""}
    <dl class="kv">
      <dt>Amount</dt><dd><strong>${e(formatTCN(amount))}</strong></dd>
      <dt>To</dt><dd class="mono">${e(state.draft.to)}</dd>
      <dt>From</dt><dd class="mono">${e(account.address)}</dd>
      ${memo ? `<dt>Note</dt><dd>${e(memo)}</dd>` : ""}
      <dt>Fee</dt><dd>${e(formatTCN(signed.fee))} <span class="muted">(${e(level)})</span></dd>
      <dt>Total</dt><dd><strong>${e(formatTCN(total))}</strong></dd>
      <dt>Expires</dt><dd>in ${e(blocksToText(1440, network))} if not mined</dd>
    </dl>
    <div class="field-error" id="send-err" hidden></div>
    <button class="primary full" id="confirm" ${enough ? "" : "disabled"}>Send ${e(formatTCN(amount))}</button>
    <button class="ghost full" id="close">Cancel</button>`);

  node.querySelector("#confirm").onclick = async () => {
    const button = node.querySelector("#confirm");
    const error = node.querySelector("#send-err");
    button.disabled = true;
    button.innerHTML = '<span class="spinner"></span> Sending…';
    error.hidden = true;
    try {
      const result = await state.api.submit(signed.hex);
      recordSent({
        txid: result.txid,
        to: state.draft.to,
        amount: amount.toString(),
        fee: signed.fee.toString(),
        at: Date.now(),
        hex: signed.hex,
      });
      state.draft = {};
      closeSheet();
      await refresh({ quiet: true });
      screenSent(result.txid, amount, state.draft.to);
    } catch (err) {
      error.textContent = err.message;
      error.hidden = false;
      button.disabled = false;
      button.textContent = "Try again";
    }
  };
}

function screenSent(txid, amount) {
  state.tab = "home";
  render(`<div class="screen">
      <div class="card" style="text-align:center;align-items:center">
        <div style="font-size:34px">✅</div>
        <h1>Payment sent</h1>
        <p class="muted">${e(formatTCN(amount))} is on its way. It normally appears in the next block, about 15 seconds.</p>
      </div>
      <div class="card"><h2>Receipt</h2><p class="mono">${e(txid)}</p>
        <div class="grid2"><button class="secondary" id="copy-txid">Copy id</button>
        <a class="secondary" style="text-align:center;padding:11px 14px;border:1px solid var(--line);border-radius:10px" href="${e(explorerLink(txid))}" target="_blank" rel="noopener">Open explorer</a></div>
      </div>
      <button class="primary full" id="done">Done</button>
    </div>`);
  $("#copy-txid").onclick = () => copy(txid, "Transaction id copied");
  $("#done").onclick = screenHome;
  pollTransaction(txid);
}

async function pollTransaction(txid) {
  for (let i = 0; i < 10; i++) {
    await new Promise((resolve) => setTimeout(resolve, 4000));
    try {
      const tx = await state.api.transaction(txid);
      if (!tx.in_mempool) {
        toast("Payment confirmed in a block");
        await refresh({ quiet: true });
        return;
      }
    } catch {
      return;
    }
  }
}

/* ------------------------------------------------------------- receive --- */

function screenReceive() {
  state.tab = "receive";
  const account = state.wallet.account(state.accountIndex);
  const uri = buildPaymentUri({ address: account.address });
  render(`<div class="screen">
      <h1>Receive TCN</h1>
      <p class="muted">Show this code or send the address. It never expires and can be reused.</p>
      <div class="qr" id="qr">${qrSvg(uri)}</div>
      <div class="card"><p class="mono" id="addr">${e(account.address)}</p>
        <div class="grid2"><button class="secondary" id="copy-addr">Copy address</button>
          <button class="secondary" id="ask">Ask for an amount</button></div>
      </div>
      <p class="muted">Only The Coin (${e(state.settings.network)}) can be received here. Coins from other networks are lost forever.</p>
    </div>`);
  $("#copy-addr").onclick = () => copy(account.address, "Address copied");
  $("#ask").onclick = () => {
    const node = sheet(`<h2>Request a payment</h2>
      <div><label for="req-amount">Amount</label><input type="text" id="req-amount" inputmode="decimal" placeholder="2.50" /></div>
      <div><label for="req-memo">Note (optional)</label><input type="text" id="req-memo" placeholder="Order #123" /></div>
      <div class="field-error" id="req-err" hidden></div>
      <button class="primary full" id="make">Create the request</button>
      <button class="ghost full" id="close">Cancel</button>`);
    node.querySelector("#make").onclick = () => {
      const error = node.querySelector("#req-err");
      try {
        const amount = parseAmount(node.querySelector("#req-amount").value);
        const memo = node.querySelector("#req-memo").value.trim();
        const requestUri = buildPaymentUri({
          address: account.address,
          amount,
          memo: memo || undefined,
        });
        closeSheet();
        render(`<div class="screen">
            <h1>Payment request</h1>
            <p class="muted">${e(formatTCN(amount))}${memo ? ` · ${e(memo)}` : ""}</p>
            <div class="qr">${qrSvg(requestUri)}</div>
            <div class="card"><p class="mono">${e(requestUri)}</p>
              <button class="secondary full" id="copy-uri">Copy the link</button></div>
            <button class="ghost full" id="back">Back</button>
          </div>`);
        $("#copy-uri").onclick = () => copy(requestUri, "Request copied");
        $("#back").onclick = screenReceive;
      } catch (err) {
        error.textContent = err.message;
        error.hidden = false;
      }
    };
  };
}

/* ------------------------------------------------------------ activity --- */

async function openTxSheet(txid) {
  const node = sheet(
    `<h2>Payment</h2><p class="muted"><span class="spinner"></span> loading…</p>`,
  );
  let tx;
  try {
    tx = await state.api.transaction(txid);
  } catch (err) {
    node.querySelector(".inner").innerHTML =
      `<h2>Payment</h2><div class="notice bad">${e(err.message)}</div><p class="mono">${e(txid)}</p><button class="ghost full" id="close">Close</button>`;
    return;
  }
  const action = tx.action || {};
  const mine = state.wallet.account(state.accountIndex).address;
  const incoming = action.to === mine;
  const security = tx.in_mempool
    ? null
    : await state.api.security(action.amount ?? 0n).catch(() => null);
  node.querySelector(".inner").innerHTML =
    `<h2>${incoming ? "Received" : "Sent"} payment</h2>
    ${tx.conflict ? '<div class="notice bad">Another payment with the same number was seen: this may be a double spend. Wait for confirmations.</div>' : ""}
    ${tx.replaceable && incoming ? '<div class="notice warn">The sender can still replace this payment. Treat it as unconfirmed.</div>' : ""}
    ${tx.success === false ? `<div class="notice bad">The contract call failed: ${e(tx.error || "")}. The fee was still charged.</div>` : ""}
    <dl class="kv">
      <dt>Amount</dt><dd><strong>${e(action.amount !== undefined ? formatTCN(action.amount) : "—")}</strong></dd>
      <dt>${incoming ? "From" : "To"}</dt><dd class="mono">${e(incoming ? tx.sender : action.to || "—")}</dd>
      ${action.memo_text ? `<dt>Note</dt><dd>${e(action.memo_text)}</dd>` : ""}
      <dt>Status</dt><dd>${tx.in_mempool ? "waiting in the mempool" : `${e(tx.confirmations)} confirmation(s)`}</dd>
      ${tx.block_height !== undefined && tx.block_height !== null ? `<dt>Block</dt><dd>${e(tx.block_height)}</dd>` : ""}
      <dt>Fee</dt><dd>${e(formatTCN(tx.fee ?? 0n))}</dd>
      <dt>Id</dt><dd class="mono">${e(tx.txid)}</dd>
    </dl>
    ${security?.explanation ? `<p class="muted">${e(security.explanation)}</p>` : ""}
    <a class="secondary full" style="text-align:center;display:block;padding:11px;border:1px solid var(--line);border-radius:10px" href="${e(explorerLink(tx.txid))}" target="_blank" rel="noopener">See it in the explorer</a>
    <button class="ghost full" id="close">Close</button>`;
}

/* ------------------------------------------------------------ settings --- */

function screenSettings() {
  state.tab = "settings";
  const settings = state.settings;
  render(`<div class="screen">
      <h1>Settings</h1>

      <div class="card"><h2>Network</h2>
        <div><label for="node">Node</label>
          <select id="node">
            ${DEFAULT_NODES.map((n) => `<option value="${e(n.url)}" data-network="${e(n.network)}" ${n.url === settings.nodeUrl ? "selected" : ""}>${e(n.label)}</option>`).join("")}
            <option value="custom" ${DEFAULT_NODES.every((n) => n.url !== settings.nodeUrl) ? "selected" : ""}>Another node…</option>
          </select></div>
        <div id="custom-wrap" ${DEFAULT_NODES.every((n) => n.url !== settings.nodeUrl) ? "" : "hidden"}>
          <label for="custom-url">Node address</label>
          <input type="text" id="custom-url" value="${e(settings.nodeUrl)}" placeholder="https://…" />
          <div class="hint">The node must allow this site (CORS) and be on the same network.</div>
        </div>
        <p class="muted">Current: ${e(settings.network)} · ${e(settings.nodeUrl)}</p>
        <button class="secondary full" id="save-node">Save and reconnect</button>
      </div>

      <div class="card"><h2>Contacts</h2>
        <div id="contacts" class="stack"></div>
        <button class="secondary full" id="add-contact">Add a contact</button>
      </div>

      <div class="card"><h2>Security</h2>
        <div><label for="autolock">Lock after</label>
          <select id="autolock">
            ${[1, 5, 15, 60, 0].map((m) => `<option value="${m}" ${Number(settings.autoLockMinutes) === m ? "selected" : ""}>${m === 0 ? "Never" : `${m} minute${m > 1 ? "s" : ""} of inactivity`}</option>`).join("")}
          </select></div>
        <button class="secondary full" id="show-phrase">Show my recovery phrase</button>
        <button class="secondary full" id="change-pw">Change the password</button>
      </div>

      <div class="card"><h2>This device</h2>
        <p class="muted">The wallet lives in this browser only. Clearing site data removes it — your recovery phrase is the only backup.</p>
        <button class="danger full" id="forget">Remove this wallet from the browser</button>
      </div>

      <p class="muted">The Coin wallet · <a href="https://the-coin.cloud" target="_blank" rel="noopener">the-coin.cloud</a></p>
    </div>`);

  const nodeSelect = $("#node");
  nodeSelect.onchange = () => {
    $("#custom-wrap").hidden = nodeSelect.value !== "custom";
  };
  $("#save-node").onclick = async () => {
    const chosen = nodeSelect.value;
    const option = nodeSelect.selectedOptions[0];
    const url = chosen === "custom" ? $("#custom-url").value.trim() : chosen;
    if (!/^https?:\/\//.test(url))
      return toast("The node address must start with https://");
    const network =
      chosen === "custom" ? await detectNetwork(url) : option.dataset.network;
    if (!network) return;
    writeSettings({ nodeUrl: url, network });
    state.settings = readSettings();
    state.api.setBaseUrl(url);
    state.wallet.setNetwork(network);
    await refresh();
    toast(`Connected to ${network}`);
    screenSettings();
  };
  $("#autolock").onchange = (event) => {
    writeSettings({ autoLockMinutes: Number(event.target.value) });
    state.settings = readSettings();
    touchActivity();
  };
  $("#show-phrase").onclick = revealPhrase;
  $("#change-pw").onclick = changePasswordSheet;
  $("#forget").onclick = forgetSheet;
  $("#add-contact").onclick = () => contactSheet();
  renderContacts();
}

function renderContacts() {
  const list = $("#contacts");
  const contacts = readContacts();
  if (!list) return;
  list.innerHTML = contacts.length
    ? contacts
        .map(
          (c, i) =>
            `<div class="row between"><span><strong>${e(c.label)}</strong><br><span class="mono muted">${e(shortAddress(c.address))}</span></span><button class="ghost" data-remove="${i}">Remove</button></div>`,
        )
        .join("")
    : `<p class="muted">No contacts yet. Save the addresses you pay often.</p>`;
  list.onclick = (event) => {
    const index = event.target.dataset.remove;
    if (index === undefined) return;
    const contacts = readContacts();
    contacts.splice(Number(index), 1);
    writeContacts(contacts);
    renderContacts();
  };
}

function contactSheet() {
  const node = sheet(`<h2>New contact</h2>
    <div><label for="c-label">Name</label><input type="text" id="c-label" placeholder="Maria" /></div>
    <div><label for="c-addr">Address</label><input type="text" id="c-addr" placeholder="${e(NETWORKS[state.settings.network].hrp)}1…" autocapitalize="off" spellcheck="false" /></div>
    <div class="field-error" id="c-err" hidden></div>
    <button class="primary full" id="save">Save</button>
    <button class="ghost full" id="close">Cancel</button>`);
  node.querySelector("#save").onclick = () => {
    const label = node.querySelector("#c-label").value.trim();
    const address = node.querySelector("#c-addr").value.trim();
    const error = node.querySelector("#c-err");
    try {
      decodeAddress(address, state.settings.network);
      if (!label) throw new Error("Give the contact a name.");
      writeContacts([...readContacts(), { label, address }]);
      closeSheet();
      renderContacts();
    } catch (err) {
      error.textContent = err.message;
      error.hidden = false;
    }
  };
}

function revealPhrase() {
  const node = sheet(`<h2>Show the recovery phrase</h2>
    <div class="notice warn">Make sure nobody can see your screen. Anyone with these words can spend your coins.</div>
    <div><label for="rp">Password</label><input type="password" id="rp" autocomplete="current-password" /></div>
    <div class="field-error" id="rp-err" hidden></div>
    <button class="primary full" id="reveal">Show it</button>
    <button class="ghost full" id="close">Cancel</button>`);
  node.querySelector("#reveal").onclick = async () => {
    const error = node.querySelector("#rp-err");
    try {
      const mnemonic = await openVault(node.querySelector("#rp").value);
      const words = mnemonic.split(" ");
      node.querySelector(".inner").innerHTML = `<h2>Your recovery phrase</h2>
        <div class="words">${words.map((w, i) => `<span class="word"><b>${i + 1}</b>${e(w)}</span>`).join("")}</div>
        <p class="muted">Write it on paper. It restores this wallet in any The Coin wallet, including the command line one.</p>
        <button class="ghost full" id="close">Done</button>`;
    } catch (err) {
      error.textContent = err.message;
      error.hidden = false;
    }
  };
}

function changePasswordSheet() {
  const node = sheet(`<h2>Change the password</h2>
    <div><label for="old">Current password</label><input type="password" id="old" /></div>
    <div><label for="new1">New password</label><input type="password" id="new1" /></div>
    <div><label for="new2">Repeat the new password</label><input type="password" id="new2" /></div>
    <div class="field-error" id="cp-err" hidden></div>
    <button class="primary full" id="save">Change it</button>
    <button class="ghost full" id="close">Cancel</button>`);
  node.querySelector("#save").onclick = async () => {
    const error = node.querySelector("#cp-err");
    const next = node.querySelector("#new1").value;
    error.hidden = true;
    if (passwordStrength(next).score === 0) {
      error.textContent = "Use at least 8 characters.";
      error.hidden = false;
      return;
    }
    if (next !== node.querySelector("#new2").value) {
      error.textContent = "The two passwords are different.";
      error.hidden = false;
      return;
    }
    try {
      await changePassword(node.querySelector("#old").value, next);
      closeSheet();
      toast("Password changed");
    } catch (err) {
      error.textContent = err.message;
      error.hidden = false;
    }
  };
}

function forgetSheet() {
  const node = sheet(`<h2>Remove this wallet?</h2>
    <div class="notice bad">This browser will forget the wallet. Without your recovery phrase the coins are gone for good.</div>
    <p class="muted">Type <strong>REMOVE</strong> to confirm.</p>
    <input type="text" id="confirm-text" autocapitalize="characters" />
    <button class="danger full" id="really">Remove the wallet</button>
    <button class="ghost full" id="close">Cancel</button>`);
  node.querySelector("#really").onclick = () => {
    if (
      node.querySelector("#confirm-text").value.trim().toUpperCase() !==
      "REMOVE"
    )
      return toast("Type REMOVE to confirm");
    forgetWallet();
    state.wallet?.lock();
    state.wallet = null;
    closeSheet();
    screenWelcome();
  };
}

async function detectNetwork(url) {
  const probe = new NodeApi(url);
  try {
    const status = await probe.status();
    if (!NETWORKS[status.network]) throw new Error("unknown network");
    return status.network;
  } catch (err) {
    toast(`Could not reach that node: ${err.message}`);
    return null;
  }
}

/* -------------------------------------------- requests from other sites --- */

/**
 * A site (a game, a shop) opens this wallet in a popup and asks for the user's
 * address or for a payment. The site's identity is the origin of the message
 * event — never anything the page itself claims.
 */
window.addEventListener("message", (event) => {
  const data = event.data;
  if (!data || data.thecoin !== 1 || !event.source) return;
  const request = {
    origin: event.origin,
    source: event.source,
    id: data.id,
    type: data.type,
    payload: data.payload || {},
  };
  if (!["connect", "pay"].includes(request.type)) return;
  state.pending = request;
  if (state.wallet) screenApprove(request);
  else if (walletExists())
    screenUnlock("A site is asking for your approval. Unlock to continue.");
  else screenWelcome();
});

function replyToSite(request, message) {
  try {
    request.source.postMessage(
      { thecoin: 1, id: request.id, ...message },
      request.origin,
    );
  } catch {
    /* the site closed */
  }
}

function screenApprove(request) {
  state.pending = null;
  const account = state.wallet.account(state.accountIndex);
  const site = e(request.origin.replace(/^https?:\/\//, ""));

  if (request.type === "connect") {
    render(
      `<div class="screen">
        <h1>Connect to ${site}?</h1>
        <div class="card"><p class="muted">This site will see:</p>
          <ul class="muted"><li>your address <span class="mono">${e(shortAddress(account.address))}</span></li><li>your balance</li></ul>
          <p class="muted">It can ask you to approve payments, but it can never move your coins on its own and never sees your recovery phrase.</p>
        </div>
        <button class="primary full" id="approve">Connect</button>
        <button class="ghost full" id="reject">Reject</button>
      </div>`,
      { tab: false },
    );
    $("#approve").onclick = () => {
      replyToSite(request, {
        ok: true,
        address: account.address,
        network: state.settings.network,
      });
      screenHome();
      toast(`Connected to ${site}`);
    };
    $("#reject").onclick = () => {
      replyToSite(request, { ok: false, error: "rejected" });
      screenHome();
    };
    return;
  }

  // payment request
  let to, amount, memo;
  try {
    const payload = request.payload;
    // `amount` from a site is a decimal TCN string, like "2.5" (see connect.js).
    const parsed = payload.uri
      ? parsePaymentUri(payload.uri, state.settings.network)
      : {
          address: payload.to,
          amount: payload.amount ? parseAmount(payload.amount) : null,
          memo: payload.memo,
        };
    to = decodeAddress(parsed.address, state.settings.network);
    amount = parsed.amount;
    memo = parsed.memo || "";
    if (!amount || amount <= 0n)
      throw new Error("the site did not ask for a valid amount");
  } catch (err) {
    replyToSite(request, { ok: false, error: String(err.message) });
    toast(`That site sent an invalid request: ${err.message}`);
    screenHome();
    return;
  }

  refresh({ quiet: true }).then(() => {
    const feeEstimate = minimumFee(feeParams(), 200n);
    render(
      `<div class="screen">
        <h1>${site} asks for a payment</h1>
        <div class="balance"><div class="amount">${e(formatTCN(amount))}</div><div class="sub">approximate fee ${e(formatTCN(feeEstimate))}</div></div>
        <dl class="kv">
          <dt>To</dt><dd class="mono">${e(encodeAddress(to, state.settings.network))}</dd>
          ${memo ? `<dt>Note</dt><dd>${e(memo)}</dd>` : ""}
          <dt>From</dt><dd class="mono">${e(account.address)}</dd>
          <dt>Balance</dt><dd>${e(formatTCN(state.account?.spendable ?? 0n))}</dd>
        </dl>
        <div class="notice">Check the address: a site can ask for anything. Approving sends the coins immediately.</div>
        <div class="field-error" id="err" hidden></div>
        <button class="primary full" id="pay">Approve and send</button>
        <button class="ghost full" id="reject">Reject</button>
      </div>`,
      { tab: false },
    );
    $("#reject").onclick = () => {
      replyToSite(request, { ok: false, error: "rejected" });
      screenHome();
    };
    $("#pay").onclick = async () => {
      const button = $("#pay");
      const error = $("#err");
      button.disabled = true;
      button.innerHTML = '<span class="spinner"></span> Sending…';
      try {
        const signed = buildTransfer({
          account,
          network: state.settings.network,
          to,
          amount,
          memo: utf8(memo),
          nonce: BigInt(state.account.next_nonce),
          expiryHeight: BigInt(state.status.height) + 1440n,
          feeParams: feeParams(),
          priorityBp: priorityBp(state.settings.priority || "normal"),
        });
        const result = await state.api.submit(signed.hex);
        recordSent({
          txid: result.txid,
          to: encodeAddress(to, state.settings.network),
          amount: amount.toString(),
          fee: signed.fee.toString(),
          at: Date.now(),
          hex: signed.hex,
        });
        replyToSite(request, { ok: true, txid: result.txid });
        await refresh({ quiet: true });
        screenSent(result.txid, amount);
      } catch (err) {
        error.textContent = err.message;
        error.hidden = false;
        button.disabled = false;
        button.textContent = "Try again";
      }
    };
  });
}

/* ----------------------------------------------------------------- boot --- */

async function boot() {
  if (!window.isSecureContext) {
    render(`<div class="notice bad">This wallet only runs over HTTPS.</div>`, {
      tab: false,
    });
    return;
  }
  if (walletExists()) screenUnlock();
  else screenWelcome();
  // Tell the opener we are ready to receive a request (no data, just a ping).
  if (window.opener)
    window.opener.postMessage({ thecoin: 1, type: "ready" }, "*");
}

boot();
