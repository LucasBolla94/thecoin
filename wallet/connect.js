/**
 * The Coin — connect a website or game to the user's wallet.
 *
 * Drop this file in your page:
 *
 *   <script type="module">
 *     import { connect, pay } from "https://wallet.the-coin.cloud/connect.js";
 *
 *     const { address } = await connect();              // asks the user once
 *     const { txid }    = await pay({ to: address, amount: "2.5", memo: "Sword" });
 *   </script>
 *
 * The wallet opens in a small window, the user approves or rejects, and the
 * window closes. Your page never sees the recovery phrase or the private keys,
 * and nothing can be sent without the user pressing the button.
 */

const WALLET_URL = "https://wallet.the-coin.cloud/";
const POPUP =
  "width=460,height=720,menubar=no,toolbar=no,location=no,status=no";

let counter = 0;

function ask(
  type,
  payload,
  { walletUrl = WALLET_URL, timeoutMs = 180000 } = {},
) {
  return new Promise((resolve, reject) => {
    const id = `${Date.now()}-${counter++}`;
    const origin = new URL(walletUrl).origin;
    const popup = window.open(walletUrl, "thecoin-wallet", POPUP);
    if (!popup) {
      reject(
        new Error(
          "The wallet window was blocked. Allow pop-ups for this site and try again.",
        ),
      );
      return;
    }

    let settled = false;
    const finish = (fn, value) => {
      if (settled) return;
      settled = true;
      window.removeEventListener("message", onMessage);
      clearInterval(closedTimer);
      clearTimeout(timer);
      try {
        popup.close();
      } catch {
        /* already closed */
      }
      fn(value);
    };

    function onMessage(event) {
      if (event.origin !== origin || !event.data || event.data.thecoin !== 1)
        return;
      // The wallet says it is ready: send the actual request.
      if (event.data.type === "ready") {
        popup.postMessage({ thecoin: 1, id, type, payload }, origin);
        return;
      }
      if (event.data.id !== id) return;
      if (event.data.ok) finish(resolve, event.data);
      else
        finish(
          reject,
          new Error(
            event.data.error === "rejected"
              ? "The user rejected the request."
              : event.data.error,
          ),
        );
    }

    window.addEventListener("message", onMessage);
    const closedTimer = setInterval(() => {
      if (popup.closed)
        finish(reject, new Error("The wallet window was closed."));
    }, 500);
    const timer = setTimeout(
      () => finish(reject, new Error("The wallet did not answer in time.")),
      timeoutMs,
    );
  });
}

/** Asks the user to share their address. Returns { address, network }. */
export function connect(options) {
  return ask("connect", {}, options);
}

/**
 * Asks the user to pay. Returns { txid } once they approve and the node
 * accepts the payment.
 *
 *   pay({ to: "tc1…", amount: "2.5", memo: "Sword" })
 *   pay({ uri: "thecoin:tc1…?amount=2.5" })
 *
 * `amount` is a decimal TCN string. A payment is final for your purposes once
 * the transaction has confirmations — check it with the node's
 * /api/v1/tx/{txid} or wait for the finality badge in the explorer.
 */
export function pay({ to, amount, memo, uri } = {}, options) {
  if (!uri && (!to || !amount))
    throw new Error("pay() needs to and amount, or a thecoin: uri");
  return ask(
    "pay",
    {
      to,
      amount: amount === undefined ? undefined : String(amount),
      memo,
      uri,
    },
    options,
  );
}

export default { connect, pay };
