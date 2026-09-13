(function () {
  "use strict";
  var $ = function (id) {
    return document.getElementById(id);
  };
  async function refresh() {
    try {
      var s = await TC.api("/status");
      $("s-height").textContent = TC.fmtInt(s.height);
      $("s-hashrate").textContent = TC.fmtHashrate(s.hashrate_estimate);
      $("s-emitted").textContent = TC.fmtTCN(s.supply.emitted, {
        maxDecimals: 0,
      });
      $("s-peers").textContent = TC.fmtInt(s.peers);
      $("live-dot").classList.add("on");
      $("live-updated").textContent =
        s.network + " · " + (s.syncing ? "Syncing" : "Connected");
      $("live-error").textContent =
        "Updated " +
        new Date().toLocaleTimeString("en-US") +
        " · Data from the connected node.";
    } catch (error) {
      ["s-height", "s-hashrate", "s-emitted", "s-peers"].forEach(function (id) {
        $(id).textContent = "—";
      });
      $("live-dot").classList.remove("on");
      $("live-updated").textContent = "Live data is temporarily unavailable";
      $("live-error").textContent =
        "The site cannot reach a node right now. It will check again automatically.";
    }
  }
  refresh();
  setInterval(function () {
    if (!document.hidden) refresh();
  }, 20000);
})();
