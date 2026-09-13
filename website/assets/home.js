/* Landing page: live network stats. */
(function () {
  "use strict";
  var $ = function (id) { return document.getElementById(id); };

  async function refresh() {
    try {
      var s = await TC.api("/status");
      var sup = s.supply;
      $("s-height").textContent = TC.fmtInt(s.height);
      $("s-tip").textContent = "último bloco " + TC.timeAgo(s.tip_timestamp);
      $("s-hashrate").textContent = TC.fmtHashrate(s.hashrate_estimate);
      $("s-difficulty").textContent = "dificuldade " + TC.fmtDifficulty(s.difficulty);
      $("s-emitted").textContent = TC.fmtTCN(sup.emitted, { maxDecimals: 0 });
      var pct = Number((BigInt(sup.emitted) * 10000n) / BigInt(sup.max_supply)) / 100;
      $("s-emitted-pct").textContent = pct.toFixed(2).replace(".", ",") + "% de " + TC.fmtTCN(sup.max_supply, { maxDecimals: 0 });
      $("s-circulating").textContent = TC.fmtTCN(sup.circulating, { maxDecimals: 0 });
      $("s-burned").textContent = "queimados: " + TC.fmtTCN(sup.burned);
      $("s-reward").textContent = TC.fmtTCN(sup.current_block_reward);
      $("s-era").textContent = "era " + (sup.era + 1);
      var left = Math.max(0, sup.next_halving_height - (s.height + 1));
      $("s-halving").textContent = "bloco " + TC.fmtInt(sup.next_halving_height);
      $("s-halving-eta").textContent = "faltam " + TC.fmtInt(left) + " blocos ≈ " + TC.fmtDuration(left * sup.target_block_time);
      $("s-peers").textContent = TC.fmtInt(s.peers);
      $("s-mempool").textContent = TC.fmtInt(s.mempool_txs) + " tx no mempool" + (s.syncing ? " · sincronizando" : "");
      $("s-fee").textContent = TC.fmtInt(s.params.min_fee_per_byte) + " motes";
      $("live-dot").classList.add("on");
      $("live-updated").textContent = s.network + " · atualizado " + new Date().toLocaleTimeString("pt-BR");
      $("live-error").hidden = true;
      if (s.software_upgrade_required) {
        $("live-error").hidden = false;
        $("live-error").textContent = "Atualização de software aprovada pela governança: versão " + s.software_upgrade_required;
      }
    } catch (e) {
      $("live-dot").classList.remove("on");
      $("live-updated").textContent = "offline";
      $("live-error").hidden = false;
      $("live-error").textContent = "Não foi possível consultar a API da rede agora (" + (e.message || e) + "). Tentaremos novamente.";
    }
  }

  refresh();
  setInterval(refresh, 20000);
})();
