/* The Coin block explorer (hash-routed single page). */
(function () {
  "use strict";

  var view = document.getElementById("view");
  var e = TC.esc;
  var refreshTimer = null;
  var routeToken = 0;

  /* ---------------- link helpers ---------------- */
  function blockLink(id, text) { return '<a class="mono" href="#/block/' + e(id) + '">' + e(text !== undefined ? text : id) + "</a>"; }
  function txLink(id, short) { return '<a class="mono" href="#/tx/' + e(id) + '">' + e(short === false ? id : TC.shortHash(id)) + "</a>"; }
  function addrLink(a, short) { return a ? '<a class="mono" href="#/address/' + e(a) + '" title="' + e(a) + '">' + e(short === false ? a : TC.shortAddr(a)) + "</a>" : "—"; }
  function contractLink(id, short) { return '<a class="mono" href="#/contract/' + e(id) + '">' + e(short === false ? id : TC.shortHash(id)) + "</a>"; }
  function proposalLink(id, short) { return '<a class="mono" href="governance.html#/proposal/' + e(id) + '">' + e(short === false ? id : TC.shortHash(id)) + "</a>"; }
  function copyBtn(text) { return ' <button class="btn btn-small" type="button" data-copy="' + e(text) + '">copiar</button>'; }
  function amount(m) { return '<span class="nowrap">' + e(TC.fmtTCN(m)) + "</span>"; }
  function kv(rows) {
    return '<dl class="kv">' + rows.filter(Boolean).map(function (r) { return "<dt>" + r[0] + "</dt><dd>" + r[1] + "</dd>"; }).join("") + "</dl>";
  }
  function memo(hexStr, text) {
    if (!hexStr) return '<span class="faint">—</span>';
    if (!text) text = hexToUtf8(hexStr);
    if (text) return "“" + e(text) + "”";
    return '<span class="mono break">0x' + e(hexStr) + "</span>";
  }
  function hexToUtf8(hexStr) {
    if (!/^([0-9a-f]{2})+$/i.test(hexStr) || typeof TextDecoder === "undefined") return null;
    var bytes = new Uint8Array(hexStr.length / 2);
    for (var i = 0; i < bytes.length; i++) bytes[i] = parseInt(hexStr.substr(i * 2, 2), 16);
    try { return new TextDecoder("utf-8", { fatal: true }).decode(bytes); } catch (err) { return null; }
  }
  function showError(err, what) {
    var msg = err && err.status === 404 ? (what || "Não encontrado") + ": " + e(err.message) : "Erro ao consultar a API: " + e(err && err.message ? err.message : err);
    view.innerHTML = '<div class="notice error">' + msg + '</div><p style="margin-top:16px"><a href="#/">← Voltar para os blocos</a></p>';
  }

  /* ---------------- action rendering ---------------- */
  var CALL_LABELS = {
    escrow_release: "Liberar escrow ao recebedor",
    escrow_refund: "Reembolsar escrow ao pagador",
    vesting_claim: "Resgatar valor liberado (vesting)",
    vesting_revoke: "Revogar vesting",
    subscription_claim: "Cobrar períodos da assinatura",
    subscription_cancel: "Cancelar assinatura",
    htlc_redeem: "Resgatar HTLC com segredo",
    htlc_refund: "Reembolsar HTLC expirado",
    multisig_deposit: "Depositar no cofre multisig",
    multisig_propose: "Propor gasto do multisig",
    multisig_approve: "Aprovar gasto do multisig",
    multisig_cancel: "Cancelar gasto do multisig",
  };
  var KIND_LABELS = { escrow: "Escrow", vesting: "Vesting", subscription: "Assinatura recorrente", htlc: "HTLC", multisig: "Multisig" };
  var CHOICE_LABELS = { yes: "Sim", no: "Não", abstain: "Abstenção" };

  function actionLabel(a) {
    switch (a.type) {
      case "transfer": return "Transferência";
      case "batch_transfer": return "Pagamento em lote";
      case "create_contract": return "Criar contrato · " + (KIND_LABELS[a.spec.kind] || a.spec.kind);
      case "call_contract": return "Contrato · " + (CALL_LABELS[a.call.call] || a.call.call);
      case "propose": return "Proposta de governança";
      case "vote": return "Voto · " + (CHOICE_LABELS[a.choice] || a.choice);
      default: return a.type;
    }
  }

  /** Short "value" column for tables. */
  function actionValue(a) {
    switch (a.type) {
      case "transfer": return amount(a.amount) + ' <span class="faint">→</span> ' + addrLink(a.to);
      case "batch_transfer": return amount(a.total) + ' <span class="faint">→ ' + a.outputs.length + " destinatários</span>";
      case "create_contract": {
        var s = a.spec;
        var v = s.amount !== undefined ? s.amount : (s.kind === "subscription" ? String(BigInt(s.amount_per_period) * BigInt(s.max_periods)) : s.initial_deposit);
        return amount(v);
      }
      case "call_contract": return a.call.amount !== undefined ? amount(a.call.amount) : contractLink(a.contract);
      case "propose": return e(a.title);
      case "vote": return amount(a.weight) + ' <span class="faint">em</span> ' + proposalLink(a.proposal);
      default: return "";
    }
  }

  function specDetails(s) {
    switch (s.kind) {
      case "escrow": return kv([
        ["Recebedor", addrLink(s.payee, false)],
        ["Árbitro", s.arbiter ? addrLink(s.arbiter, false) : '<span class="faint">nenhum</span>'],
        ["Valor", amount(s.amount)],
        ["Prazo (reembolso pelo pagador)", "após o bloco " + blockLink(s.deadline_height, TC.fmtInt(s.deadline_height))],
      ]);
      case "vesting": return kv([
        ["Beneficiário", addrLink(s.beneficiary, false)],
        ["Valor total", amount(s.amount)],
        ["Início / cliff / fim", TC.fmtInt(s.start_height) + " / " + TC.fmtInt(s.cliff_height) + " / " + TC.fmtInt(s.end_height)],
        ["Revogável", s.revocable ? "sim" : "não"],
      ]);
      case "subscription": return kv([
        ["Recebedor", addrLink(s.payee, false)],
        ["Valor por período", amount(s.amount_per_period)],
        ["Período", TC.fmtInt(s.period_blocks) + " blocos ≈ " + TC.fmtDuration(s.period_blocks * 60)],
        ["Períodos", TC.fmtInt(s.max_periods) + " (total " + TC.fmtTCN(String(BigInt(s.amount_per_period) * BigInt(s.max_periods))) + ")"],
      ]);
      case "htlc": return kv([
        ["Destinatário", addrLink(s.recipient, false)],
        ["Valor", amount(s.amount)],
        ["Hash lock (SHA-256)", '<span class="mono break">' + e(s.hash_lock) + "</span>"],
        ["Expira após o bloco", TC.fmtInt(s.timeout_height)],
      ]);
      case "multisig": return kv([
        ["Assinantes", s.signers.map(function (x) { return addrLink(x, false); }).join("<br>")],
        ["Limite", s.threshold + " de " + s.signers.length],
        ["Depósito inicial", amount(s.initial_deposit)],
      ]);
      default: return '<pre><code>' + e(JSON.stringify(s, null, 2)) + "</code></pre>";
    }
  }

  function callDetails(c) {
    var rows = [["Chamada", e(CALL_LABELS[c.call] || c.call)]];
    if (c.preimage_hex !== undefined) rows.push(["Segredo (preimage)", '<span class="mono break">' + e(c.preimage_hex) + "</span>"]);
    if (c.amount !== undefined) rows.push(["Valor", amount(c.amount)]);
    if (c.to !== undefined) rows.push(["Para", addrLink(c.to, false)]);
    if (c.memo_hex !== undefined) rows.push(["Memo", memo(c.memo_hex, null)]);
    if (c.spend_id !== undefined) rows.push(["Gasto #", e(c.spend_id)]);
    return kv(rows);
  }

  function proposalActionText(pa) {
    switch (pa.type) {
      case "text": return "Proposta de texto (decisão registrada on-chain)";
      case "set_param": return "Alterar parâmetro <code>" + e(pa.param) + "</code> para <strong>" + TC.fmtInt(pa.value) + "</strong>";
      case "software_upgrade": return "Atualização de software para a versão <strong>" + e(pa.version) + '</strong><br><span class="mono break faint">' + e(pa.release_hash) + "</span>";
      default: return e(JSON.stringify(pa));
    }
  }

  function actionDetails(a) {
    switch (a.type) {
      case "transfer": return kv([
        ["Para", addrLink(a.to, false)],
        ["Valor", amount(a.amount)],
        ["Memo", memo(a.memo_hex, a.memo_text)],
      ]);
      case "batch_transfer": return kv([
        ["Total", amount(a.total)],
        ["Memo", memo(a.memo_hex, a.memo_text)],
      ]) + '<div class="table-wrap" style="margin-top:12px"><table><thead><tr><th>#</th><th>Destinatário</th><th class="num">Valor</th></tr></thead><tbody>' +
        a.outputs.map(function (o, i) { return "<tr><td>" + (i + 1) + "</td><td>" + addrLink(o.to, false) + '</td><td class="num">' + amount(o.amount) + "</td></tr>"; }).join("") +
        "</tbody></table></div>";
      case "create_contract": return "<h3>" + e(KIND_LABELS[a.spec.kind] || a.spec.kind) + "</h3>" + specDetails(a.spec);
      case "call_contract": return kv([["Contrato", contractLink(a.contract, false)]]) + callDetails(a.call);
      case "propose": return kv([
        ["Título", e(a.title)],
        ["Texto completo", a.url ? '<a href="' + e(safeUrl(a.url)) + '" rel="noopener nofollow" target="_blank">' + e(a.url) + "</a>" : '<span class="faint">—</span>'],
        ["Hash do texto", '<span class="mono break">' + e(a.content_hash) + "</span>"],
        ["Efeito", proposalActionText(a.action)],
      ]);
      case "vote": return kv([
        ["Proposta", proposalLink(a.proposal, false)],
        ["Escolha", e(CHOICE_LABELS[a.choice] || a.choice)],
        ["Peso (moedas travadas)", amount(a.weight)],
      ]);
      default: return "<pre><code>" + e(JSON.stringify(a, null, 2)) + "</code></pre>";
    }
  }

  function safeUrl(u) {
    return /^https?:\/\//i.test(u) ? u : "#";
  }

  function txRows(txs, opts) {
    opts = opts || {};
    if (!txs.length) return '<p class="faint">Nenhuma transação.</p>';
    return '<div class="table-wrap"><table><thead><tr>' +
      "<th>Txid</th>" + (opts.showBlock ? "<th>Bloco</th>" : "") + '<th>Tipo</th><th class="hide-sm">De</th><th>Valor / detalhe</th><th class="num hide-sm">Taxa</th></tr></thead><tbody>' +
      txs.map(function (t) {
        var where = t.in_mempool ? '<span class="badge warn">mempool</span>' : (t.block_height !== null && t.block_height !== undefined ? blockLink(t.block_height, TC.fmtInt(t.block_height)) : "—");
        return "<tr><td>" + txLink(t.txid) + "</td>" + (opts.showBlock ? "<td>" + where + "</td>" : "") +
          "<td>" + e(actionLabel(t.action)) + '</td><td class="hide-sm">' + addrLink(t.sender) + "</td><td>" + actionValue(t.action) +
          '</td><td class="num hide-sm">' + e(TC.fmtTCN(t.fee)) + "</td></tr>";
      }).join("") + "</tbody></table></div>";
  }

  /* ---------------- pages ---------------- */

  async function pageHome(token, before) {
    var q = "/blocks?limit=20" + (before !== undefined ? "&before=" + before : "");
    var results = await Promise.all([TC.api(q), TC.api("/status")]);
    if (token !== routeToken) return;
    var blocks = results[0], s = results[1];
    var html = '<div class="grid grid-4">' +
      '<div class="card stat"><span class="label">Altura</span><span class="value">' + TC.fmtInt(s.height) + '</span><span class="sub">' + e(s.network) + (s.syncing ? " · sincronizando" : "") + "</span></div>" +
      '<div class="card stat"><span class="label">Dificuldade</span><span class="value">' + e(TC.fmtDifficulty(s.difficulty)) + '</span><span class="sub">' + e(TC.fmtHashrate(s.hashrate_estimate)) + "</span></div>" +
      '<div class="card stat"><span class="label">Emitidos</span><span class="value">' + e(TC.fmtTCN(s.supply.emitted, { maxDecimals: 0 })) + '</span><span class="sub">recompensa ' + e(TC.fmtTCN(s.supply.current_block_reward)) + "</span></div>" +
      '<div class="card stat"><span class="label">Mempool</span><span class="value"><a href="#/mempool">' + TC.fmtInt(s.mempool_txs) + ' tx</a></span><span class="sub">' + e(TC.fmtBytes(s.mempool_bytes)) + "</span></div>" +
      "</div>";
    html += '<div class="section-title"><h2>' + (before === undefined ? "Últimos blocos" : "Blocos anteriores a " + TC.fmtInt(before)) + "</h2>" +
      (before === undefined ? '<span class="faint">atualiza a cada 15 s</span>' : '<a href="#/">← mais recentes</a>') + "</div>";
    html += '<div class="table-wrap"><table><thead><tr><th>Altura</th><th>Hash</th><th>Idade</th><th class="num">Txs</th><th class="hide-sm">Minerador</th><th class="num hide-sm">Tamanho</th></tr></thead><tbody>' +
      blocks.map(function (b) {
        return "<tr><td>" + blockLink(b.height, TC.fmtInt(b.height)) + "</td><td>" + blockLink(b.hash, TC.shortHash(b.hash)) +
          '</td><td class="nowrap" title="' + e(TC.fmtDate(b.timestamp)) + '">' + e(TC.timeAgo(b.timestamp)) + '</td><td class="num">' + b.tx_count +
          '</td><td class="hide-sm">' + addrLink(b.miner) + '</td><td class="num hide-sm">' + e(TC.fmtBytes(b.size)) + "</td></tr>";
      }).join("") + "</tbody></table></div>";
    var lowest = blocks.length ? blocks[blocks.length - 1].height : 0;
    if (lowest > 0) html += '<div class="pager"><a class="btn" href="#/blocks/' + lowest + '">Blocos anteriores →</a></div>';
    view.innerHTML = html;
    if (before === undefined) {
      refreshTimer = setTimeout(function () { if (token === routeToken) pageHome(token).catch(function () {}); }, 15000);
    }
  }

  async function pageBlock(token, id) {
    var b = await TC.api("/block/" + encodeURIComponent(id));
    if (token !== routeToken) return;
    var html = '<div class="crumbs"><a href="#/">Blocos</a> › bloco ' + TC.fmtInt(b.height) + "</div>";
    html += '<div class="section-title"><h1>Bloco ' + TC.fmtInt(b.height) + "</h1><span>" +
      (b.height > 0 ? '<a class="btn btn-small" href="#/block/' + (b.height - 1) + '">← anterior</a> ' : "") +
      '<a class="btn btn-small" href="#/block/' + (b.height + 1) + '">próximo →</a></span></div>';
    html += '<div class="card">' + kv([
      ["Hash", '<span class="mono break">' + e(b.hash) + "</span>" + copyBtn(b.hash)],
      ["Confirmações", b.confirmations > 0 ? TC.fmtInt(b.confirmations) : '<span class="badge warn">fora da cadeia principal</span>'],
      ["Data", e(TC.fmtDate(b.timestamp)) + ' <span class="faint">(' + e(TC.timeAgo(b.timestamp)) + ")</span>"],
      ["Minerador", addrLink(b.miner, false)],
      ["Recompensa", amount(b.subsidy) + ' <span class="faint">+ taxas</span> ' + amount(b.fees)],
      ["Transações", TC.fmtInt(b.tx_count)],
      ["Tamanho", e(TC.fmtBytes(b.size))],
      ["Bloco anterior", b.height > 0 ? blockLink(b.prev_hash) : '<span class="faint">gênese</span>'],
      ["Dificuldade", e(TC.fmtDifficulty(b.difficulty))],
      ["Alvo (target)", '<span class="mono break">' + e(b.target) + "</span>"],
      ["Nonce", '<span class="mono">' + e(b.nonce) + "</span>"],
      ["Raiz das transações", '<span class="mono break">' + e(b.tx_root) + "</span>"],
      ["Raiz de estado", '<span class="mono break">' + e(b.state_root) + "</span>"],
      ["Sinalização", b.signal ? '<span class="mono">0x' + b.signal.toString(16) + "</span> (bits de governança)" : '<span class="faint">nenhuma</span>'],
      ["Versão", e(b.version)],
    ]) + "</div>";
    html += '<div class="section-title"><h2>Transações</h2></div>' + txRows(b.txs);
    view.innerHTML = html;
  }

  async function pageTx(token, id) {
    var t = await TC.api("/tx/" + encodeURIComponent(id));
    if (token !== routeToken) return;
    var status = t.in_mempool ? '<span class="badge warn">aguardando no mempool</span>'
      : '<span class="badge ok">confirmada</span> ' + TC.fmtInt(t.confirmations) + " confirmações";
    var html = '<div class="crumbs"><a href="#/">Blocos</a> › transação</div><h1>Transação</h1>';
    html += '<div class="card">' + kv([
      ["Txid", '<span class="mono break">' + e(t.txid) + "</span>" + copyBtn(t.txid)],
      ["Status", status],
      t.block_height !== null && t.block_height !== undefined ? ["Bloco", blockLink(t.block_height, TC.fmtInt(t.block_height))] : null,
      ["Tipo", e(actionLabel(t.action))],
      ["Remetente", addrLink(t.sender, false)],
      ["Nonce", TC.fmtInt(t.nonce)],
      ["Taxa", amount(t.fee) + ' <span class="faint">(' + TC.fmtInt(t.size) + " bytes)</span>"],
      ["Expira", t.expiry_height ? "após o bloco " + TC.fmtInt(t.expiry_height) : '<span class="faint">não expira</span>'],
      t.created ? [t.action.type === "propose" ? "Proposta criada" : "Contrato criado", t.action.type === "propose" ? proposalLink(t.created, false) : contractLink(t.created, false)] : null,
    ]) + "</div>";
    html += '<div class="section-title"><h2>' + e(actionLabel(t.action)) + '</h2></div><div class="card">' + actionDetails(t.action) + "</div>";
    view.innerHTML = html;
  }

  async function pageAddress(token, addr) {
    var acc = await TC.api("/address/" + encodeURIComponent(addr));
    if (token !== routeToken) return;
    var html = '<div class="crumbs"><a href="#/">Blocos</a> › endereço</div><h1>Endereço</h1>';
    html += '<p class="mono break">' + e(acc.address) + copyBtn(acc.address) + "</p>";
    html += '<div class="grid grid-4">' +
      '<div class="card stat"><span class="label">Saldo</span><span class="value">' + e(TC.fmtTCN(acc.balance)) + "</span></div>" +
      '<div class="card stat"><span class="label">Disponível</span><span class="value">' + e(TC.fmtTCN(acc.spendable)) + "</span></div>" +
      '<div class="card stat"><span class="label">Travado em votos</span><span class="value">' + e(TC.fmtTCN(acc.locked)) + '</span><span class="sub">' + (acc.locked > 0 ? "até o bloco " + TC.fmtInt(acc.locked_until) : "—") + "</span></div>" +
      '<div class="card stat"><span class="label">Recompensas maturando</span><span class="value">' + e(TC.fmtTCN(acc.immature)) + "</span></div>" +
      "</div>";
    html += '<div class="card" style="margin-top:16px">' + kv([
      ["Nonce (tx enviadas)", TC.fmtInt(acc.nonce)],
      ["Próximo nonce", TC.fmtInt(acc.next_nonce)],
      ["Tx pendentes no mempool", TC.fmtInt(acc.mempool_txs)],
    ]) + "</div>";
    html += '<div class="section-title"><h2>Histórico</h2></div><div id="history"><p class="loading">Carregando histórico…</p></div><div class="pager" id="more"></div>';
    view.innerHTML = html;
    await loadHistory(token, acc.address, null, []);
  }

  /*
   * The history endpoint pages with cursor "height:position" (exclusive).
   * TxView does not carry the in-block position, so we look it up in the
   * block of the last row.
   */
  async function loadHistory(token, addr, cursor, acc) {
    var limit = 25;
    var rows;
    try {
      rows = await TC.api("/address/" + encodeURIComponent(addr) + "/txs?limit=" + limit + (cursor ? "&cursor=" + cursor : ""));
    } catch (err) {
      if (token !== routeToken) return;
      document.getElementById("history").innerHTML = '<div class="notice error">' + e(err.message) + "</div>";
      return;
    }
    if (token !== routeToken) return;
    var all = acc.concat(rows);
    document.getElementById("history").innerHTML = txRows(all, { showBlock: true });
    var more = document.getElementById("more");
    more.innerHTML = "";
    var confirmed = rows.filter(function (t) { return !t.in_mempool && t.block_hash; });
    if (confirmed.length >= limit) {
      var last = confirmed[confirmed.length - 1];
      var btn = document.createElement("button");
      btn.className = "btn";
      btn.textContent = "Carregar mais";
      btn.onclick = async function () {
        btn.disabled = true;
        btn.textContent = "Carregando…";
        try {
          var blk = await TC.api("/block/" + last.block_hash);
          var pos = blk.txs.findIndex(function (t) { return t.txid === last.txid; });
          await loadHistory(token, addr, last.block_height + ":" + Math.max(0, pos), all);
        } catch (err) {
          btn.textContent = "Erro: " + err.message;
        }
      };
      more.appendChild(btn);
    }
  }

  function contractState(c) {
    var s = c.state;
    switch (s.kind) {
      case "escrow": return kv([
        ["Pagador", addrLink(s.payer, false)],
        ["Recebedor", addrLink(s.payee, false)],
        ["Árbitro", s.arbiter ? addrLink(s.arbiter, false) : '<span class="faint">nenhum</span>'],
        ["Prazo", "pagador pode reembolsar após o bloco " + TC.fmtInt(s.deadline_height)],
      ]);
      case "vesting": {
        var total = BigInt(s.total), vested = BigInt(s.vested_now), claimed = BigInt(s.claimed);
        var pct = total > 0n ? Number(vested * 10000n / total) / 100 : 0;
        return kv([
          ["Beneficiário", addrLink(s.beneficiary, false)],
          ["Total", amount(s.total)],
          ["Liberado até agora", amount(s.vested_now) + " (" + pct.toFixed(2).replace(".", ",") + "%)"],
          ["Já resgatado", amount(s.claimed)],
          ["Disponível para resgate", amount(String(vested > claimed ? vested - claimed : 0n))],
          ["Início / cliff / fim", TC.fmtInt(s.start_height) + " / " + TC.fmtInt(s.cliff_height) + " / " + TC.fmtInt(s.end_height)],
          ["Revogável", s.revocable ? "sim" : "não"],
        ]) + meter("Liberado", pct, null);
      }
      case "subscription": return kv([
        ["Pagador", addrLink(s.payer, false)],
        ["Recebedor", addrLink(s.payee, false)],
        ["Valor por período", amount(s.amount_per_period)],
        ["Período", TC.fmtInt(s.period_blocks) + " blocos ≈ " + TC.fmtDuration(s.period_blocks * 60)],
        ["Períodos cobrados", TC.fmtInt(s.claimed_periods) + " de " + TC.fmtInt(s.max_periods)],
        ["Cobráveis agora", TC.fmtInt(s.claimable_periods_now)],
        ["Início", "bloco " + TC.fmtInt(s.start_height)],
      ]);
      case "htlc": return kv([
        ["Remetente", addrLink(s.sender, false)],
        ["Destinatário", addrLink(s.recipient, false)],
        ["Hash lock (SHA-256)", '<span class="mono break">' + e(s.hash_lock) + "</span>"],
        ["Expira após o bloco", TC.fmtInt(s.timeout_height)],
      ]);
      case "multisig": return kv([
        ["Assinantes", s.signers.map(function (x) { return addrLink(x, false); }).join("<br>")],
        ["Limite", s.threshold + " de " + s.signers.length + " aprovações"],
        ["Próximo id de gasto", TC.fmtInt(s.next_spend_id)],
      ]) + '<h3 style="margin-top:18px">Gastos pendentes</h3>' + (s.pending.length ?
        '<div class="table-wrap"><table><thead><tr><th>#</th><th>Para</th><th class="num">Valor</th><th>Aprovações</th><th class="hide-sm">Criado</th></tr></thead><tbody>' +
        s.pending.map(function (p) {
          return "<tr><td>" + p.id + "</td><td>" + addrLink(p.to) + '</td><td class="num">' + amount(p.amount) + "</td><td>" + p.approvals.length + "/" + s.threshold + " " +
            p.approvals.map(function (a) { return addrLink(a); }).join(", ") + '</td><td class="hide-sm">bloco ' + TC.fmtInt(p.created_height) + "</td></tr>";
        }).join("") + "</tbody></table></div>" : '<p class="faint">Nenhum gasto pendente.</p>');
      default: return "<pre><code>" + e(JSON.stringify(s, null, 2)) + "</code></pre>";
    }
  }

  function meter(label, pct, thresholdPct) {
    var p = Math.max(0, Math.min(100, pct));
    return '<div class="meter-row"><div class="meter-head"><span>' + e(label) + '</span><span class="v">' + pct.toFixed(2).replace(".", ",") + "%</span></div>" +
      '<div class="meter" role="progressbar" aria-valuemin="0" aria-valuemax="100" aria-valuenow="' + p.toFixed(0) + '"><div class="fill" style="width:' + p + '%"></div>' +
      (thresholdPct !== null ? '<span class="threshold" style="left:calc(' + thresholdPct + '% - 1px)"></span>' : "") + "</div></div>";
  }

  async function pageContract(token, id) {
    var c;
    try {
      c = await TC.api("/contract/" + encodeURIComponent(id));
    } catch (err) {
      if (err.status === 404) {
        if (token !== routeToken) return;
        view.innerHTML = '<div class="crumbs"><a href="#/">Blocos</a> › contrato</div><h1>Contrato</h1><p class="mono break">' + e(id) + '</p>' +
          '<div class="notice">Este contrato não existe no estado atual. Contratos finalizados (saldo zero) são removidos do estado para mantê-lo compacto — o histórico das chamadas continua disponível nas transações dos participantes.</div>';
        return;
      }
      throw err;
    }
    if (token !== routeToken) return;
    var html = '<div class="crumbs"><a href="#/">Blocos</a> › contrato</div><h1>Contrato · ' + e(KIND_LABELS[c.state.kind] || c.state.kind) + "</h1>";
    html += '<div class="card">' + kv([
      ["Id", '<span class="mono break">' + e(c.id) + "</span>" + copyBtn(c.id)],
      ["Criador", addrLink(c.creator, false)],
      ["Criado no bloco", blockLink(c.created_height, TC.fmtInt(c.created_height))],
      ["Saldo do contrato", amount(c.balance)],
    ]) + "</div>";
    html += '<div class="section-title"><h2>Estado</h2></div><div class="card">' + contractState(c) + "</div>";
    view.innerHTML = html;
  }

  async function pageMempool(token) {
    var m = await TC.api("/mempool");
    if (token !== routeToken) return;
    var html = '<div class="crumbs"><a href="#/">Blocos</a> › mempool</div><div class="section-title"><h1>Mempool</h1><span class="faint">atualiza a cada 15 s</span></div>';
    html += '<p class="muted">' + TC.fmtInt(m.count) + " transações aguardando (" + e(TC.fmtBytes(m.bytes)) + "). Mostrando as de maior taxa por byte.</p>";
    html += txRows(m.txs);
    view.innerHTML = html;
    refreshTimer = setTimeout(function () { if (token === routeToken) pageMempool(token).catch(function () {}); }, 15000);
  }

  async function exists(path) {
    try { await TC.api(path); return true; } catch (err) { if (err.status === 404 || err.status === 400) return false; throw err; }
  }

  async function search(qRaw) {
    var q = qRaw.trim();
    if (!q) return;
    if (/^\d+$/.test(q)) { location.hash = "#/block/" + q; return; }
    if (TC.isAddress(q)) { location.hash = "#/address/" + q.toLowerCase(); return; }
    if (TC.isHash(q)) {
      var h = q.toLowerCase();
      view.innerHTML = '<p class="loading">Procurando ' + e(TC.shortHash(h)) + "…</p>";
      if (await exists("/block/" + h)) { location.hash = "#/block/" + h; return; }
      if (await exists("/tx/" + h)) { location.hash = "#/tx/" + h; return; }
      if (await exists("/contract/" + h)) { location.hash = "#/contract/" + h; return; }
      if (await exists("/governance/proposal/" + h)) { location.href = "governance.html#/proposal/" + h; return; }
      view.innerHTML = '<div class="notice error">Nada encontrado para <span class="mono break">' + e(h) + '</span>. Contratos finalizados são removidos do estado.</div>';
      return;
    }
    view.innerHTML = '<div class="notice error">Formato não reconhecido. Use uma altura de bloco, um hash de 64 caracteres hexadecimais ou um endereço <span class="mono">tc1…</span>.</div>';
  }

  /* ---------------- router ---------------- */
  async function route() {
    clearTimeout(refreshTimer);
    var token = ++routeToken;
    var parts = (location.hash || "#/").replace(/^#\/?/, "").split("/");
    var kind = parts[0] || "", arg = decodeURIComponent(parts.slice(1).join("/"));
    if (kind !== "blocks" && kind !== "") window.scrollTo(0, 0);
    view.innerHTML = '<p class="loading">Carregando…</p>';
    try {
      switch (kind) {
        case "": await pageHome(token); break;
        case "blocks": await pageHome(token, Number(arg)); break;
        case "block": await pageBlock(token, arg); break;
        case "tx": await pageTx(token, arg); break;
        case "address": await pageAddress(token, arg); break;
        case "contract": await pageContract(token, arg); break;
        case "mempool": await pageMempool(token); break;
        case "search": await search(arg); break;
        default: view.innerHTML = '<div class="notice error">Página não encontrada.</div>';
      }
    } catch (err) {
      if (token === routeToken) showError(err);
    }
  }

  document.getElementById("search").addEventListener("submit", function (ev) {
    ev.preventDefault();
    var q = document.getElementById("q").value;
    search(q).catch(function (err) { showError(err); });
  });
  window.addEventListener("hashchange", route);
  route();
})();
