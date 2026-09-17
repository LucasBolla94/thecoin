/* The Coin block explorer (hash-routed single page). */
(function () {
  "use strict";

  var view = document.getElementById("view");
  var e = TC.esc;
  var refreshTimer = null;
  var routeToken = 0;
  var NET = window.THECOIN_NETWORK_INFO || { label: "Mainnet", node: "mainnet", hrp: "tc", launched: true };
  var HRP = NET.hrp || "tc";
  /* Governance pages exist on the-coin.cloud for the mainnet only. */
  var GOVERNANCE_URL = NET.node === "mainnet" ? "https://the-coin.cloud/governance.html" : null;

  /* ---------------- link helpers ---------------- */
  function blockLink(id, text) {
    return (
      '<a class="mono" href="#/block/' +
      e(id) +
      '">' +
      e(text !== undefined ? text : id) +
      "</a>"
    );
  }
  function txLink(id, short) {
    return (
      '<a class="mono" href="#/tx/' +
      e(id) +
      '">' +
      e(short === false ? id : TC.shortHash(id)) +
      "</a>"
    );
  }
  function addrLink(a, short) {
    return a
      ? '<a class="mono" href="#/address/' +
          e(a) +
          '" title="' +
          e(a) +
          '">' +
          e(short === false ? a : TC.shortAddr(a)) +
          "</a>"
      : "—";
  }
  function contractLink(id, short) {
    return (
      '<a class="mono" href="#/contract/' +
      e(id) +
      '">' +
      e(short === false ? id : TC.shortHash(id)) +
      "</a>"
    );
  }
  function proposalLink(id, short) {
    var text = e(short === false ? id : TC.shortHash(id));
    if (!GOVERNANCE_URL) return '<span class="mono" title="' + e(id) + '">' + text + "</span>";
    return '<a class="mono" href="' + GOVERNANCE_URL + "#/proposal/" + e(id) + '">' + text + "</a>";
  }
  function copyBtn(text) {
    return (
      ' <button class="btn btn-small" type="button" data-copy="' +
      e(text) +
      '">Copy</button>'
    );
  }
  function amount(m) {
    return '<span class="nowrap">' + e(TC.fmtTCN(m)) + "</span>";
  }
  function mono(s) {
    return '<span class="mono break">' + e(s) + "</span>";
  }
  function faint(s) {
    return '<span class="faint">' + e(s) + "</span>";
  }
  function kv(rows) {
    return (
      '<dl class="kv">' +
      rows
        .filter(Boolean)
        .map(function (r) {
          return "<dt>" + r[0] + "</dt><dd>" + r[1] + "</dd>";
        })
        .join("") +
      "</dl>"
    );
  }
  function memo(hexStr, text) {
    if (!hexStr) return faint("—");
    if (!text) text = hexToUtf8(hexStr);
    if (text) return "“" + e(text) + "”";
    return '<span class="mono break">0x' + e(hexStr) + "</span>";
  }
  function hexToUtf8(hexStr) {
    if (!/^([0-9a-f]{2})+$/i.test(hexStr) || typeof TextDecoder === "undefined")
      return null;
    var bytes = new Uint8Array(hexStr.length / 2);
    for (var i = 0; i < bytes.length; i++)
      bytes[i] = parseInt(hexStr.substr(i * 2, 2), 16);
    try {
      return new TextDecoder("utf-8", { fatal: true }).decode(bytes);
    } catch (err) {
      return null;
    }
  }
  function showError(err, what) {
    var status = err && err.status;
    var answered = status >= 400 && status < 500;
    var msg = answered
      ? "<strong>" +
        (status === 404 ? "Not found" : "Invalid request") +
        "</strong><p>" +
        e(what || err.message) +
        "</p>"
      : "<strong>Live data is temporarily unavailable.</strong><p>We cannot reach a network node right now. You can still browse the guides and learn how The Coin works.</p>";
    view.innerHTML =
      '<div class="notice error">' +
      msg +
      '</div><div class="actions"><button type="button" class="btn btn-small" id="retry-network">Try again ↻</button><a class="text-link" href="#/">Back to blocks</a></div>';
    document.getElementById("retry-network").addEventListener("click", route);
  }

  /** Name declared in a TCCL source ("contract Name"), or null. */
  function contractName(source) {
    var m = /^[ \t]*contract[ \t]+([A-Za-z_][A-Za-z0-9_]*)/m.exec(
      String(source || ""),
    );
    return m ? m[1] : null;
  }

  function has(v) {
    return v !== null && v !== undefined;
  }

  /* ---------------- finality ---------------- */

  /** Target seconds between blocks (crates/core/src/params.rs). */
  var BLOCK_SECONDS = 15;

  var FINAL_TITLE =
    "Two thirds of the recent miners signed this block: it can never be reversed";
  var PENDING_TITLE =
    "Waiting for the signatures of two thirds of the recent miners — usually one block, about 30 seconds";

  /** Badge showing whether a block (and everything in it) is irreversible. */
  function finalBadge(isFinal) {
    return isFinal
      ? '<span class="badge ok" title="' + e(FINAL_TITLE) + '">final</span>'
      : '<span class="badge warn" title="' +
          e(PENDING_TITLE) +
          '">not final yet</span>';
  }

  /** Finality of a height against /status, when the node reports one. */
  function finalizedAt(height, finalizedHeight) {
    return (
      has(height) &&
      Number(finalizedHeight) > 0 &&
      Number(height) <= Number(finalizedHeight)
    );
  }

  function finalityNote(isFinal) {
    return isFinal
      ? ' <span class="faint">signed by two thirds of the recent miners — irreversible</span>'
      : ' <span class="faint">protected by proof of work; blocks normally become final one block later, about 30 seconds</span>';
  }

  /** Reads /status without breaking a page when the endpoint is unavailable. */
  function statusOrNull() {
    return TC.api("/status").catch(function () {
      return null;
    });
  }


  /* ---------------- action rendering ---------------- */
  var CALL_LABELS = {
    escrow_release: "Release escrow to payee",
    escrow_refund: "Refund escrow to payer",
    vesting_claim: "Claim vested funds",
    vesting_revoke: "Revoke vesting",
    subscription_claim: "Claim subscription periods",
    subscription_cancel: "Cancel subscription",
    htlc_redeem: "Redeem HTLC with secret",
    htlc_refund: "Refund expired HTLC",
    multisig_deposit: "Deposit to multisig vault",
    multisig_propose: "Propose multisig spend",
    multisig_approve: "Approve multisig spend",
    multisig_cancel: "Cancel multisig spend",
    multisig_close: "Close multisig vault (refund deposit)",
  };
  var KIND_LABELS = {
    escrow: "Escrow",
    vesting: "Vesting",
    subscription: "Subscription",
    htlc: "HTLC",
    multisig: "Multisig",
  };
  var CHOICE_LABELS = { yes: "Yes", no: "No", abstain: "Abstain" };
  var FN_KIND_LABELS = {
    init: "init",
    action: "action",
    view: "read-only view",
  };

  function actionLabel(a) {
    switch (a.type) {
      case "transfer":
        return "Transfer";
      case "batch_transfer":
        return "Batch payment";
      case "create_contract":
        return "Create contract · " + (KIND_LABELS[a.spec.kind] || a.spec.kind);
      case "call_contract":
        return "Contract · " + (CALL_LABELS[a.call.call] || a.call.call);
      case "propose":
        return "Governance proposal";
      case "vote":
        return "Vote · " + (CHOICE_LABELS[a.choice] || a.choice);
      case "deploy":
        return "Deploy TCCL contract";
      case "invoke":
        return "Invoke TCCL contract";
      case "upgrade":
        return "Upgrade TCCL contract";
      case "set_upgrade_authority":
        return a.new_authority
          ? "Hand over the upgrade authority"
          : "Make the contract final";
      default:
        return a.type;
    }
  }

  /** Short "value" column for tables. */
  function actionValue(a) {
    switch (a.type) {
      case "transfer":
        return (
          amount(a.amount) + ' <span class="faint">→</span> ' + addrLink(a.to)
        );
      case "batch_transfer":
        return (
          amount(a.total) +
          ' <span class="faint">→ ' +
          a.outputs.length +
          " recipients</span>"
        );
      case "create_contract": {
        var s = a.spec;
        var v =
          s.amount !== undefined
            ? s.amount
            : s.kind === "subscription"
              ? String(BigInt(s.amount_per_period) * BigInt(s.max_periods))
              : s.initial_deposit;
        return amount(v);
      }
      case "call_contract":
        return a.call.amount !== undefined
          ? amount(a.call.amount)
          : contractLink(a.contract);
      case "propose":
        return e(a.title);
      case "vote":
        return (
          amount(a.weight) +
          ' <span class="faint">in</span> ' +
          proposalLink(a.proposal)
        );
      case "deploy": {
        var name = contractName(a.source);
        return (
          '<span class="mono">' +
          e(name || "contract") +
          "</span>" +
          (Number(a.value) > 0 ? " · " + amount(a.value) : "")
        );
      }
      case "invoke":
        return (
          '<span class="mono">' +
          e(a.function) +
          "()</span>" +
          (Number(a.value) > 0 ? " · " + amount(a.value) : "") +
          ' <span class="faint">in</span> ' +
          addrLink(a.contract)
        );
      case "upgrade": {
        var upName = contractName(a.source);
        return (
          '<span class="mono">' +
          e(upName || "new code") +
          "</span>" +
          ' <span class="faint">in</span> ' +
          addrLink(a.contract)
        );
      }
      case "set_upgrade_authority":
        return (
          (a.new_authority
            ? '<span class="faint">to</span> ' + addrLink(a.new_authority)
            : '<span class="badge ok">final</span>') +
          ' <span class="faint">in</span> ' +
          addrLink(a.contract)
        );
      default:
        return "";
    }
  }

  function specDetails(s) {
    switch (s.kind) {
      case "escrow":
        return kv([
          ["Payee", addrLink(s.payee, false)],
          ["Arbiter", s.arbiter ? addrLink(s.arbiter, false) : faint("none")],
          ["Amount", amount(s.amount)],
          [
            "Deadline (payer refund)",
            "after block " +
              blockLink(s.deadline_height, TC.fmtInt(s.deadline_height)),
          ],
        ]);
      case "vesting":
        return kv([
          ["Beneficiary", addrLink(s.beneficiary, false)],
          ["Total amount", amount(s.amount)],
          [
            "Start / cliff / end",
            TC.fmtInt(s.start_height) +
              " / " +
              TC.fmtInt(s.cliff_height) +
              " / " +
              TC.fmtInt(s.end_height),
          ],
          ["Revocable", s.revocable ? "yes" : "no"],
        ]);
      case "subscription":
        return kv([
          ["Payee", addrLink(s.payee, false)],
          ["Amount per period", amount(s.amount_per_period)],
          [
            "Period",
            TC.fmtInt(s.period_blocks) +
              " blocks ≈ " +
              TC.fmtDuration(s.period_blocks * BLOCK_SECONDS),
          ],
          [
            "Periods",
            TC.fmtInt(s.max_periods) +
              " (total " +
              TC.fmtTCN(
                String(BigInt(s.amount_per_period) * BigInt(s.max_periods)),
              ) +
              ")",
          ],
        ]);
      case "htlc":
        return kv([
          ["Recipient", addrLink(s.recipient, false)],
          ["Amount", amount(s.amount)],
          ["Hash lock (SHA-256)", mono(s.hash_lock)],
          ["Expires after block", TC.fmtInt(s.timeout_height)],
        ]);
      case "multisig":
        return kv([
          [
            "Signers",
            s.signers
              .map(function (x) {
                return addrLink(x, false);
              })
              .join("<br>"),
          ],
          ["Threshold", e(s.threshold) + " of " + s.signers.length],
          ["Initial deposit", amount(s.initial_deposit)],
        ]);
      default:
        return "<pre><code>" + e(JSON.stringify(s, null, 2)) + "</code></pre>";
    }
  }

  function callDetails(c) {
    var rows = [["Call", e(CALL_LABELS[c.call] || c.call)]];
    if (c.preimage_hex !== undefined)
      rows.push(["Secret (preimage)", mono(c.preimage_hex)]);
    if (c.amount !== undefined) rows.push(["Amount", amount(c.amount)]);
    if (c.to !== undefined) rows.push(["To", addrLink(c.to, false)]);
    if (c.memo_hex !== undefined)
      rows.push(["Memo", memo(c.memo_hex, c.memo_text)]);
    if (c.spend_id !== undefined) rows.push(["Spend #", e(c.spend_id)]);
    return kv(rows);
  }

  function proposalActionText(pa) {
    switch (pa.type) {
      case "text":
        return "Text proposal (on-chain decision)";
      case "set_param":
        return (
          "Change parameter <code>" +
          e(pa.param) +
          "</code> to <strong>" +
          TC.fmtInt(pa.value) +
          "</strong>"
        );
      case "software_upgrade":
        return (
          "Upgrade software to version <strong>" +
          e(pa.version) +
          '</strong><br><span class="mono break faint">' +
          e(pa.release_hash) +
          "</span>"
        );
      default:
        return e(JSON.stringify(pa));
    }
  }

  function argList(args) {
    if (!args || !args.length) return faint("none");
    return (
      '<ol class="arglist">' +
      args
        .map(function (x) {
          return '<li><span class="mono break">' + e(x) + "</span></li>";
        })
        .join("") +
      "</ol>"
    );
  }

  function actionDetails(t) {
    var a = t.action;
    switch (a.type) {
      case "transfer":
        return kv([
          ["To", addrLink(a.to, false)],
          ["Amount", amount(a.amount)],
          ["Memo", memo(a.memo_hex, a.memo_text)],
        ]);
      case "batch_transfer":
        return (
          kv([
            ["Total", amount(a.total)],
            ["Memo", memo(a.memo_hex, a.memo_text)],
          ]) +
          '<div class="table-wrap" tabindex="0" role="region" aria-label="Data table" style="margin-top:12px"><table><thead><tr><th>#</th><th>Recipient</th><th class="num">Amount</th></tr></thead><tbody>' +
          a.outputs
            .map(function (o, i) {
              return (
                "<tr><td>" +
                (i + 1) +
                "</td><td>" +
                addrLink(o.to, false) +
                '</td><td class="num">' +
                amount(o.amount) +
                "</td></tr>"
              );
            })
            .join("") +
          "</tbody></table></div>"
        );
      case "create_contract":
        return (
          "<h3>" +
          e(KIND_LABELS[a.spec.kind] || a.spec.kind) +
          "</h3>" +
          specDetails(a.spec)
        );
      case "call_contract":
        return (
          kv([["Contract", contractLink(a.contract, false)]]) +
          callDetails(a.call)
        );
      case "propose":
        return kv([
          ["Title", e(a.title)],
          [
            "Full text",
            a.url && safeUrl(a.url) !== "#"
              ? '<a href="' +
                e(a.url) +
                '" rel="noopener nofollow" target="_blank">' +
                e(a.url) +
                "</a>"
              : a.url
                ? '<span class="mono break">' + e(a.url) + "</span>"
                : faint("—"),
          ],
          ["Content hash", mono(a.content_hash)],
          ["Effect", proposalActionText(a.action)],
        ]);
      case "vote":
        return kv([
          ["Proposal", proposalLink(a.proposal, false)],
          ["Choice", e(CHOICE_LABELS[a.choice] || a.choice)],
          ["Weight (locked coins)", amount(a.weight)],
        ]);
      case "deploy": {
        var name = contractName(a.source);
        var where = t.program
          ? addrLink(t.program, false) +
            (t.in_mempool
              ? ' <span class="faint">(expected address)</span>'
              : "")
          : t.block_height !== null && t.block_height !== undefined
            ? faint("none — deployment failed")
            : faint("—");
        return (
          kv([
            ["Contract", '<span class="mono">' + e(name || "—") + "</span>"],
            ["Contract address", where],
            ["Source code", TC.fmtInt(TC.byteLength(a.source)) + " bytes"],
            ["Source code hash", mono(a.source_hash)],
            ["init arguments", argList(a.init_args)],
            ["Amount sent", amount(a.value)],
            ["Maximum fuel", TC.fmtInt(a.max_fuel)],
            ["Maximum accepted deposit", amount(a.max_deposit)],
          ]) +
          '<details class="source"><summary>View TCCL source code</summary><pre><code>' +
          e(a.source) +
          "</code></pre></details>"
        );
      }
      case "invoke":
        return kv([
          ["Contract", addrLink(a.contract, false)],
          ["Function", '<span class="mono">' + e(a.function) + "</span>"],
          ["Arguments", argList(a.args)],
          ["Amount sent", amount(a.value)],
          ["Maximum fuel", TC.fmtInt(a.max_fuel)],
          ["Maximum accepted deposit", amount(a.max_deposit)],
        ]);
      case "upgrade": {
        var upName = contractName(a.source);
        return (
          kv([
            ["Contract", addrLink(a.contract, false)],
            [
              "New contract name",
              '<span class="mono">' + e(upName || "—") + "</span>",
            ],
            ["New source code", TC.fmtInt(TC.byteLength(a.source)) + " bytes"],
            ["New source code hash", mono(a.source_hash)],
            [
              "Replaces the code with hash",
              mono(a.expected_code_hash) +
                ' <span class="faint">the upgrade is refused if the code changed in the meantime</span>',
            ],
            ["upgrade() arguments", argList(a.args)],
            ["Maximum fuel", TC.fmtInt(a.max_fuel)],
            ["Maximum accepted deposit", amount(a.max_deposit)],
          ]) +
          '<p class="faint">Only the contract\u2019s upgrade authority may replace its code, and the new code must keep the existing state readable.</p>' +
          '<details class="source"><summary>View the new TCCL source code</summary><pre><code>' +
          e(a.source) +
          "</code></pre></details>"
        );
      }
      case "set_upgrade_authority":
        return (
          kv([
            ["Contract", addrLink(a.contract, false)],
            [
              "New upgrade authority",
              a.new_authority
                ? addrLink(a.new_authority, false)
                : '<span class="badge ok">final</span> <span class="faint">nobody can ever change this code again</span>',
            ],
            ["Applies to the code with hash", mono(a.expected_code_hash)],
          ]) +
          (a.new_authority
            ? ""
            : '<p class="faint">Giving up the authority is irreversible: the contract behaves exactly like its published source code for ever.</p>')
        );
      default:
        return "<pre><code>" + e(JSON.stringify(a, null, 2)) + "</code></pre>";
    }
  }

  function safeUrl(u) {
    return /^https?:\/\//i.test(u) ? u : "#";
  }

  function isProgramTx(t) {
    return (
      t.action.type === "deploy" ||
      t.action.type === "invoke" ||
      t.action.type === "upgrade"
    );
  }

  /** Small badges next to the type: failure, replaceable, double spend. */
  function txBadges(t) {
    var out = "";
    if (t.success === false) out += ' <span class="badge bad">failed</span>';
    if (t.replaceable)
      out +=
        ' <span class="badge warn" title="The sender can replace this transaction with a higher-fee transaction before confirmation">replaceable</span>';
    if (t.conflict)
      out +=
        ' <span class="badge bad" title="Another transaction with the same sender and nonce was observed">double spend?</span>';
    return out;
  }

  function txRows(txs, opts) {
    opts = opts || {};
    if (!txs.length) return '<p class="faint">No transactions yet.</p>';
    return (
      '<div class="table-wrap" tabindex="0" role="region" aria-label="Data table"><table><thead><tr>' +
      "<th>Txid</th>" +
      (opts.showBlock ? "<th>Block</th>" : "") +
      '<th>Type</th><th class="hide-sm">From</th><th>Amount / details</th><th class="num hide-sm">Fee</th></tr></thead><tbody>' +
      txs
        .map(function (t) {
          var where = t.in_mempool
            ? '<span class="badge warn">mempool</span>'
            : has(t.block_height)
              ? blockLink(t.block_height, TC.fmtInt(t.block_height))
              : "—";
          return (
            "<tr><td>" +
            txLink(t.txid) +
            "</td>" +
            (opts.showBlock ? "<td>" + where + "</td>" : "") +
            "<td>" +
            e(actionLabel(t.action)) +
            txBadges(t) +
            '</td><td class="hide-sm">' +
            addrLink(t.sender) +
            "</td><td>" +
            actionValue(t.action) +
            '</td><td class="num hide-sm">' +
            e(TC.fmtTCN(t.fee)) +
            "</td></tr>"
          );
        })
        .join("") +
      "</tbody></table></div>"
    );
  }

  function logsTable(logs) {
    if (!logs || !logs.length) return '<p class="faint">No events emitted.</p>';
    return (
      '<div class="table-wrap" tabindex="0" role="region" aria-label="Data table"><table><thead><tr><th>#</th><th>Event</th><th>Fields</th><th class="hide-sm">Contract</th></tr></thead><tbody>' +
      logs
        .map(function (l, i) {
          var fields = (l.fields || [])
            .map(function (f) {
              return (
                '<div><span class="faint">' +
                e(f[0]) +
                ':</span> <span class="mono break">' +
                e(f[1]) +
                "</span></div>"
              );
            })
            .join("");
          return (
            "<tr><td>" +
            (i + 1) +
            '</td><td class="mono">' +
            e(l.event) +
            "</td><td>" +
            (fields || faint("—")) +
            '</td><td class="hide-sm">' +
            addrLink(l.contract) +
            "</td></tr>"
          );
        })
        .join("") +
      "</tbody></table></div>"
    );
  }

  /* ---------------- network panels (fees, confirmations, alerts) ---------------- */

  var PRIORITY_ROWS = [
    ["low_bp", "Low", "minimum fee — suitable when blocks are not full"],
    [
      "normal_bp",
      "Normal",
      "minimum + 25% — wallet default, covers one block of fee growth",
    ],
    ["high_bp", "High", "ahead of most queued transactions"],
    ["urgent_bp", "Urgent", "higher priority for the next block"],
  ];

  function feesPanel(f) {
    var cong = Number(f.congestion_bp);
    var congBadge =
      cong > 10000
        ? '<span class="badge warn">congested · ' +
          e(TC.fmtMultiplier(cong)) +
          "</span>"
        : '<span class="badge ok">no congestion · 1.00×</span>';
    var rows = PRIORITY_ROWS.map(function (p) {
      var bpv = f.priority[p[0]];
      return (
        "<tr><td><strong>" +
        e(p[1]) +
        '</strong><br><span class="faint">' +
        e(p[2]) +
        '</span></td><td class="num">' +
        e(TC.fmtMultiplier(bpv)) +
        '</td><td class="num">' +
        amount(TC.applyBp(f.typical_transfer_fee, bpv)) +
        "</td></tr>"
      );
    }).join("");
    return (
      '<div class="section-title" style="margin-top:0"><h3>Current fees</h3>' +
      congBadge +
      "</div>" +
      '<div class="table-wrap" tabindex="0" role="region" aria-label="Data table"><table><thead><tr><th>Priority</th><th class="num">× minimum</th><th class="num">Transfer (160 bytes)</th></tr></thead><tbody>' +
      rows +
      "</tbody></table></div>" +
      kv([
        ["Base fee", amount(f.base_fee)],
        ["Per 1,000 bytes", amount(f.fee_per_kb)],
        ["Per 1,000 fuel units", amount(f.fee_per_kfuel)],
        [
          "Storage deposit",
          amount(f.storage_deposit_per_kb) +
            ' <span class="faint">per kB (refundable)</span>',
        ],
        [
          "Queue (mempool)",
          TC.fmtInt(f.mempool_txs) + " tx · " + e(TC.fmtBytes(f.mempool_bytes)),
        ],
      ]) +
      '<p class="faint" style="margin:10px 0 0">Minimum = (base + size × fee per kB + reserved fuel × fee per 1,000 units) × congestion. Congestion can rise by up to 12.5% per block above 50% utilization (1× to 1,000×). The surcharge is <strong>burned</strong>. Wallet: <code>--priority low|normal|high|urgent</code>.</p>'
    );
  }

  function securityForm() {
    return (
      "<h3>How many confirmations?</h3>" +
      '<p class="muted" style="margin-bottom:10px">The larger the payment, the more confirmations the node recommends waiting for.</p>' +
      '<form id="sec-form" class="inline-form" autocomplete="off">' +
      '<label for="sec-amount">Amount received (TCN)</label>' +
      '<div class="row"><input id="sec-amount" type="text" inputmode="decimal" value="100" spellcheck="false">' +
      '<button class="btn btn-primary" type="submit">Calculate</button></div></form>' +
      '<div id="sec-out" aria-live="polite"></div>'
    );
  }

  function bindSecurityForm() {
    var form = document.getElementById("sec-form");
    if (!form) return;
    var out = document.getElementById("sec-out");
    function run() {
      var motes = TC.parseTCN(document.getElementById("sec-amount").value);
      if (motes === null || BigInt(motes) > 9007199254740991n) {
        out.innerHTML =
          '<p class="notice error" style="margin-top:12px">Enter a TCN amount, such as 250 or 12.5 (at most 90,000,000 TCN).</p>';
        return;
      }
      out.innerHTML = '<p class="loading">Querying…</p>';
      TC.api("/security?amount=" + motes)
        .then(function (s) {
          var atStake = String(
            BigInt(s.value_per_block) * BigInt(s.confirmations),
          );
          var toFinal = has(s.blocks_to_finality)
            ? Number(s.blocks_to_finality)
            : null;
          var wait = toFinal !== null ? toFinal : Number(s.confirmations);
          out.innerHTML =
            '<div class="sec-result">' +
            '<div class="stat"><span class="label">Wait for</span><span class="value">' +
            TC.fmtInt(wait) +
            (wait === 1 ? " confirmation" : " confirmations") +
            "</span>" +
            '<span class="sub">≈ ' +
            e(TC.fmtDuration(Math.max(1, wait) * BLOCK_SECONDS)) +
            " for a payment of " +
            e(TC.fmtTCN(s.amount)) +
            "</span></div>" +
            (toFinal !== null
              ? '<p class="muted" style="margin:10px 0 0">The miners are signing blocks, so this payment becomes <strong>final</strong> — irreversible, whatever its value — about ' +
                TC.fmtInt(toFinal) +
                " block(s) after it is mined (" +
                e(TC.fmtDuration(Math.max(1, toFinal) * BLOCK_SECONDS)) +
                "). Undoing it would need two thirds of the recent miners to sign a different chain."
              : '<p class="muted" style="margin:10px 0 0">No block is being finalised by the miners right now, so this answer rests on proof of work alone.') +
            " To reverse " +
            TC.fmtInt(s.confirmations) +
            " block(s), an attacker must redo their proof of work and forgo approximately " +
            e(TC.fmtTCN(atStake, { maxDecimals: 2 })) +
            " in rewards (" +
            e(TC.fmtTCN(s.value_per_block, { maxDecimals: 4 })) +
            " per block)" +
            (BigInt(atStake) >= BigInt(s.amount) * 2n
              ? " — at least twice the payment amount"
              : toFinal !== null
                ? " — the miners\u2019 signatures, not this cost, are what protect it"
                : " — less than twice the payment: the node never recommends waiting longer than the deepest reorganization it would accept, so this is the maximum wait") +
            ". Current network hashrate: " +
            e(TC.fmtHashrate(s.network_hashrate)) +
            ".</p></div>";
        })
        .catch(function (err) {
          out.innerHTML =
            '<p class="notice error" style="margin-top:12px">Error: ' +
            e(err.message || err) +
            "</p>";
        });
    }
    form.addEventListener("submit", function (ev) {
      ev.preventDefault();
      run();
    });
  }

  function alertsPanel(list) {
    var head =
      '<div class="section-title"><h2>Double-spend alerts</h2><span class="faint">observed by this node</span></div>';
    if (!list.length)
      return (
        head +
        '<p class="faint">No recent double-spend attempts observed. Conflicting transactions with the same sender and nonce appear here.</p>'
      );
    return (
      head +
      '<div class="notice error" style="margin-bottom:12px">Conflicting transactions were observed from these senders. Wait for confirmations before accepting a payment.</div>' +
      '<div class="table-wrap" tabindex="0" role="region" aria-label="Data table"><table><thead><tr><th>When</th><th>Sender</th><th class="num hide-sm">Nonce</th><th>First</th><th>Second</th></tr></thead><tbody>' +
      list
        .slice()
        .reverse()
        .map(function (a) {
          return (
            '<tr><td class="nowrap" title="' +
            e(TC.fmtDate(a.seen_at)) +
            '">' +
            e(TC.timeAgo(a.seen_at)) +
            "</td><td>" +
            addrLink(a.sender) +
            '</td><td class="num hide-sm">' +
            TC.fmtInt(a.nonce) +
            "</td><td>" +
            txLink(a.first) +
            "</td><td>" +
            txLink(a.second) +
            "</td></tr>"
          );
        })
        .join("") +
      "</tbody></table></div>"
    );
  }

  /* ---------------- pages ---------------- */

  async function pageHome(token) {
    view.innerHTML =
      '<div id="h-stats" class="grid grid-4"><p class="loading">Loading…</p></div>' +
      '<div class="grid grid-2" style="margin-top:16px">' +
      '<div class="card" id="h-fees"><h3>Current fees</h3><p class="loading">Loading…</p></div>' +
      '<div class="card" id="h-sec">' +
      securityForm() +
      "</div></div>" +
      '<div id="h-alerts"></div>' +
      '<div class="section-title"><h2>Latest blocks</h2><span class="faint">updates every 15 seconds</span></div>' +
      '<div id="h-blocks"><p class="loading">Loading…</p></div><div class="pager" id="h-pager"></div>';
    bindSecurityForm();
    await refreshHome(token);
    (function loop() {
      if (token !== routeToken) return;
      refreshTimer = setTimeout(function () {
        if (token !== routeToken) return;
        refreshHome(token)
          .catch(function () {})
          .then(loop);
      }, 15000);
    })();
  }

  async function refreshHome(token) {
    var results = await Promise.all([
      TC.api("/blocks?limit=20"),
      TC.api("/status"),
      TC.api("/fees").catch(function (err) {
        return { error: err };
      }),
      TC.api("/alerts").catch(function () {
        return null;
      }),
    ]);
    if (token !== routeToken) return;
    var blocks = results[0],
      s = results[1],
      f = results[2],
      alerts = results[3];
    document.getElementById("h-stats").innerHTML =
      '<div class="card stat"><span class="label">Height</span><span class="value">' +
      TC.fmtInt(s.height) +
      '</span><span class="sub">' +
      e(s.network) +
      (s.syncing ? " · syncing" : "") +
      " · " +
      (Number(s.finalized_height) > 0
        ? "final up to " + TC.fmtInt(s.finalized_height)
        : "no finality yet") +
      "</span></div>" +
      '<div class="card stat"><span class="label">Difficulty</span><span class="value">' +
      e(TC.fmtDifficulty(s.difficulty)) +
      '</span><span class="sub">' +
      e(TC.fmtHashrate(s.hashrate_estimate)) +
      "</span></div>" +
      '<div class="card stat"><span class="label">Issued</span><span class="value">' +
      e(TC.fmtTCN(s.supply.emitted, { maxDecimals: 0 })) +
      '</span><span class="sub">reward ' +
      e(TC.fmtTCN(s.supply.current_block_reward)) +
      " · burned " +
      e(TC.fmtTCN(s.supply.burned, { maxDecimals: 4 })) +
      "</span></div>" +
      '<div class="card stat"><span class="label">Mempool</span><span class="value"><a href="#/mempool">' +
      TC.fmtInt(s.mempool_txs) +
      ' tx</a></span><span class="sub">' +
      e(TC.fmtBytes(s.mempool_bytes)) +
      " · congestion " +
      e(TC.fmtMultiplier(s.congestion_bp)) +
      "</span></div>";
    document.getElementById("h-fees").innerHTML =
      f && !f.error
        ? feesPanel(f)
        : '<h3>Current fees</h3><p class="notice error">Unable to load fees (' +
          e(f && f.error ? f.error.message : "?") +
          ").</p>";
    document.getElementById("h-alerts").innerHTML = alerts
      ? alertsPanel(alerts)
      : "";
    document.getElementById("h-blocks").innerHTML = blocksTable(
      blocks,
      s.finalized_height,
    );
    var lowest = blocks.length ? blocks[blocks.length - 1].height : 0;
    document.getElementById("h-pager").innerHTML =
      lowest > 0
        ? '<a class="btn" href="#/blocks/' + lowest + '">Older blocks →</a>'
        : "";
  }

  function blocksTable(blocks, finalizedHeight) {
    return (
      '<div class="table-wrap" tabindex="0" role="region" aria-label="Data table"><table><thead><tr><th>Height</th><th>Status</th><th>Hash</th><th>Age</th><th class="num">Txs</th><th class="hide-sm">Miner</th><th class="num hide-sm">Size</th></tr></thead><tbody>' +
      blocks
        .map(function (b) {
          return (
            "<tr><td>" +
            blockLink(b.height, TC.fmtInt(b.height)) +
            "</td><td>" +
            finalBadge(finalizedAt(b.height, finalizedHeight)) +
            "</td><td>" +
            blockLink(b.hash, TC.shortHash(b.hash)) +
            '</td><td class="nowrap" title="' +
            e(TC.fmtDate(b.timestamp)) +
            '">' +
            e(TC.timeAgo(b.timestamp)) +
            '</td><td class="num">' +
            TC.fmtInt(b.tx_count) +
            '</td><td class="hide-sm">' +
            addrLink(b.miner) +
            '</td><td class="num hide-sm">' +
            e(TC.fmtBytes(b.size)) +
            "</td></tr>"
          );
        })
        .join("") +
      "</tbody></table></div>"
    );
  }

  async function pageBlocks(token, before) {
    if (!(before > 0)) {
      location.hash = "#/";
      return;
    }
    var res = await Promise.all([
      TC.api("/blocks?limit=20&before=" + before),
      statusOrNull(),
    ]);
    if (token !== routeToken) return;
    var blocks = res[0],
      st = res[1];
    var html =
      '<div class="section-title"><h2>Blocks before ' +
      TC.fmtInt(before) +
      '</h2><a href="#/">← latest</a></div>' +
      blocksTable(blocks, st && st.finalized_height);
    var lowest = blocks.length ? blocks[blocks.length - 1].height : 0;
    if (lowest > 0)
      html +=
        '<div class="pager"><a class="btn" href="#/blocks/' +
        lowest +
        '">Older blocks →</a></div>';
    view.innerHTML = html;
  }

  /**
   * Uncles: recent valid blocks that lost the race. The block that carries one
   * pays its miner (7 - depth)/24 of the subsidy, and its work counts for the
   * chain — which keeps small miners fair at 15 s blocks.
   */
  function unclesTable(uncles) {
    return (
      '<div class="table-wrap" tabindex="0" role="region" aria-label="Data table"><table><thead><tr><th>Height</th><th>Hash</th><th class="hide-sm">Miner</th><th class="num">Blocks behind</th><th class="num">Reward</th></tr></thead><tbody>' +
      uncles
        .map(function (u) {
          return (
            "<tr><td>" +
            TC.fmtInt(u.height) +
            '</td><td><span class="mono">' +
            e(TC.shortHash(u.hash)) +
            '</span></td><td class="hide-sm">' +
            addrLink(u.miner) +
            '</td><td class="num">' +
            TC.fmtInt(u.depth) +
            '</td><td class="num">' +
            amount(u.reward) +
            "</td></tr>"
          );
        })
        .join("") +
      "</tbody></table></div>"
    );
  }

  async function pageBlock(token, id) {
    var b = await TC.api("/block/" + encodeURIComponent(id));
    if (token !== routeToken) return;
    var html =
      '<div class="crumbs"><a href="#/">Blocks</a> › block ' +
      TC.fmtInt(b.height) +
      "</div>";
    html +=
      '<div class="section-title"><h1>Block ' +
      TC.fmtInt(b.height) +
      " " +
      finalBadge(b.finalized) +
      "</h1><span>" +
      (b.height > 0
        ? '<a class="btn btn-small" href="#/block/' +
          (b.height - 1) +
          '">← previous</a> '
        : "") +
      (Number(b.confirmations) > 1
        ? '<a class="btn btn-small" href="#/block/' +
          (b.height + 1) +
          '">next →</a>'
        : "") +
      "</span></div>";
    var burned = b.txs.reduce(function (acc, t) {
      return acc + BigInt(t.burned || 0);
    }, 0n);
    var uncles = b.uncles || [];
    var uncleReward = uncles.reduce(function (acc, u) {
      return acc + BigInt(u.reward || 0);
    }, 0n);
    html +=
      '<div class="card">' +
      kv([
        ["Hash", mono(b.hash) + copyBtn(b.hash)],
        [
          "Confirmations",
          b.confirmations > 0
            ? TC.fmtInt(b.confirmations)
            : '<span class="badge warn">outside the main chain</span>',
        ],
        ["Finality", finalBadge(b.finalized) + finalityNote(b.finalized)],
        [
          "Date",
          e(TC.fmtDate(b.timestamp)) +
            ' <span class="faint">(' +
            e(TC.timeAgo(b.timestamp)) +
            ")</span>",
        ],
        ["Miner", addrLink(b.miner, false)],
        [
          "Reward",
          amount(b.subsidy) +
            ' <span class="faint">+ fees</span> ' +
            amount(b.fees),
        ],
        burned > 0n
          ? ["Burned (congestion surcharge)", amount(burned.toString())]
          : null,
        [
          "Uncles",
          uncles.length
            ? TC.fmtInt(uncles.length) +
              ' <span class="faint">rewarded with</span> ' +
              amount(uncleReward.toString())
            : faint("none"),
        ],
        ["Transactions", TC.fmtInt(b.tx_count)],
        ["Size", e(TC.fmtBytes(b.size))],
        [
          "Previous block",
          b.height > 0 ? blockLink(b.prev_hash) : faint("genesis"),
        ],
        ["Difficulty", e(TC.fmtDifficulty(b.difficulty))],
        ["Target", mono(b.target)],
        ["Nonce", '<span class="mono">' + e(b.nonce) + "</span>"],
        ["Transaction root", mono(b.tx_root)],
        ["State root", mono(b.state_root)],
        [
          "Signaling",
          b.signal
            ? '<span class="mono">0x' +
              Number(b.signal).toString(16) +
              "</span> (governance bits)"
            : faint("none"),
        ],
        ["Version", e(b.version)],
      ]) +
      "</div>";
    if (uncles.length) {
      html +=
        '<div class="section-title"><h2>Uncles</h2><span class="faint">recent blocks that lost the race</span></div>' +
        '<p class="muted">These blocks were valid but arrived second. This block carries them, so their work counts for the chain and their miners are paid part of the subsidy — (7 − blocks behind) ÷ 24 each. That is what keeps a small miner fair when blocks come every 15 seconds.</p>' +
        unclesTable(uncles);
    }
    html +=
      '<div class="section-title"><h2>Transactions</h2></div>' + txRows(b.txs);
    view.innerHTML = html;
  }

  async function pageTx(token, id) {
    var res = await Promise.all([
      TC.api("/tx/" + encodeURIComponent(id)),
      statusOrNull(),
    ]);
    if (token !== routeToken) return;
    var t = res[0],
      st = res[1];
    var a = t.action;
    var confirmed = !t.in_mempool && has(t.block_height);
    var isFinal =
      confirmed && st && finalizedAt(t.block_height, st.finalized_height);
    var status = t.in_mempool
      ? '<span class="badge warn">waiting in mempool</span>'
      : '<span class="badge ok">confirmed</span> ' +
        TC.fmtInt(t.confirmations) +
        (Number(t.confirmations) === 1 ? " confirmation" : " confirmations");
    if (confirmed) status += " " + finalBadge(isFinal);
    if (confirmed && t.success === false)
      status += ' <span class="badge bad">execution failed</span>';
    var html =
      '<div class="crumbs"><a href="#/">Blocks</a> › transaction</div><h1>Transaction</h1>';
    if (t.conflict) {
      html +=
        '<div class="notice error" style="margin-bottom:16px"><strong>Double-spend alert.</strong> The sender submitted another transaction with the same nonce (' +
        txLink(t.conflict) +
        "). " +
        (t.in_mempool
          ? "Only one can confirm. Wait for confirmations before accepting the payment."
          : "This transaction confirmed; the other cannot be included in the same chain.") +
        "</div>";
    }
    if (t.replaceable && t.in_mempool) {
      html +=
        '<div class="notice accent" style="margin-bottom:16px">This transaction was submitted as <strong>replaceable</strong>: before confirmation, the sender can replace it with a higher-fee transaction (<code>bump-fee</code>). Recipients should wait for at least one confirmation.</div>';
    }
    var feeCell =
      amount(t.fee) +
      ' <span class="faint">(' +
      TC.fmtInt(t.size) +
      " bytes)</span>";
    if (Number(t.burned) > 0)
      feeCell +=
        ' · <span class="nowrap">burned ' +
        e(TC.fmtTCN(t.burned)) +
        "</span>";
    html +=
      '<div class="card">' +
      kv([
        ["Txid", mono(t.txid) + copyBtn(t.txid)],
        ["Status", status],
        confirmed
          ? [
              "Finality",
              isFinal
                ? '<span class="badge ok">final</span> <span class="faint">its block was signed by two thirds of the recent miners: this payment can never be undone</span>'
                : '<span class="badge warn">not final yet</span> <span class="faint">waiting for the miners\u2019 signatures — normally one more block, about 30 seconds</span>',
            ]
          : null,
        has(t.block_height)
          ? ["Block", blockLink(t.block_height, TC.fmtInt(t.block_height))]
          : null,
        ["Type", e(actionLabel(a))],
        ["Sender", addrLink(t.sender, false)],
        ["Nonce", TC.fmtInt(t.nonce)],
        ["Fee", feeCell],
        [
          "Replaceable (RBF)",
          t.replaceable
            ? '<span class="badge warn">yes</span> <span class="faint">the sender enabled fee replacement before confirmation</span>'
            : "no",
        ],
        [
          "Expires",
          t.expiry_height
            ? "after block " + TC.fmtInt(t.expiry_height)
            : faint("does not expire"),
        ],
        t.created
          ? [
              a.type === "propose" ? "Created proposal" : "Created contract",
              a.type === "propose"
                ? proposalLink(t.created, false)
                : contractLink(t.created, false),
            ]
          : null,
      ]) +
      "</div>";
    html +=
      '<div class="section-title"><h2>' +
      e(actionLabel(a)) +
      '</h2></div><div class="card">' +
      actionDetails(t) +
      "</div>";

    if (isProgramTx(t) || t.success === false || (t.logs && t.logs.length)) {
      html +=
        '<div class="section-title"><h2>Execution result</h2></div><div class="card">';
      if (!confirmed) {
        html +=
          '<p class="muted" style="margin:0">The transaction has not executed yet. Its result, fuel usage, and events appear after confirmation.</p>';
      } else {
        var maxFuel = isProgramTx(t) ? a.max_fuel : null;
        html += kv([
          [
            "Result",
            t.success === false
              ? '<span class="badge bad">failed</span> <span class="mono break">' +
                e(t.error || "unknown error") +
                '</span><br><span class="faint">All effects were reverted; the fee was charged.</span>'
              : '<span class="badge ok">success</span>',
          ],
          [
            "Fuel used",
            TC.fmtInt(t.fuel_used) +
              (maxFuel
                ? ' <span class="faint">of ' +
                  TC.fmtInt(maxFuel) +
                  " reserved</span>"
                : ""),
          ],
          [
            "Burned fee",
            Number(t.burned) > 0
              ? amount(t.burned)
              : faint("none (no congestion)"),
          ],
          a.type === "deploy" && t.program
            ? ["Created contract", addrLink(t.program, false)]
            : null,
          has(t.return_value) ? ["Return value", mono(t.return_value)] : null,
        ]);
        html += '<h3 style="margin-top:18px">Events</h3>' + logsTable(t.logs);
      }
      html += "</div>";
    }
    view.innerHTML = html;
  }

  var ARG_HINTS = {
    int: "number, e.g. 42 or 2.5tcn",
    bool: "true or false",
    text: "text",
    bytes: "hexadecimal, e.g. 0xabcd",
    address: "address " + HRP + "1…",
  };
  function argHint(type) {
    if (ARG_HINTS[type]) return ARG_HINTS[type];
    if (/^list\[/.test(type)) return "list, e.g. [1, 2, 3]";
    return type;
  }

  function signature(f) {
    return (
      f.name +
      "(" +
      f.params
        .map(function (p) {
          return p[0] + ": " + p[1];
        })
        .join(", ") +
      ")" +
      (f.returns && f.returns !== "nothing" ? " -> " + f.returns : "") +
      (f.payable ? " payable" : "")
    );
  }

  /** Builds (with DOM APIs, no HTML strings) a form that calls a view through the API. */
  function viewForm(addr, f) {
    var form = document.createElement("form");
    form.className = "view-form";
    form.setAttribute("autocomplete", "off");
    var title = document.createElement("div");
    title.className = "mono view-sig";
    title.textContent = signature(f);
    form.appendChild(title);
    var inputs = [];
    f.params.forEach(function (p, i) {
      var id = "v-" + f.name + "-" + i;
      var label = document.createElement("label");
      label.setAttribute("for", id);
      label.textContent = p[0] + " (" + p[1] + ")";
      var input = document.createElement("input");
      input.type = "text";
      input.id = id;
      input.spellcheck = false;
      input.placeholder = argHint(p[1]);
      form.appendChild(label);
      form.appendChild(input);
      inputs.push(input);
    });
    var btn = document.createElement("button");
    btn.className = "btn btn-small";
    btn.type = "submit";
    btn.textContent = "Query";
    form.appendChild(btn);
    var out = document.createElement("div");
    out.className = "view-out";
    out.setAttribute("aria-live", "polite");
    form.appendChild(out);
    form.addEventListener("submit", function (ev) {
      ev.preventDefault();
      btn.disabled = true;
      out.className = "view-out faint";
      out.textContent = "Querying…";
      TC.post("/program/" + encodeURIComponent(addr) + "/view", {
        function: f.name,
        args: inputs.map(function (x) {
          return x.value;
        }),
      })
        .then(function (r) {
          if (has(r.result)) {
            out.className = "view-out ok";
            out.textContent =
              "→ " + r.result + "   (fuel: " + TC.fmtInt(r.fuel_used) + ")";
          } else {
            out.className = "view-out bad";
            out.textContent =
              "Error: " +
              (r.error || "no result") +
              "   (fuel: " +
              TC.fmtInt(r.fuel_used) +
              ")";
          }
        })
        .catch(function (err) {
          out.className = "view-out bad";
          out.textContent = "Error: " + (err.message || err);
        })
        .then(function () {
          btn.disabled = false;
        });
    });
    return form;
  }

  function accountCards(acc) {
    return (
      '<div class="grid grid-4">' +
      '<div class="card stat"><span class="label">Balance</span><span class="value">' +
      e(TC.fmtTCN(acc.balance)) +
      '</span><span class="sub">total in account</span></div>' +
      '<div class="card stat"><span class="label">Spendable</span><span class="value">' +
      e(TC.fmtTCN(acc.spendable)) +
      '</span><span class="sub">balance minus locked funds</span></div>' +
      '<div class="card stat"><span class="label">Locked in votes</span><span class="value">' +
      e(TC.fmtTCN(acc.locked)) +
      '</span><span class="sub">' +
      (Number(acc.locked) > 0
        ? "until block " + TC.fmtInt(acc.locked_until)
        : "—") +
      "</span></div>" +
      '<div class="card stat"><span class="label">Rewards in cooldown</span><span class="value">' +
      e(TC.fmtTCN(acc.immature)) +
      '</span><span class="sub">not yet in balance: 25% released after 400 blocks, the rest after 4,000</span></div>' +
      "</div>"
    );
  }

  async function pageAddress(token, addr) {
    var res = await Promise.all([
      TC.api("/address/" + encodeURIComponent(addr)),
      TC.api("/program/" + encodeURIComponent(addr)).catch(function (err) {
        if (err.status === 404 || err.status === 400) return null;
        throw err;
      }),
    ]);
    if (token !== routeToken) return;
    var acc = res[0],
      prog = res[1];
    var html;
    if (prog) {
      html =
        '<div class="crumbs"><a href="#/">Blocks</a> › TCCL contract</div><h1>TCCL contract · <span class="mono">' +
        e(prog.name) +
        "</span></h1>";
      html +=
        '<p class="mono break">' +
        e(prog.address) +
        copyBtn(prog.address) +
        "</p>";
      html +=
        '<div class="grid grid-4">' +
        '<div class="card stat"><span class="label">Contract balance</span><span class="value">' +
        e(TC.fmtTCN(prog.balance)) +
        "</span></div>" +
        '<div class="card stat"><span class="label">Storage</span><span class="value">' +
        e(TC.fmtBytes(prog.state_bytes)) +
        '</span><span class="sub">' +
        TC.fmtInt(prog.storage_items) +
        " items (code + data)</span></div>" +
        '<div class="card stat"><span class="label">Storage deposit</span><span class="value">' +
        e(TC.fmtTCN(prog.deposit)) +
        '</span><span class="sub">refundable when storage is released</span></div>' +
        '<div class="card stat"><span class="label">Upgrades</span><span class="value">' +
        (prog.upgrade_authority ? "possible" : "final") +
        '</span><span class="sub">' +
        (prog.upgrade_authority
          ? "authority " + e(TC.shortAddr(prog.upgrade_authority))
          : "the code can never change") +
        " · code v" +
        e(prog.code_version) +
        "</span></div>" +
        '<div class="card stat"><span class="label">Functions</span><span class="value">' +
        TC.fmtInt(prog.functions.length) +
        '</span><span class="sub">' +
        TC.fmtInt(
          prog.functions.filter(function (f) {
            return f.kind === "view";
          }).length,
        ) +
        " free views</span></div>" +
        "</div>";
      html +=
        '<div class="card" style="margin-top:16px">' +
        kv([
          ["Creator", addrLink(prog.creator, false)],
          [
            "Created at block",
            blockLink(prog.created_height, TC.fmtInt(prog.created_height)),
          ],
          [
            Number(prog.code_version) > 1
              ? "Last upgrade transaction"
              : "Deployment transaction",
            txLink(prog.deploy_txid, false),
          ],
          ["Source code hash", mono(prog.source_hash)],
          ["Compiled code hash", mono(prog.code_hash)],
          [
            "Upgrades",
            prog.upgrade_authority
              ? '<span class="badge warn">can be replaced</span> <span class="faint">by</span> ' +
                addrLink(prog.upgrade_authority, false) +
                '<br><span class="faint">That address may publish new code for this contract. It can hand the authority over, or give it up to make the contract final for ever.</span>'
              : '<span class="badge ok">final</span> <span class="faint">nobody can change this code — what you read below is what runs, for ever</span>',
          ],
          [
            "Code version",
            TC.fmtInt(prog.code_version) +
              (Number(prog.code_version) > 1
                ? ' <span class="faint">(' +
                  TC.fmtInt(Number(prog.code_version) - 1) +
                  " upgrade" +
                  (Number(prog.code_version) === 2 ? "" : "s") +
                  " since the first deployment)</span>"
                : ' <span class="faint">(original deployment)</span>'),
          ],
          [
            "Language",
            "TCCL version " +
              e(prog.language) +
              (Number(prog.language) >= 2
                ? ' <span class="faint">records, enums, roles, interfaces and calls between contracts</span>'
                : ""),
          ],
          [
            "Nonce / pending transactions",
            TC.fmtInt(acc.nonce) + " / " + TC.fmtInt(acc.mempool_txs),
          ],
        ]) +
        '<p style="margin:14px 0 0"><button class="btn btn-small" type="button" id="load-source">Show source code</button></p><div id="source-box"></div></div>';
      html +=
        '<div class="section-title"><h2>Functions</h2></div><div class="table-wrap" tabindex="0" role="region" aria-label="Data table"><table><thead><tr><th>Function</th><th>Type</th><th class="hide-sm">Parameters</th><th class="hide-sm">Returns</th></tr></thead><tbody>' +
        prog.functions
          .map(function (f) {
            return (
              '<tr><td class="mono">' +
              e(f.name) +
              "</td><td>" +
              e(FN_KIND_LABELS[f.kind] || f.kind) +
              (f.payable ? ' <span class="badge accent">payable</span>' : "") +
              '</td><td class="hide-sm mono">' +
              (f.params.length
                ? f.params
                    .map(function (p) {
                      return e(p[0]) + ": " + e(p[1]);
                    })
                    .join(", ")
                : "—") +
              '</td><td class="hide-sm mono">' +
              e(f.returns === "nothing" ? "—" : f.returns) +
              "</td></tr>"
            );
          })
          .join("") +
        "</tbody></table></div>";
      html +=
        '<div class="section-title"><h2>Read-only views</h2><span class="faint">free, no transaction</span></div><div id="views" class="grid grid-2"></div>';
      html +=
        '<p class="faint">To call actions, use the wallet: <code>thecoin-wallet contract invoke ' +
        e(prog.address) +
        " &lt;function&gt; [arguments…]</code>.</p>";
    } else {
      html =
        '<div class="crumbs"><a href="#/">Blocks</a> › address</div><h1>Address</h1>';
      html +=
        '<p class="mono break">' +
        e(acc.address) +
        copyBtn(acc.address) +
        "</p>";
      html += accountCards(acc);
      html +=
        '<div class="card" style="margin-top:16px">' +
        kv([
          ["Nonce (sent transactions)", TC.fmtInt(acc.nonce)],
          ["Next nonce", TC.fmtInt(acc.next_nonce)],
          ["Pending transactions", TC.fmtInt(acc.mempool_txs)],
        ]) +
        "</div>";
    }
    html +=
      '<div class="section-title"><h2>History</h2></div><div id="history"><p class="loading">Loading history…</p></div><div class="pager" id="more"></div>';
    view.innerHTML = html;

    if (prog) {
      var box = document.getElementById("views");
      var views = prog.functions.filter(function (f) {
        return f.kind === "view";
      });
      if (!views.length)
        box.outerHTML =
          '<p class="faint">This contract has no read-only views.</p>';
      views.forEach(function (f) {
        var card = document.createElement("div");
        card.className = "card";
        card.appendChild(viewForm(prog.address, f));
        box.appendChild(card);
      });
      document
        .getElementById("load-source")
        .addEventListener("click", function () {
          loadSource(token, prog, this);
        });
    }
    await loadHistory(token, acc.address, null, []);
  }

  async function loadSource(token, prog, btn) {
    var boxEl = document.getElementById("source-box");
    btn.disabled = true;
    btn.textContent = "Loading…";
    try {
      var t = await TC.api("/tx/" + prog.deploy_txid);
      if (token !== routeToken) return;
      var src = t.action && t.action.source;
      if (typeof src !== "string")
        throw new Error("deployment transaction has no source code");
      boxEl.textContent = "";
      var note = document.createElement("p");
      var ok = t.action.source_hash === prog.source_hash;
      note.className = ok ? "badge ok" : "badge bad";
      note.textContent = ok
        ? Number(prog.code_version) > 1
          ? "✓ matches the source hash of the code running now (version " +
            prog.code_version +
            ")"
          : "✓ matches the hash stored in the contract"
        : "✗ hash does not match";
      var pre = document.createElement("pre");
      var code = document.createElement("code");
      code.textContent = src;
      pre.appendChild(code);
      pre.style.marginTop = "10px";
      boxEl.appendChild(note);
      boxEl.appendChild(pre);
      btn.remove();
    } catch (err) {
      btn.disabled = false;
      btn.textContent = "Show source code";
      boxEl.textContent = "Unable to load source code: " + (err.message || err);
    }
  }

  /*
   * The history endpoint pages with cursor "height:position" (exclusive).
   */
  async function loadHistory(token, addr, cursor, acc) {
    var limit = 25;
    var rows;
    try {
      rows = await TC.api(
        "/address/" +
          encodeURIComponent(addr) +
          "/txs?limit=" +
          limit +
          (cursor ? "&cursor=" + cursor : ""),
      );
    } catch (err) {
      if (token !== routeToken) return;
      document.getElementById("history").innerHTML =
        '<div class="notice error">' + e(err.message) + "</div>";
      return;
    }
    if (token !== routeToken) return;
    var all = acc.concat(rows);
    document.getElementById("history").innerHTML = txRows(all, {
      showBlock: true,
    });
    var more = document.getElementById("more");
    more.innerHTML = "";
    var confirmed = rows.filter(function (t) {
      return !t.in_mempool && t.block_hash;
    });
    if (confirmed.length >= limit) {
      var last = confirmed[confirmed.length - 1];
      var btn = document.createElement("button");
      btn.className = "btn";
      btn.textContent = "Load more";
      btn.onclick = async function () {
        btn.disabled = true;
        btn.textContent = "Loading…";
        try {
          var pos = has(last.position) ? last.position : null;
          if (pos === null) {
            var blk = await TC.api("/block/" + last.block_hash);
            pos = Math.max(
              0,
              blk.txs.findIndex(function (t) {
                return t.txid === last.txid;
              }),
            );
          }
          await loadHistory(token, addr, last.block_height + ":" + pos, all);
        } catch (err) {
          btn.textContent = "Error: " + err.message;
        }
      };
      more.appendChild(btn);
    }
  }

  function contractState(c) {
    var s = c.state;
    switch (s.kind) {
      case "escrow":
        return kv([
          ["Payer", addrLink(s.payer, false)],
          ["Payee", addrLink(s.payee, false)],
          ["Arbiter", s.arbiter ? addrLink(s.arbiter, false) : faint("none")],
          [
            "Deadline",
            "payer can refund after block " + TC.fmtInt(s.deadline_height),
          ],
        ]);
      case "vesting": {
        var total = BigInt(s.total),
          vested = BigInt(s.vested_now),
          claimed = BigInt(s.claimed);
        var pct = total > 0n ? Number((vested * 10000n) / total) / 100 : 0;
        return (
          kv([
            ["Beneficiary", addrLink(s.beneficiary, false)],
            ["Total", amount(s.total)],
            [
              "Vested so far",
              amount(s.vested_now) + " (" + pct.toFixed(2) + "%)",
            ],
            ["Already claimed", amount(s.claimed)],
            [
              "Available to claim",
              amount(String(vested > claimed ? vested - claimed : 0n)),
            ],
            [
              "Start / cliff / end",
              TC.fmtInt(s.start_height) +
                " / " +
                TC.fmtInt(s.cliff_height) +
                " / " +
                TC.fmtInt(s.end_height),
            ],
            ["Revocable", s.revocable ? "yes" : "no"],
          ]) + meter("Vested", pct, null)
        );
      }
      case "subscription":
        return kv([
          ["Payer", addrLink(s.payer, false)],
          ["Payee", addrLink(s.payee, false)],
          ["Amount per period", amount(s.amount_per_period)],
          [
            "Period",
            TC.fmtInt(s.period_blocks) +
              " blocks ≈ " +
              TC.fmtDuration(s.period_blocks * BLOCK_SECONDS),
          ],
          [
            "Claimed periods",
            TC.fmtInt(s.claimed_periods) + " of " + TC.fmtInt(s.max_periods),
          ],
          ["Claimable now", TC.fmtInt(s.claimable_periods_now)],
          ["Start", "block " + TC.fmtInt(s.start_height)],
        ]);
      case "htlc":
        return kv([
          ["Sender", addrLink(s.sender, false)],
          ["Recipient", addrLink(s.recipient, false)],
          ["Hash lock (SHA-256)", mono(s.hash_lock)],
          ["Expires after block", TC.fmtInt(s.timeout_height)],
        ]);
      case "multisig":
        return (
          kv([
            [
              "Signers",
              s.signers
                .map(function (x) {
                  return addrLink(x, false);
                })
                .join("<br>"),
            ],
            [
              "Threshold",
              e(s.threshold) + " of " + s.signers.length + " approvals",
            ],
            ["Next spend ID", TC.fmtInt(s.next_spend_id)],
          ]) +
          '<h3 style="margin-top:18px">Pending spends</h3>' +
          (s.pending.length
            ? '<div class="table-wrap" tabindex="0" role="region" aria-label="Data table"><table><thead><tr><th>#</th><th>To</th><th class="num">Amount</th><th>Approvals</th><th class="hide-sm">Created</th></tr></thead><tbody>' +
              s.pending
                .map(function (p) {
                  return (
                    "<tr><td>" +
                    e(p.id) +
                    "</td><td>" +
                    addrLink(p.to) +
                    '</td><td class="num">' +
                    amount(p.amount) +
                    "</td><td>" +
                    p.approvals.length +
                    "/" +
                    e(s.threshold) +
                    " " +
                    p.approvals
                      .map(function (x) {
                        return addrLink(x);
                      })
                      .join(", ") +
                    '</td><td class="hide-sm">block ' +
                    TC.fmtInt(p.created_height) +
                    "</td></tr>"
                  );
                })
                .join("") +
              "</tbody></table></div>"
            : '<p class="faint">No pending spends. With a zero balance, any signer can close the vault and refund its deposit: <code>thecoin-wallet contract multisig-close</code>.</p>')
        );
      default:
        return "<pre><code>" + e(JSON.stringify(s, null, 2)) + "</code></pre>";
    }
  }

  function meter(label, pct, thresholdPct) {
    var p = Math.max(0, Math.min(100, pct));
    return (
      '<div class="meter-row"><div class="meter-head"><span>' +
      e(label) +
      '</span><span class="v">' +
      pct.toFixed(2) +
      "%</span></div>" +
      '<div class="meter" role="progressbar" aria-valuemin="0" aria-valuemax="100" aria-valuenow="' +
      p.toFixed(0) +
      '"><div class="fill" style="width:' +
      p +
      '%"></div>' +
      (thresholdPct !== null
        ? '<span class="threshold" style="left:calc(' +
          thresholdPct +
          '% - 1px)"></span>'
        : "") +
      "</div></div>"
    );
  }

  async function pageContract(token, id) {
    var c;
    try {
      c = await TC.api("/contract/" + encodeURIComponent(id));
    } catch (err) {
      if (err.status === 404) {
        if (token !== routeToken) return;
        view.innerHTML =
          '<div class="crumbs"><a href="#/">Blocks</a> › contract</div><h1>Contract</h1><p class="mono break">' +
          e(id) +
          "</p>" +
          '<div class="notice">This contract is not in the current state. Closed zero-balance contracts are removed to keep state compact, and storage deposits are refunded to the creator. Call history remains in participant transactions.</div>';
        return;
      }
      throw err;
    }
    if (token !== routeToken) return;
    var html =
      '<div class="crumbs"><a href="#/">Blocks</a> › contract</div><h1>Contract · ' +
      e(KIND_LABELS[c.state.kind] || c.state.kind) +
      "</h1>";
    html +=
      '<div class="card">' +
      kv([
        ["Id", mono(c.id) + copyBtn(c.id)],
        ["Creator", addrLink(c.creator, false)],
        [
          "Created at block",
          blockLink(c.created_height, TC.fmtInt(c.created_height)),
        ],
        ["Contract balance", amount(c.balance)],
        [
          "Storage deposit",
          has(c.deposit)
            ? amount(c.deposit) +
              ' <span class="faint">refunded to the creator when the contract closes</span>'
            : faint("—"),
        ],
      ]) +
      "</div>";
    html +=
      '<div class="section-title"><h2>State</h2></div><div class="card">' +
      contractState(c) +
      "</div>";
    view.innerHTML = html;
  }

  async function pageMempool(token) {
    var m = await TC.api("/mempool");
    if (token !== routeToken) return;
    var html =
      '<div class="crumbs"><a href="#/">Blocks</a> › mempool</div><div class="section-title"><h1>Mempool</h1><span class="faint">updates every 15 seconds</span></div>';
    html +=
      '<p class="muted">' +
      TC.fmtInt(m.count) +
      " pending transactions (" +
      e(TC.fmtBytes(m.bytes)) +
      "). Showing the highest fee per unit of weight (bytes + fuel ÷ 100).</p>";
    html += txRows(m.txs);
    view.innerHTML = html;
    refreshTimer = setTimeout(function () {
      if (token === routeToken) pageMempool(token).catch(function () {});
    }, 15000);
  }

  async function exists(path) {
    try {
      await TC.api(path);
      return true;
    } catch (err) {
      if (err.status === 404 || err.status === 400) return false;
      throw err;
    }
  }

  async function search(qRaw) {
    var q = qRaw.trim();
    if (!q) return;
    function go(hash) {
      if (location.hash === hash) route();
      else location.hash = hash;
    }
    if (/^\d+$/.test(q)) {
      go("#/block/" + q);
      return;
    }
    if (TC.isAddress(q)) {
      go("#/address/" + q.toLowerCase());
      return;
    }
    if (TC.isHash(q)) {
      var h = q.toLowerCase();
      view.innerHTML =
        '<p class="loading">Searching ' + e(TC.shortHash(h)) + "…</p>";
      if (await exists("/block/" + h)) {
        go("#/block/" + h);
        return;
      }
      if (await exists("/tx/" + h)) {
        go("#/tx/" + h);
        return;
      }
      if (await exists("/contract/" + h)) {
        go("#/contract/" + h);
        return;
      }
      if (await exists("/governance/proposal/" + h)) {
        if (GOVERNANCE_URL) {
          location.href = GOVERNANCE_URL + "#/proposal/" + h;
        } else {
          view.innerHTML =
            '<div class="notice accent">Governance proposal <span class="mono break">' +
            e(h) +
            "</span> exists on the " +
            e(NET.label) +
            ". Its votes can be followed with <code>thecoin-wallet --network " +
            e(NET.node) +
            " gov show " +
            e(h) +
            "</code>.</div>";
        }
        return;
      }
      view.innerHTML =
        '<div class="notice error">No results for <span class="mono break">' +
        e(h) +
        "</span>. Closed contracts are removed from the state; rejected transactions never enter the blockchain.</div>";
      return;
    }
    view.innerHTML =
      '<div class="notice error">Enter a block height, a 64-character hexadecimal hash, or a <span class="mono">' + HRP + '1…</span> address (accounts and TCCL contracts).</div>';
  }

  /* ---------------- router ---------------- */
  async function route() {
    clearTimeout(refreshTimer);
    var token = ++routeToken;
    if (location.hash === "#main") return; // skip link, not a route
    var parts = (location.hash || "#/").replace(/^#\/?/, "").split("/");
    var kind = parts[0] || "",
      arg = parts.slice(1).join("/");
    try {
      arg = decodeURIComponent(arg);
    } catch (err) {
      kind = "invalid";
    }
    if (kind !== "blocks" && kind !== "") window.scrollTo(0, 0);
    view.innerHTML = '<p class="loading">Loading…</p>';
    try {
      switch (kind) {
        case "":
          await pageHome(token);
          break;
        case "blocks":
          await pageBlocks(token, Number(arg));
          break;
        case "block":
          await pageBlock(token, arg);
          break;
        case "tx":
          await pageTx(token, arg);
          break;
        case "address":
        case "program":
          await pageAddress(token, arg);
          break;
        case "contract":
          await pageContract(token, arg);
          break;
        case "mempool":
          await pageMempool(token);
          break;
        case "search":
          await search(arg);
          break;
        default:
          view.innerHTML = '<div class="notice error">Page not found.</div>';
      }
    } catch (err) {
      if (token === routeToken)
        showError(
          err,
          err && err.status === 404 && kind === "tx"
            ? "Transaction not found (it may have been rejected or replaced)"
            : undefined,
        );
    }
  }

  document.getElementById("search").addEventListener("submit", function (ev) {
    ev.preventDefault();
    var q = document.getElementById("q").value;
    search(q).catch(function (err) {
      showError(err);
    });
  });
  if (!NET.launched) {
    view.innerHTML =
      '<div class="notice accent"><strong>The ' +
      e(NET.label) +
      " is not online yet.</strong><p>This explorer is ready for it and will show its blocks and transactions as soon as the network launches. Meanwhile, explore the DevNet with the network selector above.</p></div>";
    document.getElementById("search").hidden = true;
    return;
  }
  window.addEventListener("hashchange", route);
  route();
  /* Warn if the API answers for a different network than the one selected. */
  TC.api("/status")
    .then(function (s) {
      if (s && s.network && s.network !== NET.node) {
        var warn = document.getElementById("network-warning");
        if (warn) {
          warn.textContent =
            "Warning: the node answering reports the " + s.network + " network, not the " + NET.label + ".";
          warn.hidden = false;
        }
      }
    })
    .catch(function () {
      /* outage messages are shown by the pages */
    });
})();
