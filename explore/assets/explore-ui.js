/* explore.the-coin.cloud — network selector and network banner. */
(function () {
  "use strict";

  var nets = window.THECOIN_NETWORKS || {};
  var current = window.THECOIN_NETWORK_ID;
  var info = window.THECOIN_NETWORK_INFO || {};

  var select = document.getElementById("network");
  if (select) {
    Object.keys(nets).forEach(function (id) {
      var opt = document.createElement("option");
      opt.value = id;
      opt.textContent = nets[id].label + (nets[id].launched ? "" : " (soon)");
      if (id === current) opt.selected = true;
      select.appendChild(opt);
    });
    select.addEventListener("change", function () {
      window.THECOIN_SWITCH_NETWORK(select.value);
    });
  }

  var name = document.getElementById("network-name");
  if (name) name.textContent = info.label || "";
  var desc = document.getElementById("network-description");
  if (desc) desc.textContent = info.description || "";
  var badge = document.getElementById("network-badge");
  if (badge) {
    badge.textContent = info.label || "";
    badge.className = "badge " + (current === "mainnet" ? "ok" : "warn");
  }
  var q = document.getElementById("q");
  if (q) q.placeholder = "Search a block, transaction, or " + (info.hrp || "tc") + "1… address";
  if (info.label) document.title = "The Coin explorer · " + info.label;
})();
