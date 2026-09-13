//! Payment request URIs.
//!
//! `thecoin:<address>?amount=<TCN decimal>&memo=<text>&label=<text>`
//!
//! Example: `thecoin:tc1q...?amount=12.5&memo=Order%20%23123`
//! Shops show this as a QR code; wallets parse it and prefill a payment.

use anyhow::{anyhow, bail, Result};
use thecoin_core::amount::{format_amount, parse_amount};
use thecoin_core::{Address, Network};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PaymentRequest {
    pub address: String,
    pub amount: Option<u64>,
    pub memo: Option<String>,
    pub label: Option<String>,
}

fn encode_component(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

fn decode_component(s: &str) -> Result<String> {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => {
                let h = std::str::from_utf8(&bytes[i + 1..i + 3])?;
                out.push(u8::from_str_radix(h, 16).map_err(|_| anyhow!("bad percent encoding"))?);
                i += 3;
            }
            b'%' => bail!("bad percent encoding"),
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    Ok(String::from_utf8(out)?)
}

impl PaymentRequest {
    pub fn to_uri(&self) -> String {
        let mut params = Vec::new();
        if let Some(a) = self.amount {
            params.push(format!("amount={}", format_amount(a)));
        }
        if let Some(m) = &self.memo {
            params.push(format!("memo={}", encode_component(m)));
        }
        if let Some(l) = &self.label {
            params.push(format!("label={}", encode_component(l)));
        }
        if params.is_empty() {
            format!("thecoin:{}", self.address)
        } else {
            format!("thecoin:{}?{}", self.address, params.join("&"))
        }
    }

    pub fn parse(uri: &str, network: Network) -> Result<PaymentRequest> {
        let rest = uri.trim().strip_prefix("thecoin:").ok_or_else(|| anyhow!("URI must start with 'thecoin:'"))?;
        let (addr, query) = match rest.split_once('?') {
            Some((a, q)) => (a, Some(q)),
            None => (rest, None),
        };
        Address::decode(addr, network).map_err(|e| anyhow!("{e}"))?;
        let mut req = PaymentRequest { address: addr.to_string(), amount: None, memo: None, label: None };
        if let Some(q) = query {
            for pair in q.split('&').filter(|p| !p.is_empty()) {
                let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
                let v = decode_component(v)?;
                match k {
                    "amount" => req.amount = Some(parse_amount(&v).map_err(|e| anyhow!("amount: {e}"))?),
                    "memo" => req.memo = Some(v),
                    "label" => req.label = Some(v),
                    _ => {}
                }
            }
        }
        Ok(req)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let addr = Address::from_public_key(&[3; 32]).encode(Network::Mainnet);
        let r =
            PaymentRequest {
                address: addr, amount: Some(1_250_000_000), memo: Some("Pedido #123 ção".into()), label: Some("Loja".into())
            };
        let uri = r.to_uri();
        assert!(uri.contains("amount=12.5"));
        assert_eq!(PaymentRequest::parse(&uri, Network::Mainnet).unwrap(), r);
        assert!(PaymentRequest::parse(&uri, Network::Testnet).is_err());
    }
}
