/*
 * The Coin website — shared helpers (API access with failover, formatting).
 * No dependencies. Exposes a single global: window.TC
 */
(function () {
  "use strict";

  var MOTES_PER_TCN = 100000000n;
  var LS_KEY = "thecoin_api_override";

  function apiBases() {
    var bases =
      window.THECOIN_API && window.THECOIN_API.length
        ? window.THECOIN_API.slice()
        : ["/api/v1"];
    try {
      var params = new URLSearchParams(window.location.search);
      var q = params.get("api");
      if (q === "reset") {
        localStorage.removeItem(LS_KEY);
      } else if (q) {
        localStorage.setItem(LS_KEY, q);
      }
      var o = localStorage.getItem(LS_KEY);
      if (o) bases = [o].concat(bases);
    } catch (e) {
      /* storage unavailable: use defaults */
    }
    return bases.map(function (b) {
      return b.replace(/\/+$/, "");
    });
  }

  var BASES = apiBases();
  var preferred = 0;

  function ApiError(status, message) {
    this.status = status;
    this.message = message;
  }
  ApiError.prototype = Object.create(Error.prototype);

  async function fetchJson(url, opts) {
    var ctrl = new AbortController();
    var timer = setTimeout(function () {
      ctrl.abort();
    }, 15000);
    try {
      var res = await fetch(
        url,
        Object.assign(
          { signal: ctrl.signal, headers: { Accept: "application/json" } },
          opts || {},
        ),
      );
      var body = null;
      try {
        body = await res.json();
      } catch (e) {
        body = null;
      }
      if (!res.ok) {
        // A 404 without the API's JSON error body comes from a web server
        // that does not proxy the API: treat it as an outage, not an answer.
        if (res.status === 404 && !(body && body.error)) {
          throw new ApiError(0, "API not available at " + url);
        }
        throw new ApiError(
          res.status,
          (body && body.error) || "HTTP " + res.status,
        );
      }
      return body;
    } finally {
      clearTimeout(timer);
    }
  }

  /**
   * GET/POST an API path (e.g. "/status") trying each base in turn.
   * 4xx responses are returned as errors immediately (they are answers, not outages).
   */
  async function api(path, opts) {
    var lastErr = null;
    for (var i = 0; i < BASES.length; i++) {
      var idx = (preferred + i) % BASES.length;
      try {
        var out = await fetchJson(BASES[idx] + path, opts);
        preferred = idx;
        return out;
      } catch (e) {
        lastErr = e;
        if (e instanceof ApiError && e.status >= 400 && e.status < 500) throw e;
      }
    }
    throw lastErr || new ApiError(0, "API unavailable");
  }

  /** POST a JSON body (read-only calls such as contract views). */
  function post(path, body) {
    return api(path, {
      method: "POST",
      headers: {
        Accept: "application/json",
        "Content-Type": "application/json",
      },
      body: JSON.stringify(body),
    });
  }

  /* ---------------- formatting ---------------- */

  function groupThousands(s) {
    return s.replace(/\B(?=(\d{3})+(?!\d))/g, ",");
  }

  /** Motes (number | string | bigint) → "1,234.5" (en-US), exact (BigInt). */
  function fmtTCN(motes, opts) {
    opts = opts || {};
    var m;
    try {
      m = BigInt(motes);
    } catch (e) {
      return "—";
    }
    var neg = m < 0n;
    if (neg) m = -m;
    var whole = m / MOTES_PER_TCN;
    var frac = (m % MOTES_PER_TCN).toString().padStart(8, "0");
    if (opts.maxDecimals !== undefined) frac = frac.slice(0, opts.maxDecimals);
    frac = frac.replace(/0+$/, "");
    var s = groupThousands(whole.toString()) + (frac ? "." + frac : "");
    return (neg ? "-" : "") + s + (opts.unit === false ? "" : " TCN");
  }

  function fmtInt(n) {
    if (n === null || n === undefined) return "—";
    return groupThousands(String(Math.trunc(Number(n))));
  }

  function fmtBytes(n) {
    n = Number(n);
    if (n < 1024) return n + " B";
    if (n < 1048576) return (n / 1024).toFixed(1) + " KB";
    return (n / 1048576).toFixed(2) + " MB";
  }

  function fmtHashrate(h) {
    h = Number(h) || 0;
    var units = ["H/s", "kH/s", "MH/s", "GH/s", "TH/s"];
    var i = 0;
    while (h >= 1000 && i < units.length - 1) {
      h /= 1000;
      i++;
    }
    return h.toFixed(h < 10 ? 2 : 1) + " " + units[i];
  }

  function fmtDifficulty(d) {
    d = Number(d) || 0;
    if (d < 1e6) return fmtInt(Math.round(d));
    return d.toExponential(3);
  }

  function fmtDate(ts) {
    if (!ts) return "—";
    return new Date(Number(ts) * 1000).toLocaleString("en-US");
  }

  function timeAgo(ts) {
    if (!ts) return "—";
    var s = Math.floor(Date.now() / 1000) - Number(ts);
    if (s < 0) s = 0;
    if (s < 60) return s + " s ago";
    if (s < 3600) return Math.floor(s / 60) + " min ago";
    if (s < 86400) return Math.floor(s / 3600) + " h ago";
    return Math.floor(s / 86400) + " d ago";
  }

  /** Seconds → "3 d 4 h", "12 min", ... */
  function fmtDuration(secs) {
    secs = Math.max(0, Math.round(Number(secs) || 0));
    var d = Math.floor(secs / 86400),
      h = Math.floor((secs % 86400) / 3600),
      m = Math.floor((secs % 3600) / 60);
    if (d > 0) return d + " d " + h + " h";
    if (h > 0) return h + " h " + m + " min";
    if (m > 0) return m + " min";
    return secs + " s";
  }

  function shortHash(h, n) {
    n = n || 8;
    if (!h) return "—";
    h = String(h);
    return h.length <= 2 * n + 1 ? h : h.slice(0, n) + "…" + h.slice(-n);
  }

  function shortAddr(a) {
    if (!a) return "—";
    a = String(a);
    return a.length <= 20 ? a : a.slice(0, 10) + "…" + a.slice(-6);
  }

  function esc(s) {
    return String(s === null || s === undefined ? "" : s)
      .replace(/&/g, "&amp;")
      .replace(/</g, "&lt;")
      .replace(/>/g, "&gt;")
      .replace(/"/g, "&quot;")
      .replace(/'/g, "&#39;");
  }

  function bp(v) {
    return (Number(v) / 100).toFixed(2) + "%";
  }

  /** Basis points as a multiplier: 12500 → "1,25×". */
  function fmtMultiplier(v) {
    return (Number(v) / 10000).toFixed(2) + "×";
  }

  /** ceil(motes × bp / 10000), exact (BigInt) — how wallets apply a priority. */
  function applyBp(motes, bpValue) {
    var m = BigInt(motes),
      b = BigInt(bpValue);
    return ((m * b + 9999n) / 10000n).toString();
  }

  /**
   * User-typed TCN amount ("12.5", "12,5", "1.234,5") → motes as a decimal
   * string, or null when invalid. Exact: never uses floating point.
   */
  function parseTCN(input) {
    var s = String(input || "")
      .trim()
      .replace(/\s+/g, "");
    if (s.indexOf(",") >= 0) s = s.replace(/\./g, "").replace(",", ".");
    var m = /^(\d+)(?:\.(\d{0,8}))?$/.exec(s);
    if (!m) return null;
    var motes =
      BigInt(m[1]) * MOTES_PER_TCN + BigInt((m[2] || "").padEnd(8, "0") || "0");
    if (motes > 18446744073709551615n) return null;
    return motes.toString();
  }

  /** UTF-8 byte length of a string. */
  function byteLength(s) {
    s = String(s || "");
    return typeof TextEncoder !== "undefined"
      ? new TextEncoder().encode(s).length
      : s.length;
  }

  function isHash(s) {
    return /^[0-9a-fA-F]{64}$/.test(s);
  }
  function isAddress(s) {
    return /^(tc|tct|tcr)1[02-9ac-hj-np-z]{20,}$/i.test(s);
  }

  /** Copy-to-clipboard buttons: <button data-copy="text">. */
  document.addEventListener("click", function (ev) {
    var btn = ev.target.closest && ev.target.closest("[data-copy]");
    if (!btn) return;
    var text = btn.getAttribute("data-copy");
    if (navigator.clipboard) {
      navigator.clipboard
        .writeText(text)
        .then(function () {
          var old = btn.textContent;
          btn.textContent = "Copied";
          setTimeout(function () {
            btn.textContent = old;
          }, 1200);
        })
        .catch(function () {
          btn.textContent = "Select and copy the command";
        });
    }
  });

  /* Mark the current page in the nav. */
  document.addEventListener("DOMContentLoaded", function () {
    var here = window.location.pathname.split("/").pop() || "index.html";
    document.querySelectorAll(".nav a").forEach(function (a) {
      if (a.getAttribute("href") === here)
        a.setAttribute("aria-current", "page");
    });
    var repo = window.THECOIN_REPO;
    if (repo) {
      document.querySelectorAll("[data-repo-link]").forEach(function (a) {
        a.href = repo + (a.getAttribute("data-repo-link") || "");
      });
    }
  });

  window.TC = {
    api: api,
    post: post,
    ApiError: ApiError,
    bases: BASES,
    fmtTCN: fmtTCN,
    fmtInt: fmtInt,
    fmtBytes: fmtBytes,
    fmtHashrate: fmtHashrate,
    fmtDifficulty: fmtDifficulty,
    fmtDate: fmtDate,
    fmtDuration: fmtDuration,
    timeAgo: timeAgo,
    shortHash: shortHash,
    shortAddr: shortAddr,
    esc: esc,
    bp: bp,
    fmtMultiplier: fmtMultiplier,
    applyBp: applyBp,
    parseTCN: parseTCN,
    byteLength: byteLength,
    isHash: isHash,
    isAddress: isAddress,
  };
})();
