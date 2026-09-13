// Shared helpers for the v0.2 whitepaper (whitepaper.typ and performance.typ).

#let accent = rgb("#0f766e")
#let muted = rgb("#555555")
#let orange = rgb("#ea580c")
#let burn = rgb("#dc2626")

#let note(body) = block(fill: rgb("#ecfdf5"), stroke: (left: 3pt + accent), inset: 9pt, width: 100%, body)
#let warnbox(body) = block(fill: rgb("#fff7ed"), stroke: (left: 3pt + orange), inset: 9pt, width: 100%, body)

// Integer ceiling division (a, b >= 0).
#let ceil-div(a, b) = calc.quo(a + b - 1, b)

// Number formatted in pt-BR: narrow no-break space between thousands, comma for decimals.
#let fmtnum(x, digits: 0) = {
  let p = calc.pow(10, digits)
  let scaled = int(calc.round(calc.abs(x) * p))
  let ip = calc.quo(scaled, p)
  let fp = calc.rem(scaled, p)
  let s = str(ip)
  let n = s.len()
  let out = ""
  for (i, ch) in s.clusters().enumerate() {
    if i > 0 and calc.rem(n - i, 3) == 0 { out += "\u{202F}" }
    out += ch
  }
  if digits > 0 {
    let f = str(fp)
    while f.len() < digits { f = "0" + f }
    out += "," + f
  }
  if x < 0 { "−" + out } else { out }
}

// Motes -> "0,00002590 TCN" style (8 decimals, trailing zeros trimmed to at least `min` decimals).
#let tcn(motes, min: 2) = {
  let ip = calc.quo(motes, 100000000)
  let fp = str(calc.rem(motes, 100000000))
  while fp.len() < 8 { fp = "0" + fp }
  while fp.len() > min and fp.ends-with("0") { fp = fp.slice(0, fp.len() - 1) }
  fmtnum(ip) + "," + fp + " TCN"
}

// ---- Consensus formulas (mirror crates/core/src/execution.rs) ----------------

// Mainnet defaults (crates/core/src/params.rs, MAINNET_GOV_DEFAULTS).
#let MAIN = (base_fee: 1000, fee_per_kb: 10000, fee_per_kfuel: 1000, storage_deposit_per_kb: 100000)
#let REG = (base_fee: 100, fee_per_kb: 1000, fee_per_kfuel: 100, storage_deposit_per_kb: 10000)

// Minimum fee at 1x congestion.
#let base-fee(p, size, fuel) = p.base_fee + ceil-div(size * p.fee_per_kb, 1000) + ceil-div(fuel * p.fee_per_kfuel, 1000)
// Minimum fee with congestion (basis points).
#let req-fee(p, size, fuel, cong) = ceil-div(base-fee(p, size, fuel) * calc.max(cong, 10000), 10000)
// Wallet fee for a priority multiplier (basis points), like FeePolicy::fee.
#let wallet-fee(p, size, fuel, cong, prio) = ceil-div(req-fee(p, size, fuel, cong) * calc.max(prio, 10000), 10000)
// Congestion update (next_congestion).
#let next-cong(cur, fill) = {
  let t = 5000
  let c = calc.max(cur, 10000)
  let n = if fill > t {
    c + calc.max(calc.quo(calc.quo(c * (fill - t), t), 8), 1)
  } else if fill < t {
    c - calc.quo(calc.quo(c * (t - fill), t), 8)
  } else { c }
  calc.min(calc.max(n, 10000), 10000000)
}
#let storage-deposit(p, bytes) = ceil-div(bytes, 1000) * p.storage_deposit_per_kb
