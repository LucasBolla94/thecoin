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
  function mono(s) { return '<span class="mono break">' + e(s) + "</span>"; }
  function faint(s) { return '<span class="faint">' + e(s) + "</span>"; }
  function kv(rows) {
    return '<dl class="kv">' + rows.filter(Boolean).map(function (r) { return "<dt>" + r[0] + "</dt><dd>" + r[1] + "</dd>"; }).join("") + "</dl>";
  }
  function memo(hexStr, text) {
    if (!hexStr) return faint("—");
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
  function has(v) { return v !== null && v !== undefined; }

  /** Name declared in a TCCL source ("contract Name"), or null. */
  function contractName(source) {
    var m = /^[ \t]*contract[ \t]+([A-Za-z_][A-Za-z0-9_]*)/m.exec(String(source || ""));
    return m ? m[1] : null;
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
    multisig_close: "Encerrar cofre multisig (devolve o depósito)",
  };
  var KIND_LABELS = { escrow: "Escrow", vesting: "Vesting", subscription: "Assinatura recorrente", htlc: "HTLC", multisig: "Multisig" };
  var CHOICE_LABELS = { yes: "Sim", no: "Não", abstain: "Abstenção" };
  var FN_KIND_LABELS = { init: "init", action: "ação", view: "consulta (view)" };

  function actionLabel(a) {
    switch (a.type) {
      case "transfer": return "Transferência";
      case "batch_transfer": return "Pagamento em lote";
      case "create_contract": return "Criar contrato · " + (KIND_LABELS[a.spec.kind] || a.spec.kind);
      case "call_contract": return "Contrato · " + (CALL_LABELS[a.call.call] || a.call.call);
      case "propose": return "Proposta de governança";
      case "vote": return "Voto · " + (CHOICE_LABELS[a.choice] || a.choice);
      case "deploy": return "Publicar contrato TCCL";
      case "invoke": return "Chamar contrato TCCL";
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
      case "deploy": {
        var name = contractName(a.source);
        return '<span class="mono">' + e(name || "contrato") + "</span>" + (Number(a.value) > 0 ? " · " + amount(a.value) : "");
      }
      case "invoke":
        return '<span class="mono">' + e(a.function) + "()</span>" + (Number(a.value) > 0 ? " · " + amount(a.value) : "") +
          ' <span class="faint">em</span> ' + addrLink(a.contract);
      default: return "";
    }
  }

  function specDetails(s) {
    switch (s.kind) {
      case "escrow": return kv([
        ["Recebedor", addrLink(s.payee, false)],
        ["Árbitro", s.arbiter ? addrLink(s.arbiter, false) : faint("nenhum")],
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
        ["Hash lock (SHA-256)", mono(s.hash_lock)],
        ["Expira após o bloco", TC.fmtInt(s.timeout_height)],
      ]);
      case "multisig": return kv([
        ["Assinantes", s.signers.map(function (x) { return addrLink(x, false); }).join("<br>")],
        ["Limite", e(s.threshold) + " de " + s.signers.length],
        ["Depósito inicial", amount(s.initial_deposit)],
      ]);
      default: return "<pre><code>" + e(JSON.stringify(s, null, 2)) + "</code></pre>";
    }
  }

  function callDetails(c) {
    var rows = [["Chamada", e(CALL_LABELS[c.call] || c.call)]];
    if (c.preimage_hex !== undefined) rows.push(["Segredo (preimage)", mono(c.preimage_hex)]);
    if (c.amount !== undefined) rows.push(["Valor", amount(c.amount)]);
    if (c.to !== undefined) rows.push(["Para", addrLink(c.to, false)]);
    if (c.memo_hex !== undefined) rows.push(["Memo", memo(c.memo_hex, c.memo_text)]);
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

  function argList(args) {
    if (!args || !args.length) return faint("nenhum");
    return '<ol class="arglist">' + args.map(function (x) { return '<li><span class="mono break">' + e(x) + "</span></li>"; }).join("") + "</ol>";
  }

  function actionDetails(t) {
    var a = t.action;
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
        ["Texto completo", a.url ? '<a href="' + e(safeUrl(a.url)) + '" rel="noopener nofollow" target="_blank">' + e(a.url) + "</a>" : faint("—")],
        ["Hash do texto", mono(a.content_hash)],
        ["Efeito", proposalActionText(a.action)],
      ]);
      case "vote": return kv([
        ["Proposta", proposalLink(a.proposal, false)],
        ["Escolha", e(CHOICE_LABELS[a.choice] || a.choice)],
        ["Peso (moedas travadas)", amount(a.weight)],
      ]);
      case "deploy": {
        var name = contractName(a.source);
        var where = t.program ? addrLink(t.program, false) + (t.in_mempool ? ' <span class="faint">(endereço previsto)</span>' : "")
          : (t.block_height !== null && t.block_height !== undefined ? faint("nenhum — a publicação falhou") : faint("—"));
        return kv([
          ["Contrato", '<span class="mono">' + e(name || "—") + "</span>"],
          ["Endereço do contrato", where],
          ["Código-fonte", TC.fmtInt(TC.byteLength(a.source)) + " bytes"],
          ["Hash do código-fonte", mono(a.source_hash)],
          ["Argumentos de init", argList(a.init_args)],
          ["Valor enviado", amount(a.value)],
          ["Combustível máximo", TC.fmtInt(a.max_fuel)],
          ["Depósito máximo aceito", amount(a.max_deposit)],
        ]) + '<details class="source"><summary>Ver código-fonte TCCL</summary><pre><code>' + e(a.source) + "</code></pre></details>";
      }
      case "invoke": return kv([
        ["Contrato", addrLink(a.contract, false)],
        ["Função", '<span class="mono">' + e(a.function) + "</span>"],
        ["Argumentos", argList(a.args)],
        ["Valor enviado", amount(a.value)],
        ["Combustível máximo", TC.fmtInt(a.max_fuel)],
        ["Depósito máximo aceito", amount(a.max_deposit)],
      ]);
      default: return "<pre><code>" + e(JSON.stringify(a, null, 2)) + "</code></pre>";
    }
  }

  function safeUrl(u) {
    return /^https?:\/\//i.test(u) ? u : "#";
  }

  function isProgramTx(t) { return t.action.type === "deploy" || t.action.type === "invoke"; }

  /** Small badges next to the type: failure, replaceable, double spend. */
  function txBadges(t) {
    var out = "";
    if (t.success === false) out += ' <span class="badge bad">falhou</span>';
    if (t.replaceable) out += ' <span class="badge warn" title="O remetente pode substituí-la por outra com taxa maior enquanto não for confirmada">substituível</span>';
    if (t.conflict) out += ' <span class="badge bad" title="Outra transação com o mesmo remetente e nonce foi vista">gasto duplo?</span>';
    return out;
  }

  function txRows(txs, opts) {
    opts = opts || {};
    if (!txs.length) return '<p class="faint">Nenhuma transação.</p>';
    return '<div class="table-wrap"><table><thead><tr>' +
      "<th>Txid</th>" + (opts.showBlock ? "<th>Bloco</th>" : "") + '<th>Tipo</th><th class="hide-sm">De</th><th>Valor / detalhe</th><th class="num hide-sm">Taxa</th></tr></thead><tbody>' +
      txs.map(function (t) {
        var where = t.in_mempool ? '<span class="badge warn">mempool</span>' : (has(t.block_height) ? blockLink(t.block_height, TC.fmtInt(t.block_height)) : "—");
        return "<tr><td>" + txLink(t.txid) + "</td>" + (opts.showBlock ? "<td>" + where + "</td>" : "") +
          "<td>" + e(actionLabel(t.action)) + txBadges(t) + '</td><td class="hide-sm">' + addrLink(t.sender) + "</td><td>" + actionValue(t.action) +
          '</td><td class="num hide-sm">' + e(TC.fmtTCN(t.fee)) + "</td></tr>";
      }).join("") + "</tbody></table></div>";
  }

  function logsTable(logs) {
    if (!logs || !logs.length) return '<p class="faint">Nenhum evento emitido.</p>';
    return '<div class="table-wrap"><table><thead><tr><th>#</th><th>Evento</th><th>Campos</th><th class="hide-sm">Contrato</th></tr></thead><tbody>' +
      logs.map(function (l, i) {
        var fields = (l.fields || []).map(function (f) {
          return '<div><span class="faint">' + e(f[0]) + ':</span> <span class="mono break">' + e(f[1]) + "</span></div>";
        }).join("");
        return "<tr><td>" + (i + 1) + '</td><td class="mono">' + e(l.event) + "</td><td>" + (fields || faint("—")) + '</td><td class="hide-sm">' + addrLink(l.contract) + "</td></tr>";
      }).join("") + "</tbody></table></div>";
  }

  /* ---------------- network panels (fees, confirmations, alerts) ---------------- */

  var PRIORITY_ROWS = [
    ["low_bp", "Baixa", "taxa mínima — boa quando os blocos não estão cheios"],
    ["normal_bp", "Normal", "mínimo + 25% — padrão da carteira, resiste a um bloco de alta"],
    ["high_bp", "Alta", "à frente da maioria das transações na fila"],
    ["urgent_bp", "Urgente", "próximo bloco com probabilidade muito alta"],
  ];

  function feesPanel(f) {
    var cong = Number(f.congestion_bp);
    var congBadge = cong > 10000 ? '<span class="badge warn">congestionado · ' + e(TC.fmtMultiplier(cong)) + "</span>"
      : '<span class="badge ok">sem congestionamento · 1,00×</span>';
    var rows = PRIORITY_ROWS.map(function (p) {
      var bpv = f.priority[p[0]];
      return "<tr><td><strong>" + e(p[1]) + '</strong><br><span class="faint">' + e(p[2]) + '</span></td><td class="num">' + e(TC.fmtMultiplier(bpv)) +
        '</td><td class="num">' + amount(TC.applyBp(f.typical_transfer_fee, bpv)) + "</td></tr>";
    }).join("");
    return '<div class="section-title" style="margin-top:0"><h3>Taxas agora</h3>' + congBadge + "</div>" +
      '<div class="table-wrap"><table><thead><tr><th>Prioridade</th><th class="num">× mínimo</th><th class="num">Transferência (160 bytes)</th></tr></thead><tbody>' + rows + "</tbody></table></div>" +
      kv([
        ["Taxa base", amount(f.base_fee)],
        ["Por 1.000 bytes", amount(f.fee_per_kb)],
        ["Por 1.000 de combustível", amount(f.fee_per_kfuel)],
        ["Depósito de armazenamento", amount(f.storage_deposit_per_kb) + ' <span class="faint">por kB (reembolsável)</span>'],
        ["Fila (mempool)", TC.fmtInt(f.mempool_txs) + " tx · " + e(TC.fmtBytes(f.mempool_bytes))],
      ]) +
      '<p class="faint" style="margin:10px 0 0">Mínimo = (base + tamanho × taxa por kB + combustível reservado × taxa por mil) × congestionamento. O congestionamento sobe até 12,5% por bloco quando os blocos passam de 50% de uso (1× a 1000×) e a parte extra é <strong>queimada</strong>. Carteira: <code>--priority low|normal|high|urgent</code>.</p>';
  }

  function securityForm() {
    return '<h3>Quantas confirmações?</h3>' +
      '<p class="muted" style="margin-bottom:10px">Quanto maior o valor recebido, mais blocos você deve esperar antes de entregar o produto.</p>' +
      '<form id="sec-form" class="inline-form" autocomplete="off">' +
      '<label for="sec-amount">Valor recebido (TCN)</label>' +
      '<div class="row"><input id="sec-amount" type="text" inputmode="decimal" value="100" spellcheck="false">' +
      '<button class="btn btn-primary" type="submit">Calcular</button></div></form>' +
      '<div id="sec-out" aria-live="polite"></div>';
  }

  function bindSecurityForm() {
    var form = document.getElementById("sec-form");
    if (!form) return;
    var out = document.getElementById("sec-out");
    function run() {
      var motes = TC.parseTCN(document.getElementById("sec-amount").value);
      if (motes === null) {
        out.innerHTML = '<p class="notice error" style="margin-top:12px">Digite um valor em TCN, por exemplo 250 ou 12,5.</p>';
        return;
      }
      out.innerHTML = '<p class="loading">Consultando…</p>';
      TC.api("/security?amount=" + motes).then(function (s) {
        var atStake = String(BigInt(s.value_per_block) * BigInt(s.confirmations));
        out.innerHTML = '<div class="sec-result">' +
          '<div class="stat"><span class="label">Espere</span><span class="value">' + TC.fmtInt(s.confirmations) + (Number(s.confirmations) === 1 ? " confirmação" : " confirmações") + "</span>" +
          '<span class="sub">≈ ' + e(TC.fmtDuration(Math.max(1, Number(s.minutes)) * 60)) + " para " + e(TC.fmtTCN(s.amount)) + "</span></div>" +
          '<p class="muted" style="margin:10px 0 0">Para desfazer ' + TC.fmtInt(s.confirmations) + " bloco(s), um atacante precisa refazer a prova de trabalho deles e abrir mão de ≈ " +
          e(TC.fmtTCN(atStake, { maxDecimals: 2 })) + " em recompensas (" + e(TC.fmtTCN(s.value_per_block, { maxDecimals: 4 })) + " por bloco) — pelo menos o dobro do valor. Hashrate da rede agora: " +
          e(TC.fmtHashrate(s.network_hashrate)) + ". Reorganizações com mais de 720 blocos nunca são aceitas.</p></div>";
      }).catch(function (err) {
        out.innerHTML = '<p class="notice error" style="margin-top:12px">Erro: ' + e(err.message || err) + "</p>";
      });
    }
    form.addEventListener("submit", function (ev) { ev.preventDefault(); run(); });
  }

  function alertsPanel(list) {
    var head = '<div class="section-title"><h2>Alertas de gasto duplo</h2><span class="faint">vistos por este nó</span></div>';
    if (!list.length) return head + '<p class="faint">Nenhuma tentativa de gasto duplo vista recentemente. Quando alguém envia duas transações diferentes com o mesmo nonce, ela aparece aqui.</p>';
    return head + '<div class="notice error" style="margin-bottom:12px">Estes remetentes tentaram gastar as mesmas moedas duas vezes. Se você recebeu um pagamento deles, espere confirmações antes de entregar.</div>' +
      '<div class="table-wrap"><table><thead><tr><th>Quando</th><th>Remetente</th><th class="num hide-sm">Nonce</th><th>Primeira</th><th>Segunda</th></tr></thead><tbody>' +
      list.slice().reverse().map(function (a) {
        return '<tr><td class="nowrap" title="' + e(TC.fmtDate(a.seen_at)) + '">' + e(TC.timeAgo(a.seen_at)) + "</td><td>" + addrLink(a.sender) + '</td><td class="num hide-sm">' + TC.fmtInt(a.nonce) +
          "</td><td>" + txLink(a.first) + "</td><td>" + txLink(a.second) + "</td></tr>";
      }).join("") + "</tbody></table></div>";
  }

  /* ---------------- pages ---------------- */

  async function pageHome(token) {
    view.innerHTML =
      '<div id="h-stats" class="grid grid-4"><p class="loading">Carregando…</p></div>' +
      '<div class="grid grid-2" style="margin-top:16px">' +
      '<div class="card" id="h-fees"><h3>Taxas agora</h3><p class="loading">Carregando…</p></div>' +
      '<div class="card" id="h-sec">' + securityForm() + "</div></div>" +
      '<div id="h-alerts"></div>' +
      '<div class="section-title"><h2>Últimos blocos</h2><span class="faint">atualiza a cada 15 s</span></div>' +
      '<div id="h-blocks"><p class="loading">Carregando…</p></div><div class="pager" id="h-pager"></div>';
    bindSecurityForm();
    await refreshHome(token);
    (function loop() {
      if (token !== routeToken) return;
      refreshTimer = setTimeout(function () {
        if (token !== routeToken) return;
        refreshHome(token).catch(function () {}).then(loop);
      }, 15000);
    })();
  }

  async function refreshHome(token) {
    var results = await Promise.all([
      TC.api("/blocks?limit=20"),
      TC.api("/status"),
      TC.api("/fees").catch(function (err) { return { error: err }; }),
      TC.api("/alerts").catch(function () { return null; }),
    ]);
    if (token !== routeToken) return;
    var blocks = results[0], s = results[1], f = results[2], alerts = results[3];
    document.getElementById("h-stats").innerHTML =
      '<div class="card stat"><span class="label">Altura</span><span class="value">' + TC.fmtInt(s.height) + '</span><span class="sub">' + e(s.network) + (s.syncing ? " · sincronizando" : "") + "</span></div>" +
      '<div class="card stat"><span class="label">Dificuldade</span><span class="value">' + e(TC.fmtDifficulty(s.difficulty)) + '</span><span class="sub">' + e(TC.fmtHashrate(s.hashrate_estimate)) + "</span></div>" +
      '<div class="card stat"><span class="label">Emitidos</span><span class="value">' + e(TC.fmtTCN(s.supply.emitted, { maxDecimals: 0 })) + '</span><span class="sub">recompensa ' + e(TC.fmtTCN(s.supply.current_block_reward)) + " · queimados " + e(TC.fmtTCN(s.supply.burned, { maxDecimals: 4 })) + "</span></div>" +
      '<div class="card stat"><span class="label">Mempool</span><span class="value"><a href="#/mempool">' + TC.fmtInt(s.mempool_txs) + ' tx</a></span><span class="sub">' + e(TC.fmtBytes(s.mempool_bytes)) + " · congestionamento " + e(TC.fmtMultiplier(s.congestion_bp)) + "</span></div>";
    document.getElementById("h-fees").innerHTML = f && !f.error ? feesPanel(f)
      : '<h3>Taxas agora</h3><p class="notice error">Não foi possível consultar as taxas (' + e(f && f.error ? f.error.message : "?") + ").</p>";
    document.getElementById("h-alerts").innerHTML = alerts ? alertsPanel(alerts) : "";
    document.getElementById("h-blocks").innerHTML = blocksTable(blocks);
    var lowest = blocks.length ? blocks[blocks.length - 1].height : 0;
    document.getElementById("h-pager").innerHTML = lowest > 0 ? '<a class="btn" href="#/blocks/' + lowest + '">Blocos anteriores →</a>' : "";
  }

  function blocksTable(blocks) {
    return '<div class="table-wrap"><table><thead><tr><th>Altura</th><th>Hash</th><th>Idade</th><th class="num">Txs</th><th class="hide-sm">Minerador</th><th class="num hide-sm">Tamanho</th></tr></thead><tbody>' +
      blocks.map(function (b) {
        return "<tr><td>" + blockLink(b.height, TC.fmtInt(b.height)) + "</td><td>" + blockLink(b.hash, TC.shortHash(b.hash)) +
          '</td><td class="nowrap" title="' + e(TC.fmtDate(b.timestamp)) + '">' + e(TC.timeAgo(b.timestamp)) + '</td><td class="num">' + TC.fmtInt(b.tx_count) +
          '</td><td class="hide-sm">' + addrLink(b.miner) + '</td><td class="num hide-sm">' + e(TC.fmtBytes(b.size)) + "</td></tr>";
      }).join("") + "</tbody></table></div>";
  }

  async function pageBlocks(token, before) {
    var blocks = await TC.api("/blocks?limit=20&before=" + before);
    if (token !== routeToken) return;
    var html = '<div class="section-title"><h2>Blocos anteriores a ' + TC.fmtInt(before) + '</h2><a href="#/">← mais recentes</a></div>' + blocksTable(blocks);
    var lowest = blocks.length ? blocks[blocks.length - 1].height : 0;
    if (lowest > 0) html += '<div class="pager"><a class="btn" href="#/blocks/' + lowest + '">Blocos anteriores →</a></div>';
    view.innerHTML = html;
  }

  async function pageBlock(token, id) {
    var b = await TC.api("/block/" + encodeURIComponent(id));
    if (token !== routeToken) return;
    var html = '<div class="crumbs"><a href="#/">Blocos</a> › bloco ' + TC.fmtInt(b.height) + "</div>";
    html += '<div class="section-title"><h1>Bloco ' + TC.fmtInt(b.height) + "</h1><span>" +
      (b.height > 0 ? '<a class="btn btn-small" href="#/block/' + (b.height - 1) + '">← anterior</a> ' : "") +
      '<a class="btn btn-small" href="#/block/' + (b.height + 1) + '">próximo →</a></span></div>';
    var burned = b.txs.reduce(function (acc, t) { return acc + BigInt(t.burned || 0); }, 0n);
    html += '<div class="card">' + kv([
      ["Hash", mono(b.hash) + copyBtn(b.hash)],
      ["Confirmações", b.confirmations > 0 ? TC.fmtInt(b.confirmations) : '<span class="badge warn">fora da cadeia principal</span>'],
      ["Data", e(TC.fmtDate(b.timestamp)) + ' <span class="faint">(' + e(TC.timeAgo(b.timestamp)) + ")</span>"],
      ["Minerador", addrLink(b.miner, false)],
      ["Recompensa", amount(b.subsidy) + ' <span class="faint">+ taxas</span> ' + amount(b.fees)],
      burned > 0n ? ["Queimado (sobretaxa de congestionamento)", amount(burned.toString())] : null,
      ["Transações", TC.fmtInt(b.tx_count)],
      ["Tamanho", e(TC.fmtBytes(b.size))],
      ["Bloco anterior", b.height > 0 ? blockLink(b.prev_hash) : faint("gênese")],
      ["Dificuldade", e(TC.fmtDifficulty(b.difficulty))],
      ["Alvo (target)", mono(b.target)],
      ["Nonce", '<span class="mono">' + e(b.nonce) + "</span>"],
      ["Raiz das transações", mono(b.tx_root)],
      ["Raiz de estado", mono(b.state_root)],
      ["Sinalização", b.signal ? '<span class="mono">0x' + Number(b.signal).toString(16) + "</span> (bits de governança)" : faint("nenhuma")],
      ["Versão", e(b.version)],
    ]) + "</div>";
    html += '<div class="section-title"><h2>Transações</h2></div>' + txRows(b.txs);
    view.innerHTML = html;
  }

  async function pageTx(token, id) {
    var t = await TC.api("/tx/" + encodeURIComponent(id));
    if (token !== routeToken) return;
    var a = t.action;
    var confirmed = !t.in_mempool && has(t.block_height);
    var status = t.in_mempool ? '<span class="badge warn">aguardando no mempool</span>'
      : '<span class="badge ok">confirmada</span> ' + TC.fmtInt(t.confirmations) + " confirmações";
    if (confirmed && t.success === false) status += ' <span class="badge bad">execução falhou</span>';
    var html = '<div class="crumbs"><a href="#/">Blocos</a> › transação</div><h1>Transação</h1>';
    if (t.conflict) {
      html += '<div class="notice error" style="margin-bottom:16px"><strong>Alerta de gasto duplo.</strong> O remetente enviou outra transação com o mesmo nonce (' + txLink(t.conflict) + "). " +
        (t.in_mempool ? "Só uma das duas pode ser confirmada: não entregue nada antes de ver confirmações." : "Esta foi a confirmada; a outra nunca poderá ser incluída.") + "</div>";
    }
    if (t.replaceable && t.in_mempool) {
      html += '<div class="notice accent" style="margin-bottom:16px">Esta transação foi enviada como <strong>substituível</strong>: enquanto não for confirmada, o remetente pode trocá-la por outra com taxa maior (<code>bump-fee</code>). Se você é o recebedor, espere pelo menos 1 confirmação.</div>';
    }
    var feeCell = amount(t.fee) + ' <span class="faint">(' + TC.fmtInt(t.size) + " bytes)</span>";
    if (Number(t.burned) > 0) feeCell += ' · <span class="nowrap">queimado ' + e(TC.fmtTCN(t.burned)) + "</span>";
    html += '<div class="card">' + kv([
      ["Txid", mono(t.txid) + copyBtn(t.txid)],
      ["Status", status],
      has(t.block_height) ? ["Bloco", blockLink(t.block_height, TC.fmtInt(t.block_height))] : null,
      ["Tipo", e(actionLabel(a))],
      ["Remetente", addrLink(t.sender, false)],
      ["Nonce", TC.fmtInt(t.nonce)],
      ["Taxa", feeCell],
      ["Substituível (RBF)", t.replaceable ? '<span class="badge warn">sim</span> <span class="faint">o remetente permitiu aumentar a taxa antes da confirmação</span>' : "não"],
      ["Expira", t.expiry_height ? "após o bloco " + TC.fmtInt(t.expiry_height) : faint("não expira")],
      t.created ? [a.type === "propose" ? "Proposta criada" : "Contrato criado", a.type === "propose" ? proposalLink(t.created, false) : contractLink(t.created, false)] : null,
    ]) + "</div>";
    html += '<div class="section-title"><h2>' + e(actionLabel(a)) + '</h2></div><div class="card">' + actionDetails(t) + "</div>";

    if (isProgramTx(t) || t.success === false || (t.logs && t.logs.length)) {
      html += '<div class="section-title"><h2>Resultado da execução</h2></div><div class="card">';
      if (!confirmed) {
        html += '<p class="muted" style="margin:0">A transação ainda não foi executada. O resultado (sucesso, combustível usado e eventos) aparece depois da confirmação.</p>';
      } else {
        var maxFuel = isProgramTx(t) ? a.max_fuel : null;
        html += kv([
          ["Resultado", t.success === false
            ? '<span class="badge bad">falhou</span> <span class="mono break">' + e(t.error || "erro desconhecido") + '</span><br><span class="faint">Todos os efeitos foram revertidos; a taxa foi cobrada.</span>'
            : '<span class="badge ok">sucesso</span>'],
          ["Combustível usado", TC.fmtInt(t.fuel_used) + (maxFuel ? ' <span class="faint">de ' + TC.fmtInt(maxFuel) + " reservados</span>" : "")],
          ["Taxa queimada", Number(t.burned) > 0 ? amount(t.burned) : faint("nada (sem congestionamento)")],
          a.type === "deploy" && t.program ? ["Contrato criado", addrLink(t.program, false)] : null,
          has(t.return_value) ? ["Valor retornado", mono(t.return_value)] : null,
        ]);
        html += '<h3 style="margin-top:18px">Eventos</h3>' + logsTable(t.logs);
      }
      html += "</div>";
    }
    view.innerHTML = html;
  }

  var ARG_HINTS = {
    int: "número, ex.: 42 ou 2.5tcn",
    bool: "true ou false",
    text: "texto",
    bytes: "hexadecimal, ex.: 0xabcd",
    address: "endereço tc1…",
  };
  function argHint(type) {
    if (ARG_HINTS[type]) return ARG_HINTS[type];
    if (/^list\[/.test(type)) return "lista, ex.: [1, 2, 3]";
    return type;
  }

  function signature(f) {
    return f.name + "(" + f.params.map(function (p) { return p[0] + ": " + p[1]; }).join(", ") + ")" + (f.returns && f.returns !== "nothing" ? " -> " + f.returns : "") + (f.payable ? " payable" : "");
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
    btn.textContent = "Consultar";
    form.appendChild(btn);
    var out = document.createElement("div");
    out.className = "view-out";
    out.setAttribute("aria-live", "polite");
    form.appendChild(out);
    form.addEventListener("submit", function (ev) {
      ev.preventDefault();
      btn.disabled = true;
      out.className = "view-out faint";
      out.textContent = "Consultando…";
      TC.post("/program/" + encodeURIComponent(addr) + "/view", {
        function: f.name,
        args: inputs.map(function (x) { return x.value; }),
      }).then(function (r) {
        if (has(r.result)) {
          out.className = "view-out ok";
          out.textContent = "→ " + r.result + "   (combustível: " + TC.fmtInt(r.fuel_used) + ")";
        } else {
          out.className = "view-out bad";
          out.textContent = "Erro: " + (r.error || "sem resultado") + "   (combustível: " + TC.fmtInt(r.fuel_used) + ")";
        }
      }).catch(function (err) {
        out.className = "view-out bad";
        out.textContent = "Erro: " + (err.message || err);
      }).then(function () { btn.disabled = false; });
    });
    return form;
  }

  function accountCards(acc) {
    return '<div class="grid grid-4">' +
      '<div class="card stat"><span class="label">Saldo</span><span class="value">' + e(TC.fmtTCN(acc.balance)) + '</span><span class="sub">total na conta</span></div>' +
      '<div class="card stat"><span class="label">Disponível para gastar</span><span class="value">' + e(TC.fmtTCN(acc.spendable)) + '</span><span class="sub">saldo menos o que está travado</span></div>' +
      '<div class="card stat"><span class="label">Travado em votos</span><span class="value">' + e(TC.fmtTCN(acc.locked)) + '</span><span class="sub">' + (Number(acc.locked) > 0 ? "até o bloco " + TC.fmtInt(acc.locked_until) : "—") + "</span></div>" +
      '<div class="card stat"><span class="label">Recompensas em cooldown</span><span class="value">' + e(TC.fmtTCN(acc.immature)) + '</span><span class="sub">ainda fora do saldo: 25% liberados após 100 blocos, o resto após 1.000</span></div>' +
      "</div>";
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
    var acc = res[0], prog = res[1];
    var html;
    if (prog) {
      html = '<div class="crumbs"><a href="#/">Blocos</a> › contrato TCCL</div><h1>Contrato TCCL · <span class="mono">' + e(prog.name) + "</span></h1>";
      html += '<p class="mono break">' + e(prog.address) + copyBtn(prog.address) + "</p>";
      html += '<div class="grid grid-4">' +
        '<div class="card stat"><span class="label">Saldo do contrato</span><span class="value">' + e(TC.fmtTCN(prog.balance)) + "</span></div>" +
        '<div class="card stat"><span class="label">Armazenamento</span><span class="value">' + e(TC.fmtBytes(prog.state_bytes)) + '</span><span class="sub">' + TC.fmtInt(prog.storage_items) + " itens (código + dados)</span></div>" +
        '<div class="card stat"><span class="label">Depósito de armazenamento</span><span class="value">' + e(TC.fmtTCN(prog.deposit)) + '</span><span class="sub">reembolsável quando o espaço é liberado</span></div>' +
        '<div class="card stat"><span class="label">Funções</span><span class="value">' + TC.fmtInt(prog.functions.length) + '</span><span class="sub">' + TC.fmtInt(prog.functions.filter(function (f) { return f.kind === "view"; }).length) + " consultas grátis</span></div>" +
        "</div>";
      html += '<div class="card" style="margin-top:16px">' + kv([
        ["Criador", addrLink(prog.creator, false)],
        ["Criado no bloco", blockLink(prog.created_height, TC.fmtInt(prog.created_height))],
        ["Transação de publicação", txLink(prog.deploy_txid, false)],
        ["Hash do código-fonte", mono(prog.source_hash)],
        ["Nonce / tx pendentes", TC.fmtInt(acc.nonce) + " / " + TC.fmtInt(acc.mempool_txs)],
      ]) + '<p style="margin:14px 0 0"><button class="btn btn-small" type="button" id="load-source">Mostrar código-fonte</button></p><div id="source-box"></div></div>';
      html += '<div class="section-title"><h2>Funções</h2></div><div class="table-wrap"><table><thead><tr><th>Função</th><th>Tipo</th><th class="hide-sm">Parâmetros</th><th class="hide-sm">Retorna</th></tr></thead><tbody>' +
        prog.functions.map(function (f) {
          return '<tr><td class="mono">' + e(f.name) + "</td><td>" + e(FN_KIND_LABELS[f.kind] || f.kind) + (f.payable ? ' <span class="badge accent">payable</span>' : "") +
            '</td><td class="hide-sm mono">' + (f.params.length ? f.params.map(function (p) { return e(p[0]) + ": " + e(p[1]); }).join(", ") : "—") +
            '</td><td class="hide-sm mono">' + e(f.returns === "nothing" ? "—" : f.returns) + "</td></tr>";
        }).join("") + "</tbody></table></div>";
      html += '<div class="section-title"><h2>Consultar (views)</h2><span class="faint">grátis, sem transação</span></div><div id="views" class="grid grid-2"></div>';
      html += '<p class="faint">Para chamar ações use a carteira: <code>thecoin-wallet contract invoke ' + e(prog.address) + " &lt;função&gt; [argumentos…]</code>.</p>";
    } else {
      html = '<div class="crumbs"><a href="#/">Blocos</a> › endereço</div><h1>Endereço</h1>';
      html += '<p class="mono break">' + e(acc.address) + copyBtn(acc.address) + "</p>";
      html += accountCards(acc);
      html += '<div class="card" style="margin-top:16px">' + kv([
        ["Nonce (tx enviadas)", TC.fmtInt(acc.nonce)],
        ["Próximo nonce", TC.fmtInt(acc.next_nonce)],
        ["Tx pendentes no mempool", TC.fmtInt(acc.mempool_txs)],
      ]) + "</div>";
    }
    html += '<div class="section-title"><h2>Histórico</h2></div><div id="history"><p class="loading">Carregando histórico…</p></div><div class="pager" id="more"></div>';
    view.innerHTML = html;

    if (prog) {
      var box = document.getElementById("views");
      var views = prog.functions.filter(function (f) { return f.kind === "view"; });
      if (!views.length) box.outerHTML = '<p class="faint">Este contrato não tem consultas.</p>';
      views.forEach(function (f) {
        var card = document.createElement("div");
        card.className = "card";
        card.appendChild(viewForm(prog.address, f));
        box.appendChild(card);
      });
      document.getElementById("load-source").addEventListener("click", function () {
        loadSource(token, prog, this);
      });
    }
    await loadHistory(token, acc.address, null, []);
  }

  async function loadSource(token, prog, btn) {
    var boxEl = document.getElementById("source-box");
    btn.disabled = true;
    btn.textContent = "Carregando…";
    try {
      var t = await TC.api("/tx/" + prog.deploy_txid);
      if (token !== routeToken) return;
      var src = t.action && t.action.source;
      if (typeof src !== "string") throw new Error("transação de publicação sem código-fonte");
      boxEl.textContent = "";
      var note = document.createElement("p");
      var ok = t.action.source_hash === prog.source_hash;
      note.className = ok ? "badge ok" : "badge bad";
      note.textContent = ok ? "✓ confere com o hash registrado no contrato" : "✗ o hash não confere";
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
      btn.textContent = "Mostrar código-fonte";
      boxEl.textContent = "Não foi possível carregar o código-fonte: " + (err.message || err);
    }
  }

  /*
   * The history endpoint pages with cursor "height:position" (exclusive).
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
          var pos = has(last.position) ? last.position : null;
          if (pos === null) {
            var blk = await TC.api("/block/" + last.block_hash);
            pos = Math.max(0, blk.txs.findIndex(function (t) { return t.txid === last.txid; }));
          }
          await loadHistory(token, addr, last.block_height + ":" + pos, all);
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
        ["Árbitro", s.arbiter ? addrLink(s.arbiter, false) : faint("nenhum")],
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
        ["Hash lock (SHA-256)", mono(s.hash_lock)],
        ["Expira após o bloco", TC.fmtInt(s.timeout_height)],
      ]);
      case "multisig": return kv([
        ["Assinantes", s.signers.map(function (x) { return addrLink(x, false); }).join("<br>")],
        ["Limite", e(s.threshold) + " de " + s.signers.length + " aprovações"],
        ["Próximo id de gasto", TC.fmtInt(s.next_spend_id)],
      ]) + '<h3 style="margin-top:18px">Gastos pendentes</h3>' + (s.pending.length ?
        '<div class="table-wrap"><table><thead><tr><th>#</th><th>Para</th><th class="num">Valor</th><th>Aprovações</th><th class="hide-sm">Criado</th></tr></thead><tbody>' +
        s.pending.map(function (p) {
          return "<tr><td>" + e(p.id) + "</td><td>" + addrLink(p.to) + '</td><td class="num">' + amount(p.amount) + "</td><td>" + p.approvals.length + "/" + e(s.threshold) + " " +
            p.approvals.map(function (x) { return addrLink(x); }).join(", ") + '</td><td class="hide-sm">bloco ' + TC.fmtInt(p.created_height) + "</td></tr>";
        }).join("") + "</tbody></table></div>" : '<p class="faint">Nenhum gasto pendente. Com saldo zero, qualquer assinante pode encerrar o cofre: <code>thecoin-wallet contract multisig-close</code>.</p>');
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
        view.innerHTML = '<div class="crumbs"><a href="#/">Blocos</a> › contrato</div><h1>Contrato</h1><p class="mono break">' + e(id) + "</p>" +
          '<div class="notice">Este contrato não existe no estado atual. Contratos encerrados (saldo zero) são removidos do estado para mantê-lo compacto — e o depósito de armazenamento volta ao criador. O histórico das chamadas continua disponível nas transações dos participantes.</div>';
        return;
      }
      throw err;
    }
    if (token !== routeToken) return;
    var html = '<div class="crumbs"><a href="#/">Blocos</a> › contrato</div><h1>Contrato · ' + e(KIND_LABELS[c.state.kind] || c.state.kind) + "</h1>";
    html += '<div class="card">' + kv([
      ["Id", mono(c.id) + copyBtn(c.id)],
      ["Criador", addrLink(c.creator, false)],
      ["Criado no bloco", blockLink(c.created_height, TC.fmtInt(c.created_height))],
      ["Saldo do contrato", amount(c.balance)],
      ["Depósito de armazenamento", has(c.deposit) ? amount(c.deposit) + ' <span class="faint">reembolsável ao criador quando o contrato termina</span>' : faint("—")],
    ]) + "</div>";
    html += '<div class="section-title"><h2>Estado</h2></div><div class="card">' + contractState(c) + "</div>";
    view.innerHTML = html;
  }

  async function pageMempool(token) {
    var m = await TC.api("/mempool");
    if (token !== routeToken) return;
    var html = '<div class="crumbs"><a href="#/">Blocos</a> › mempool</div><div class="section-title"><h1>Mempool</h1><span class="faint">atualiza a cada 15 s</span></div>';
    html += '<p class="muted">' + TC.fmtInt(m.count) + " transações aguardando (" + e(TC.fmtBytes(m.bytes)) + "). Mostrando as de maior taxa por unidade de peso (bytes + combustível ÷ 100).</p>";
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
      view.innerHTML = '<div class="notice error">Nada encontrado para <span class="mono break">' + e(h) + "</span>. Contratos encerrados são removidos do estado e transações recusadas (por exemplo, a segunda de um gasto duplo) nunca entram na blockchain.</div>";
      return;
    }
    view.innerHTML = '<div class="notice error">Formato não reconhecido. Use uma altura de bloco, um hash de 64 caracteres hexadecimais ou um endereço <span class="mono">tc1…</span> (contas e contratos TCCL).</div>';
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
        case "blocks": await pageBlocks(token, Number(arg)); break;
        case "block": await pageBlock(token, arg); break;
        case "tx": await pageTx(token, arg); break;
        case "address": case "program": await pageAddress(token, arg); break;
        case "contract": await pageContract(token, arg); break;
        case "mempool": await pageMempool(token); break;
        case "search": await search(arg); break;
        default: view.innerHTML = '<div class="notice error">Página não encontrada.</div>';
      }
    } catch (err) {
      if (token === routeToken) showError(err, err && err.status === 404 && kind === "tx" ? "Transação não encontrada (ela pode ter sido recusada ou substituída)" : undefined);
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
