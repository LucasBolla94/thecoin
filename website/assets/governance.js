/* The Coin governance page: proposals, tallies and parameters. */
(function () {
  "use strict";

  var view = document.getElementById("view");
  var e = TC.esc;
  var token = 0;

  var STATUS = {
    voting: ["Em votação", "accent"],
    approved: ["Aprovada · aguardando ativação", "ok"],
    activated: ["Ativada", "ok"],
    rejected: ["Rejeitada", "bad"],
  };

  var PARAM_INFO = {
    max_block_bytes: ["Tamanho máximo do bloco", "bytes"],
    min_fee_per_byte: ["Taxa mínima", "motes/byte"],
    proposal_deposit: ["Depósito de proposta", "tcn"],
    vote_period: ["Duração da votação", "blocks"],
    quorum_bp: ["Quórum (detentores)", "bp"],
    approval_bp: ["Aprovação dos detentores", "bp"],
    miner_approval_bp: ["Aprovação dos mineradores", "bp"],
    activation_delay: ["Atraso de ativação", "blocks"],
  };

  function fmtParam(name, v) {
    var unit = (PARAM_INFO[name] || [name, ""])[1];
    switch (unit) {
      case "tcn": return TC.fmtTCN(v);
      case "bp": return TC.bp(v);
      case "blocks": return TC.fmtInt(v) + (Number(v) === 1 ? " bloco" : " blocos") + " ≈ " + TC.fmtDuration(Number(v) * 60);
      case "bytes": return TC.fmtInt(v) + " bytes";
      default: return TC.fmtInt(v) + " " + unit;
    }
  }

  function badge(status) {
    var s = STATUS[status] || [status, ""];
    return '<span class="badge ' + s[1] + '">' + e(s[0]) + "</span>";
  }

  function actionText(pa) {
    switch (pa.type) {
      case "text": return "Proposta de texto";
      case "set_param": return "Alterar <strong>" + e((PARAM_INFO[pa.param] || [pa.param])[0]) + "</strong> (<code>" + e(pa.param) + "</code>) para <strong>" + e(fmtParam(pa.param, pa.value)) + "</strong>";
      case "software_upgrade": return "Atualizar software para a versão <strong>" + e(pa.version) + "</strong>";
      default: return e(pa.type);
    }
  }

  function pctOf(part, whole) {
    var w = BigInt(whole);
    if (w === 0n) return 0;
    return Number(BigInt(part) * 10000n / w) / 100;
  }

  function fmtPct(p) { return p.toFixed(2).replace(".", ",") + "%"; }

  /* Holder tally: stacked yes/no/abstain with labels (identity never by color alone). */
  function tally(t) {
    var total = BigInt(t.yes) + BigInt(t.no) + BigInt(t.abstain);
    var y = pctOf(t.yes, total), n = pctOf(t.no, total), a = pctOf(t.abstain, total);
    var bar = total === 0n ? '<div class="tally" aria-hidden="true"></div>' :
      '<div class="tally" role="img" aria-label="Sim ' + fmtPct(y) + ", não " + fmtPct(n) + ", abstenção " + fmtPct(a) + '">' +
      (y > 0 ? '<span class="yes" style="width:' + y + '%" title="Sim ' + fmtPct(y) + '"></span>' : "") +
      (n > 0 ? '<span class="no" style="width:' + n + '%" title="Não ' + fmtPct(n) + '"></span>' : "") +
      (a > 0 ? '<span class="abstain" style="width:' + a + '%" title="Abstenção ' + fmtPct(a) + '"></span>' : "") + "</div>";
    return bar + '<div class="legend">' +
      '<span><i style="background:var(--yes)"></i>Sim ' + e(TC.fmtTCN(t.yes)) + "</span>" +
      '<span><i style="background:var(--no)"></i>Não ' + e(TC.fmtTCN(t.no)) + "</span>" +
      '<span><i style="background:var(--abstain)"></i>Abstenção ' + e(TC.fmtTCN(t.abstain)) + "</span>" +
      '<span class="faint">' + TC.fmtInt(t.voters) + " eleitores</span></div>";
  }

  function meter(label, valueText, pct, thresholdPct, ok) {
    var p = Math.max(0, Math.min(100, pct));
    return '<div class="meter-row"><div class="meter-head"><span>' + e(label) + (ok === undefined ? "" : ok ? ' <span class="badge ok">✓ atingido</span>' : ' <span class="badge">pendente</span>') +
      '</span><span class="v">' + valueText + "</span></div>" +
      '<div class="meter" role="progressbar" aria-label="' + e(label) + '" aria-valuemin="0" aria-valuemax="100" aria-valuenow="' + p.toFixed(0) + '">' +
      '<div class="fill" style="width:' + p + '%"></div>' +
      (thresholdPct !== null && thresholdPct !== undefined ? '<span class="threshold" title="mínimo exigido" style="left:calc(' + Math.min(100, thresholdPct) + '% - 1px)"></span>' : "") +
      "</div></div>";
  }

  function progress(p, blockTime) {
    var t = p.tally;
    if (p.projection) {
      var pr = p.projection;
      var votes = BigInt(t.yes) + BigInt(t.no) + BigInt(t.abstain);
      var quorumPct = pr.quorum_progress_bp / 100;
      var approval = pr.approval_bp / 100, approvalNeed = pr.approval_needed_bp / 100;
      var miner = pr.miner_approval_bp / 100, minerNeed = pr.miner_approval_needed_bp / 100;
      return meter("Quórum dos detentores", e(TC.fmtTCN(String(votes))) + " de " + e(TC.fmtTCN(pr.quorum_needed)), quorumPct, null, pr.quorum_progress_bp >= 10000) +
        meter("Aprovação dos detentores (sim ÷ sim+não)", fmtPct(approval) + " · mínimo " + fmtPct(approvalNeed), approval, approvalNeed, pr.approval_bp >= pr.approval_needed_bp && Number(t.yes) > 0) +
        meter("Sinalização dos mineradores", TC.fmtInt(t.miner_yes_blocks) + " de " + TC.fmtInt(t.miner_total_blocks) + " blocos · " + fmtPct(miner) + " · mínimo " + fmtPct(minerNeed), miner, minerNeed, pr.miner_approval_bp >= pr.miner_approval_needed_bp) +
        '<p class="faint" style="margin:6px 0 0">Faltam ' + TC.fmtInt(pr.blocks_left) + " blocos ≈ " + TC.fmtDuration(pr.blocks_left * blockTime) + " (termina no bloco " + TC.fmtInt(p.end_height) + ").</p>";
    }
    var o = p.outcome;
    if (!o) return "";
    function yn(b) { return b ? '<span class="badge ok">✓ sim</span>' : '<span class="badge bad">✗ não</span>'; }
    var minerPct = t.miner_total_blocks ? pctOf(t.miner_yes_blocks, t.miner_total_blocks) : 0;
    return '<dl class="kv">' +
      "<dt>Quórum atingido</dt><dd>" + yn(o.quorum_reached) + ' <span class="faint">(circulante no fim: ' + e(TC.fmtTCN(o.circulating_at_end)) + ")</span></dd>" +
      "<dt>Detentores aprovaram</dt><dd>" + yn(o.holders_approved) + "</dd>" +
      "<dt>Mineradores aprovaram</dt><dd>" + yn(o.miners_approved) + ' <span class="faint">(' + TC.fmtInt(t.miner_yes_blocks) + "/" + TC.fmtInt(t.miner_total_blocks) + " blocos · " + fmtPct(minerPct) + ")</span></dd>" +
      "<dt>Depósito</dt><dd>" + (o.deposit_refunded ? "devolvido ao autor" : "queimado (sem quórum)") + "</dd>" +
      (p.activation_height ? "<dt>" + (p.status === "activated" ? "Ativada no bloco" : "Ativação no bloco") + "</dt><dd>" + TC.fmtInt(p.activation_height) + "</dd>" : "") +
      "</dl>";
  }

  function proposalCard(p, blockTime, full) {
    var title = full ? "<h1>" + e(p.title) + "</h1>" : '<h3><a href="#/proposal/' + e(p.id) + '">' + e(p.title) + "</a></h3>";
    return '<div class="card"' + (full ? "" : ' style="margin-bottom:16px"') + ">" +
      '<div class="section-title" style="margin:0 0 6px">' + badge(p.status) + '<span class="faint mono">' + e(TC.shortHash(p.id)) + "</span></div>" +
      title +
      '<p class="muted">' + actionText(p.action) + "</p>" +
      tally(p.tally) + progress(p, blockTime) + "</div>";
  }

  async function pageList(tk) {
    var res = await Promise.all([TC.api("/governance/proposals"), TC.api("/governance/params"), TC.api("/status")]);
    if (tk !== token) return;
    var proposals = res[0], params = res[1], status = res[2];
    var bt = status.supply.target_block_time || 60;
    var voting = proposals.filter(function (p) { return p.status === "voting"; });
    var pending = proposals.filter(function (p) { return p.status === "approved"; });
    var past = proposals.filter(function (p) { return p.status !== "voting" && p.status !== "approved"; });

    var html = "<h1>Governança</h1>" +
      '<p class="muted" style="max-width:760px">A rede evolui por votação on-chain. Detentores votam com moedas travadas e mineradores sinalizam nos blocos — as duas câmaras precisam aprovar. Altura atual: <strong>' + TC.fmtInt(status.height) + "</strong>.</p>";

    html += '<div class="section-title"><h2>Em votação (' + voting.length + ")</h2></div>";
    html += voting.length ? voting.map(function (p) { return proposalCard(p, bt, false); }).join("") :
      '<div class="notice">Nenhuma proposta em votação no momento. Veja abaixo como abrir uma.</div>';

    if (pending.length) {
      html += '<div class="section-title"><h2>Aprovadas, aguardando ativação</h2></div>' + pending.map(function (p) { return proposalCard(p, bt, false); }).join("");
    }

    html += '<div class="section-title"><h2>Histórico</h2></div>';
    html += past.length ? '<div class="table-wrap"><table><thead><tr><th>Proposta</th><th>Status</th><th class="hide-sm">Efeito</th><th class="num">Encerrou</th></tr></thead><tbody>' +
      past.map(function (p) {
        return '<tr><td><a href="#/proposal/' + e(p.id) + '">' + e(p.title) + "</a></td><td>" + badge(p.status) + '</td><td class="hide-sm">' + actionText(p.action) + '</td><td class="num">bloco ' + TC.fmtInt(p.end_height) + "</td></tr>";
      }).join("") + "</tbody></table></div>" : '<p class="faint">Nenhuma proposta encerrada ainda.</p>';

    html += '<div class="section-title"><h2>Parâmetros atuais</h2><span class="faint">' + TC.fmtInt(params.voting_proposals) + " em votação · " + TC.fmtInt(params.pending_activations) + " aguardando ativação</span></div>";
    html += '<div class="table-wrap"><table><thead><tr><th>Parâmetro</th><th class="num">Valor atual</th><th class="num hide-sm">Mínimo</th><th class="num hide-sm">Máximo</th></tr></thead><tbody>' +
      Object.keys(PARAM_INFO).map(function (k) {
        var b = params.bounds[k] || ["—", "—"];
        return "<tr><td>" + e(PARAM_INFO[k][0]) + ' <br><code>' + e(k) + '</code></td><td class="num">' + e(fmtParam(k, params.current[k])) +
          '</td><td class="num hide-sm">' + e(fmtParam(k, b[0])) + '</td><td class="num hide-sm">' + e(fmtParam(k, b[1])) + "</td></tr>";
      }).join("") + "</tbody></table></div>";
    html += '<p class="faint">Mudanças só são aceitas dentro dos limites mínimo/máximo, que fazem parte do protocolo.</p>';
    view.innerHTML = html;
  }

  async function pageProposal(tk, id) {
    var res = await Promise.all([TC.api("/governance/proposal/" + encodeURIComponent(id)), TC.api("/status")]);
    if (tk !== token) return;
    var p = res[0], status = res[1];
    var bt = status.supply.target_block_time || 60;
    var html = '<div class="crumbs"><a href="#/">Governança</a> › proposta</div>' + proposalCard(p, bt, true);
    html += '<div class="section-title"><h2>Detalhes</h2></div><div class="card"><dl class="kv">' +
      '<dt>Id</dt><dd><span class="mono break">' + e(p.id) + '</span> <button class="btn btn-small" type="button" data-copy="' + e(p.id) + '">copiar</button></dd>' +
      '<dt>Autor</dt><dd><a class="mono break" href="explorer.html#/address/' + e(p.proposer) + '">' + e(p.proposer) + "</a></dd>" +
      "<dt>Texto completo</dt><dd>" + (/^https?:\/\//i.test(p.url) ? '<a href="' + e(p.url) + '" target="_blank" rel="noopener nofollow">' + e(p.url) + "</a>" : '<span class="faint">—</span>') + "</dd>" +
      '<dt>Hash do texto</dt><dd><span class="mono break">' + e(p.content_hash) + "</span></dd>" +
      "<dt>Efeito</dt><dd>" + actionText(p.action) + (p.action.type === "software_upgrade" ? '<br><span class="mono break faint">' + e(p.action.release_hash) + "</span>" : "") + "</dd>" +
      "<dt>Depósito</dt><dd>" + e(TC.fmtTCN(p.deposit)) + "</dd>" +
      "<dt>Criada no bloco</dt><dd>" + TC.fmtInt(p.created_height) + "</dd>" +
      "<dt>Votação termina</dt><dd>bloco " + TC.fmtInt(p.end_height) + "</dd>" +
      "<dt>Bit de sinalização</dt><dd>" + e(p.signal_bit) + "</dd>" +
      "</dl></div>";
    if (p.status === "voting") {
      html += '<div class="section-title"><h2>Votar nesta proposta</h2></div><pre><code>thecoin-wallet gov vote ' + e(p.id) + ' yes &lt;peso em TCN&gt;\n\n# mineradores: em thecoind.toml\n[mining]\nsignal = ["' + e(p.id) + '"]</code></pre>';
    }
    view.innerHTML = html;
  }

  async function route(silent) {
    var tk = ++token;
    var parts = (location.hash || "#/").replace(/^#\/?/, "").split("/");
    if (!silent) view.innerHTML = '<p class="loading">Carregando…</p>';
    try {
      if (parts[0] === "proposal" && parts[1]) await pageProposal(tk, decodeURIComponent(parts[1]));
      else await pageList(tk);
    } catch (err) {
      if (tk !== token) return;
      view.innerHTML = '<div class="notice error">' + (err.status === 404 ? "Proposta não encontrada." : "Não foi possível consultar a API: " + e(err.message || err)) + '</div><p style="margin-top:12px"><a href="#/">← Todas as propostas</a></p>';
    }
  }

  window.addEventListener("hashchange", function () { route(); window.scrollTo(0, 0); });
  route();
  setInterval(function () { if (!document.hidden) route(true); }, 60000);
})();
