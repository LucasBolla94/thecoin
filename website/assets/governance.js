/* The Coin governance page: proposals, tallies and parameters. */
(function () {
  "use strict";

  var view = document.getElementById("view");
  var e = TC.esc;
  var token = 0;

  var STATUS = {
    voting: ["Voting", "accent"],
    approved: ["Approved · awaiting activation", "ok"],
    activated: ["Activated", "ok"],
    rejected: ["Rejected", "bad"],
  };

  /* [label, unit, short explanation] — names match GET /governance/params. */
  var PARAM_INFO = {
    base_fee: ["Base fee", "tcn", "charged on every transaction"],
    fee_per_kb: ["Size fee", "tcn_kb", "per 1,000 transaction bytes"],
    fee_per_kfuel: [
      "Fuel fee",
      "tcn_kfuel",
      "per 1,000 reserved fuel units (TCCL contracts)",
    ],
    storage_deposit_per_kb: [
      "Storage deposit",
      "tcn_kb",
      "locked per kB of contract state; refunded when storage is released",
    ],
    max_block_bytes: ["Maximum block size", "bytes", "byte limit per block"],
    max_block_fuel: [
      "Maximum block fuel",
      "fuel",
      "sum of contract max_fuel reservations per block",
    ],
    proposal_deposit: [
      "Proposal deposit",
      "tcn",
      "refunded if quorum is reached; otherwise burned",
    ],
    vote_period: ["Voting period", "blocks", ""],
    quorum_bp: ["Holder quorum", "bp", "votes ÷ circulating supply"],
    approval_bp: ["Holder approval", "bp", "yes ÷ (yes + no)"],
    miner_approval_bp: [
      "Miner approval",
      "bp",
      "signaling blocks ÷ voting-period blocks",
    ],
    activation_delay: [
      "Activation delay",
      "blocks",
      "time for operators to upgrade",
    ],
  };

  function fmtParam(name, v) {
    var unit = (PARAM_INFO[name] || [name, ""])[1];
    if (v === null || v === undefined) return "—";
    switch (unit) {
      case "tcn":
        return TC.fmtTCN(v);
      case "tcn_kb":
        return TC.fmtTCN(v) + " per kB";
      case "tcn_kfuel":
        return TC.fmtTCN(v) + " per 1,000 fuel units";
      case "bp":
        return TC.bp(v);
      case "blocks":
        return (
          TC.fmtInt(v) +
          (Number(v) === 1 ? " block" : " blocks") +
          " ≈ " +
          TC.fmtDuration(Number(v) * 60)
        );
      case "bytes":
        return TC.fmtInt(v) + " bytes";
      case "fuel":
        return TC.fmtInt(v) + " fuel";
      default:
        return TC.fmtInt(v) + (unit ? " " + unit : "");
    }
  }

  /** Raw value with its base unit, for tooltips/secondary text. */
  function rawParam(name, v) {
    var unit = (PARAM_INFO[name] || [name, ""])[1];
    if (v === null || v === undefined) return "";
    if (unit.indexOf("tcn") === 0) return TC.fmtInt(v) + " motes";
    if (unit === "bp") return TC.fmtInt(v) + " bp";
    return "";
  }

  function badge(status) {
    var s = STATUS[status] || [status, ""];
    return '<span class="badge ' + s[1] + '">' + e(s[0]) + "</span>";
  }

  function actionText(pa) {
    switch (pa.type) {
      case "text":
        return "Text proposal";
      case "set_param":
        return (
          "Change <strong>" +
          e((PARAM_INFO[pa.param] || [pa.param])[0]) +
          "</strong> (<code>" +
          e(pa.param) +
          "</code>) to <strong>" +
          e(fmtParam(pa.param, pa.value)) +
          "</strong>"
        );
      case "software_upgrade":
        return (
          "Upgrade software to version <strong>" + e(pa.version) + "</strong>"
        );
      default:
        return e(pa.type);
    }
  }

  function pctOf(part, whole) {
    var w = BigInt(whole);
    if (w === 0n) return 0;
    return Number((BigInt(part) * 10000n) / w) / 100;
  }

  function fmtPct(p) {
    return p.toFixed(2) + "%";
  }

  /* Holder tally: stacked yes/no/abstain with labels (identity never by color alone). */
  function tally(t) {
    var total = BigInt(t.yes) + BigInt(t.no) + BigInt(t.abstain);
    var y = pctOf(t.yes, total),
      n = pctOf(t.no, total),
      a = pctOf(t.abstain, total);
    var bar =
      total === 0n
        ? '<div class="tally" aria-hidden="true"></div>'
        : '<div class="tally" role="img" aria-label="Yes ' +
          fmtPct(y) +
          ", no " +
          fmtPct(n) +
          ", abstain " +
          fmtPct(a) +
          '">' +
          (y > 0
            ? '<span class="yes" style="width:' +
              y +
              '%" title="Yes ' +
              fmtPct(y) +
              '"></span>'
            : "") +
          (n > 0
            ? '<span class="no" style="width:' +
              n +
              '%" title="No ' +
              fmtPct(n) +
              '"></span>'
            : "") +
          (a > 0
            ? '<span class="abstain" style="width:' +
              a +
              '%" title="Abstain ' +
              fmtPct(a) +
              '"></span>'
            : "") +
          "</div>";
    return (
      bar +
      '<div class="legend">' +
      '<span><i style="background:var(--yes)"></i>Yes ' +
      e(TC.fmtTCN(t.yes)) +
      "</span>" +
      '<span><i style="background:var(--no)"></i>No ' +
      e(TC.fmtTCN(t.no)) +
      "</span>" +
      '<span><i style="background:var(--abstain)"></i>Abstain ' +
      e(TC.fmtTCN(t.abstain)) +
      "</span>" +
      '<span class="faint">' +
      TC.fmtInt(t.voters) +
      " voters</span></div>"
    );
  }

  function meter(label, valueText, pct, thresholdPct, ok) {
    var p = Math.max(0, Math.min(100, pct));
    return (
      '<div class="meter-row"><div class="meter-head"><span>' +
      e(label) +
      (ok === undefined
        ? ""
        : ok
          ? ' <span class="badge ok">✓ reached</span>'
          : ' <span class="badge">pending</span>') +
      '</span><span class="v">' +
      valueText +
      "</span></div>" +
      '<div class="meter" role="progressbar" aria-label="' +
      e(label) +
      '" aria-valuemin="0" aria-valuemax="100" aria-valuenow="' +
      p.toFixed(0) +
      '">' +
      '<div class="fill" style="width:' +
      p +
      '%"></div>' +
      (thresholdPct !== null && thresholdPct !== undefined
        ? '<span class="threshold" title="required minimum" style="left:calc(' +
          Math.min(100, thresholdPct) +
          '% - 1px)"></span>'
        : "") +
      "</div></div>"
    );
  }

  function progress(p, blockTime) {
    var t = p.tally;
    if (p.projection) {
      var pr = p.projection;
      var votes = BigInt(t.yes) + BigInt(t.no) + BigInt(t.abstain);
      var quorumPct = pr.quorum_progress_bp / 100;
      var approval = pr.approval_bp / 100,
        approvalNeed = pr.approval_needed_bp / 100;
      var miner = pr.miner_approval_bp / 100,
        minerNeed = pr.miner_approval_needed_bp / 100;
      return (
        meter(
          "Holder quorum",
          e(TC.fmtTCN(String(votes))) + " of " + e(TC.fmtTCN(pr.quorum_needed)),
          quorumPct,
          null,
          pr.quorum_progress_bp >= 10000,
        ) +
        meter(
          "Holder approval (yes ÷ yes+no)",
          fmtPct(approval) + " · minimum " + fmtPct(approvalNeed),
          approval,
          approvalNeed,
          pr.approval_bp >= pr.approval_needed_bp && Number(t.yes) > 0,
        ) +
        meter(
          "Miner signaling",
          TC.fmtInt(t.miner_yes_blocks) +
            " of " +
            TC.fmtInt(t.miner_total_blocks) +
            " blocks · " +
            fmtPct(miner) +
            " · minimum " +
            fmtPct(minerNeed),
          miner,
          minerNeed,
          pr.miner_approval_bp >= pr.miner_approval_needed_bp,
        ) +
        '<p class="faint" style="margin:6px 0 0">Remaining: ' +
        TC.fmtInt(pr.blocks_left) +
        " blocks ≈ " +
        TC.fmtDuration(pr.blocks_left * blockTime) +
        " (ends at block " +
        TC.fmtInt(p.end_height) +
        ").</p>"
      );
    }
    var o = p.outcome;
    if (!o) return "";
    function yn(b) {
      return b
        ? '<span class="badge ok">✓ yes</span>'
        : '<span class="badge bad">✗ no</span>';
    }
    var minerPct = t.miner_total_blocks
      ? pctOf(t.miner_yes_blocks, t.miner_total_blocks)
      : 0;
    return (
      '<dl class="kv">' +
      "<dt>Quorum reached</dt><dd>" +
      yn(o.quorum_reached) +
      ' <span class="faint">(circulating at the end: ' +
      e(TC.fmtTCN(o.circulating_at_end)) +
      ")</span></dd>" +
      "<dt>Holders approved</dt><dd>" +
      yn(o.holders_approved) +
      "</dd>" +
      "<dt>Miners approved</dt><dd>" +
      yn(o.miners_approved) +
      ' <span class="faint">(' +
      TC.fmtInt(t.miner_yes_blocks) +
      "/" +
      TC.fmtInt(t.miner_total_blocks) +
      " blocks · " +
      fmtPct(minerPct) +
      ")</span></dd>" +
      "<dt>Deposit</dt><dd>" +
      (o.deposit_refunded ? "returned to proposer" : "burned (no quorum)") +
      "</dd>" +
      (p.activation_height
        ? "<dt>" +
          (p.status === "activated"
            ? "Activated at block"
            : "Activation at block") +
          "</dt><dd>" +
          TC.fmtInt(p.activation_height) +
          "</dd>"
        : "") +
      "</dl>"
    );
  }

  function proposalCard(p, blockTime, full) {
    var title = full
      ? "<h1>" + e(p.title) + "</h1>"
      : '<h3><a href="#/proposal/' + e(p.id) + '">' + e(p.title) + "</a></h3>";
    return (
      '<div class="card"' +
      (full ? "" : ' style="margin-bottom:16px"') +
      ">" +
      '<div class="section-title" style="margin:0 0 6px">' +
      badge(p.status) +
      '<span class="faint mono">' +
      e(TC.shortHash(p.id)) +
      "</span></div>" +
      title +
      '<p class="muted">' +
      actionText(p.action) +
      "</p>" +
      tally(p.tally) +
      progress(p, blockTime) +
      "</div>"
    );
  }

  async function pageList(tk) {
    var res = await Promise.all([
      TC.api("/governance/proposals"),
      TC.api("/governance/params"),
      TC.api("/status"),
    ]);
    if (tk !== token) return;
    var proposals = res[0],
      params = res[1],
      status = res[2];
    var bt = status.supply.target_block_time || 60;
    var voting = proposals.filter(function (p) {
      return p.status === "voting";
    });
    var pending = proposals.filter(function (p) {
      return p.status === "approved";
    });
    var past = proposals.filter(function (p) {
      return p.status !== "voting" && p.status !== "approved";
    });

    var html =
      "<h1>Governance</h1>" +
      '<p class="muted" style="max-width:760px">Holders and miners both approve on-chain changes. Current block height: <strong>' +
      TC.fmtInt(status.height) +
      "</strong>.</p>";

    html +=
      '<div class="section-title"><h2>Voting (' +
      voting.length +
      ")</h2></div>";
    html += voting.length
      ? voting
          .map(function (p) {
            return proposalCard(p, bt, false);
          })
          .join("")
      : '<div class="notice">No proposals are currently being voted on.</div>';

    if (pending.length) {
      html +=
        '<div class="section-title"><h2>Approved, awaiting activation</h2></div>' +
        pending
          .map(function (p) {
            return proposalCard(p, bt, false);
          })
          .join("");
    }

    html += '<div class="section-title"><h2>History</h2></div>';
    html += past.length
      ? '<div class="table-wrap" tabindex="0" role="region" aria-label="Data table"><table><thead><tr><th>Proposal</th><th>Status</th><th class="hide-sm">Effect</th><th class="num">Ended</th></tr></thead><tbody>' +
        past
          .map(function (p) {
            return (
              '<tr><td><a href="#/proposal/' +
              e(p.id) +
              '">' +
              e(p.title) +
              "</a></td><td>" +
              badge(p.status) +
              '</td><td class="hide-sm">' +
              actionText(p.action) +
              '</td><td class="num">block ' +
              TC.fmtInt(p.end_height) +
              "</td></tr>"
            );
          })
          .join("") +
        "</tbody></table></div>"
      : '<p class="faint">No completed proposals yet.</p>';

    html +=
      '<div class="section-title"><h2>Current parameters</h2><span class="faint">' +
      TC.fmtInt(params.voting_proposals) +
      " voting · " +
      TC.fmtInt(params.pending_activations) +
      " awaiting activation</span></div>";
    var names = Object.keys(PARAM_INFO).concat(
      Object.keys(params.current || {}).filter(function (k) {
        return !PARAM_INFO[k];
      }),
    );
    html +=
      '<div class="table-wrap" tabindex="0" role="region" aria-label="Data table"><table><thead><tr><th>Parameter</th><th class="num">Current value</th><th class="num hide-sm">Minimum</th><th class="num hide-sm">Maximum</th></tr></thead><tbody>' +
      names
        .filter(function (k) {
          return params.current && params.current[k] !== undefined;
        })
        .map(function (k) {
          var info = PARAM_INFO[k] || [k, "", ""];
          var b = (params.bounds && params.bounds[k]) || [null, null];
          var raw = rawParam(k, params.current[k]);
          return (
            "<tr><td>" +
            e(info[0]) +
            "<br><code>" +
            e(k) +
            "</code>" +
            (info[2]
              ? '<br><span class="faint">' + e(info[2]) + "</span>"
              : "") +
            '</td><td class="num">' +
            e(fmtParam(k, params.current[k])) +
            (raw ? '<br><span class="faint">' + e(raw) + "</span>" : "") +
            '</td><td class="num hide-sm">' +
            e(fmtParam(k, b[0])) +
            '</td><td class="num hide-sm">' +
            e(fmtParam(k, b[1])) +
            "</td></tr>"
          );
        })
        .join("") +
      "</tbody></table></div>";
    html +=
      '<p class="faint">Changes must stay within the minimum and maximum bounds defined by the protocol. 1 TCN = 100,000,000 motes; 100 bp = 1%.</p>';
    view.innerHTML = html;
  }

  async function pageProposal(tk, id) {
    var res = await Promise.all([
      TC.api("/governance/proposal/" + encodeURIComponent(id)),
      TC.api("/status"),
    ]);
    if (tk !== token) return;
    var p = res[0],
      status = res[1];
    var bt = status.supply.target_block_time || 60;
    var html =
      '<div class="crumbs"><a href="#/">Governance</a> › proposal</div>' +
      proposalCard(p, bt, true);
    html +=
      '<div class="section-title"><h2>Details</h2></div><div class="card"><dl class="kv">' +
      '<dt>Id</dt><dd><span class="mono break">' +
      e(p.id) +
      '</span> <button class="btn btn-small" type="button" data-copy="' +
      e(p.id) +
      '">Copy</button></dd>' +
      '<dt>Proposer</dt><dd><a class="mono break" href="explorer.html#/address/' +
      e(p.proposer) +
      '">' +
      e(p.proposer) +
      "</a></dd>" +
      "<dt>Full text</dt><dd>" +
      (/^https?:\/\//i.test(p.url)
        ? '<a href="' +
          e(p.url) +
          '" target="_blank" rel="noopener nofollow">' +
          e(p.url) +
          "</a>"
        : '<span class="faint">—</span>') +
      "</dd>" +
      '<dt>Content hash</dt><dd><span class="mono break">' +
      e(p.content_hash) +
      "</span></dd>" +
      "<dt>Effect</dt><dd>" +
      actionText(p.action) +
      (p.action.type === "software_upgrade"
        ? '<br><span class="mono break faint">' +
          e(p.action.release_hash) +
          "</span>"
        : "") +
      "</dd>" +
      "<dt>Deposit</dt><dd>" +
      e(TC.fmtTCN(p.deposit)) +
      "</dd>" +
      "<dt>Created at block</dt><dd>" +
      TC.fmtInt(p.created_height) +
      "</dd>" +
      "<dt>Voting ends</dt><dd>block " +
      TC.fmtInt(p.end_height) +
      "</dd>" +
      "<dt>Signal bit</dt><dd>" +
      e(p.signal_bit) +
      "</dd>" +
      "</dl></div>";
    if (p.status === "voting") {
      html +=
        '<div class="section-title"><h2>Vote on this proposal</h2></div><div class="grid grid-2">' +
        '<div class="card"><h3>Holders</h3><p class="muted">The voting weight stays locked until the voting period ends.</p><pre><code>thecoin-wallet gov vote ' +
        e(p.id) +
        " yes &lt;weight in TCN&gt;</code></pre></div>" +
        '<div class="card"><h3>Validators (miners)</h3><p class="muted">Using the one-line installer:</p><pre><code>sudo thecoin signal ' +
        e(p.id) +
        "\nthecoin signals          # your supported proposals\nsudo thecoin unsignal " +
        e(p.id) +
        "</code></pre>" +
        '<p class="muted" style="margin:0">Or start the node with <code>thecoind --signal ' +
        e(p.id) +
        "</code>.</p></div></div>";
    }
    view.innerHTML = html;
  }

  async function route(silent) {
    var tk = ++token;
    var parts = (location.hash || "#/").replace(/^#\/?/, "").split("/");
    if (!silent) view.innerHTML = '<p class="loading">Loading…</p>';
    try {
      if (parts[0] === "proposal" && parts[1])
        await pageProposal(tk, decodeURIComponent(parts[1]));
      else await pageList(tk);
    } catch (err) {
      if (tk !== token) return;
      view.innerHTML =
        '<div class="notice error"><strong>' +
        (err.status === 404
          ? "Proposal not found."
          : "Live proposals are temporarily unavailable.") +
        '</strong><p>Read how governance works below, or try connecting again.</p></div><div class="actions"><button type="button" class="btn btn-small" id="retry-network">Try again ↻</button><a class="text-link" href="#/">All proposals</a></div>';
      document
        .getElementById("retry-network")
        .addEventListener("click", function () {
          route();
        });
    }
  }

  window.addEventListener("hashchange", function () {
    route();
    window.scrollTo(0, 0);
  });
  route();
  setInterval(function () {
    if (!document.hidden) route(true);
  }, 60000);
})();
