(function () {
  "use strict";
  document.documentElement.classList.add("js");
  var menu = document.querySelector(".menu-toggle");
  var nav = document.getElementById("navigation");
  menu.addEventListener("click", function () {
    var open = menu.getAttribute("aria-expanded") !== "true";
    menu.setAttribute("aria-expanded", String(open));
    nav.classList.toggle("open", open);
  });
  document.addEventListener("keydown", function (event) {
    if (
      event.key === "Escape" &&
      menu.getAttribute("aria-expanded") === "true"
    ) {
      menu.setAttribute("aria-expanded", "false");
      nav.classList.remove("open");
      menu.focus();
    }
  });
  var motion = document.querySelector(".motion-toggle");
  if (motion) {
    var reduced = matchMedia("(prefers-reduced-motion: reduce)");
    function updateMotion() {
      motion.hidden = reduced.matches;
    }
    updateMotion();
    reduced.addEventListener("change", updateMotion);
    motion.addEventListener("click", function () {
      var paused = document.body.classList.toggle("motion-paused");
      motion.setAttribute("aria-pressed", String(paused));
      motion.textContent = paused ? "Resume motion ▷" : "Pause motion Ⅱ";
    });
  }
  var steps = document.querySelectorAll("[data-step]");
  if (!steps.length) return;
  var descriptions = [
    "Your wallet signs the transaction locally. Your private keys stay with you.",
    "Nodes check the signature, balance, and transaction rules before relaying the payment.",
    "A miner groups valid transactions into a block and searches for a valid CoinHash proof of work.",
    "Peers independently verify the new block and adopt it if it extends the chain with the most accumulated work.",
  ];
  var timer = null,
    current = 0;
  var play = document.getElementById("play-journey");
  function select(step) {
    current = step;
    steps.forEach(function (button, index) {
      button.classList.toggle("active", index === step);
      button.setAttribute("aria-pressed", String(index === step));
    });
    document.querySelectorAll("[data-node]").forEach(function (node, index) {
      node.classList.toggle("active", index === step);
    });
    document.getElementById("journey-description").textContent =
      descriptions[step];
  }
  function stop() {
    clearInterval(timer);
    timer = null;
    play.textContent = "Play the journey ↗";
  }
  steps.forEach(function (button) {
    button.addEventListener("click", function () {
      stop();
      select(Number(button.dataset.step));
    });
  });
  play.addEventListener("click", function () {
    if (timer) {
      stop();
      return;
    }
    select(0);
    play.textContent = "Pause journey Ⅱ";
    timer = setInterval(function () {
      if (current === 3) {
        stop();
        return;
      }
      select(current + 1);
    }, 3000);
  });
  document.addEventListener("visibilitychange", function () {
    if (document.hidden) stop();
  });
  select(0);
})();
