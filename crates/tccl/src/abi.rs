//! Converting between text/JSON and TCCL values (wallets, CLI, API).

use crate::program::{Type, Value};
use bech32::primitives::decode::CheckedHrpstring;
use bech32::{Bech32m, Hrp};

/// Parses a command-line argument according to its declared type.
///
/// * `int`: `123`, `-5`, `1_000`, or TCN amounts like `2.5tcn` (converted to motes)
/// * `bool`: `true` / `false`
/// * `text`: anything (surrounding double quotes are removed)
/// * `bytes`: `0x`-prefixed hex
/// * `address`: bech32m (`tc1…`, `tct1…`, `tcr1…`)
/// * `list[T]`: `[a, b, c]`
pub fn parse_arg(s: &str, t: &Type) -> Result<Value, String> {
    let s = s.trim();
    match t {
        Type::Int => parse_int(s).map(Value::Int),
        Type::Bool => match s {
            "true" => Ok(Value::Bool(true)),
            "false" => Ok(Value::Bool(false)),
            _ => Err(format!("expected true or false, got '{s}'")),
        },
        Type::Text => Ok(Value::Text(s.strip_prefix('"').and_then(|x| x.strip_suffix('"')).unwrap_or(s).to_string())),
        Type::Bytes => {
            let h = s.strip_prefix("0x").ok_or_else(|| format!("bytes must start with 0x, got '{s}'"))?;
            hex::decode(h.replace('_', "")).map(Value::Bytes).map_err(|_| format!("invalid hex '{s}'"))
        }
        Type::Address => parse_address(s).map(Value::Address),
        Type::List(inner) => {
            let body =
                s.strip_prefix('[').and_then(|x| x.strip_suffix(']')).ok_or_else(|| format!("list must look like [a, b], got '{s}'"))?;
            let mut items = Vec::new();
            for part in split_top_level(body) {
                if part.trim().is_empty() {
                    continue;
                }
                items.push(parse_arg(&part, inner)?);
            }
            Ok(Value::List(items))
        }
        Type::Map(_, _) | Type::Unit => Err(format!("arguments of type {t} are not supported")),
    }
}

fn parse_int(s: &str) -> Result<i128, String> {
    let lower = s.to_ascii_lowercase();
    if let Some(amount) = lower.strip_suffix("tcn") {
        let amount = amount.trim();
        let (neg, amount) = match amount.strip_prefix('-') {
            Some(a) => (true, a),
            None => (false, amount),
        };
        let (int_part, frac) = amount.split_once('.').unwrap_or((amount, ""));
        if frac.len() > 8 || int_part.is_empty() && frac.is_empty() {
            return Err(format!("invalid TCN amount '{s}'"));
        }
        let int_v: i128 =
            if int_part.is_empty() { 0 } else { int_part.replace('_', "").parse().map_err(|_| format!("invalid TCN amount '{s}'"))? };
        let frac_v: i128 =
            if frac.is_empty() { 0 } else { format!("{frac:0<8}").parse().map_err(|_| format!("invalid TCN amount '{s}'"))? };
        let v = int_v.checked_mul(100_000_000).and_then(|v| v.checked_add(frac_v)).ok_or("amount overflow")?;
        return Ok(if neg { -v } else { v });
    }
    s.replace('_', "").parse::<i128>().map_err(|_| format!("expected an integer, got '{s}'"))
}

pub fn parse_address(s: &str) -> Result<[u8; 20], String> {
    let checked = CheckedHrpstring::new::<Bech32m>(s).map_err(|_| format!("invalid address '{s}'"))?;
    let bytes: Vec<u8> = checked.byte_iter().collect();
    bytes.try_into().map_err(|_| format!("invalid address length '{s}'"))
}

pub fn format_address(a: &[u8; 20], hrp: &str) -> String {
    bech32::encode::<Bech32m>(Hrp::parse(hrp).expect("valid hrp"), a).expect("encodable")
}

fn split_top_level(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut in_quotes = false;
    let mut cur = String::new();
    for ch in s.chars() {
        match ch {
            '"' => {
                in_quotes = !in_quotes;
                cur.push(ch);
            }
            '[' if !in_quotes => {
                depth += 1;
                cur.push(ch);
            }
            ']' if !in_quotes => {
                depth -= 1;
                cur.push(ch);
            }
            ',' if depth == 0 && !in_quotes => out.push(std::mem::take(&mut cur)),
            _ => cur.push(ch),
        }
    }
    out.push(cur);
    out
}

/// JSON rendering of a value for APIs (ints as strings to keep full precision).
pub fn to_json(v: &Value, hrp: &str) -> serde_json::Value {
    match v {
        Value::Int(i) => serde_json::Value::String(i.to_string()),
        Value::Bool(b) => serde_json::Value::Bool(*b),
        Value::Text(t) => serde_json::Value::String(t.clone()),
        Value::Bytes(b) => serde_json::Value::String(format!("0x{}", hex::encode(b))),
        Value::Address(a) => serde_json::Value::String(format_address(a, hrp)),
        Value::List(items) => serde_json::Value::Array(items.iter().map(|x| to_json(x, hrp)).collect()),
        Value::Unit => serde_json::Value::Null,
    }
}

/// Human-readable rendering for CLIs.
pub fn display(v: &Value, hrp: &str) -> String {
    match v {
        Value::Int(i) => i.to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Text(t) => format!("\"{t}\""),
        Value::Bytes(b) => format!("0x{}", hex::encode(b)),
        Value::Address(a) => format_address(a, hrp),
        Value::List(items) => format!("[{}]", items.iter().map(|x| display(x, hrp)).collect::<Vec<_>>().join(", ")),
        Value::Unit => "(nothing)".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_values() {
        assert_eq!(parse_arg("1_500", &Type::Int).unwrap(), Value::Int(1500));
        assert_eq!(parse_arg("2.5tcn", &Type::Int).unwrap(), Value::Int(250_000_000));
        assert_eq!(parse_arg("0.00000001TCN", &Type::Int).unwrap(), Value::Int(1));
        assert_eq!(parse_arg("0xab", &Type::Bytes).unwrap(), Value::Bytes(vec![0xab]));
        assert_eq!(parse_arg("\"hi, there\"", &Type::Text).unwrap(), Value::Text("hi, there".into()));
        assert_eq!(
            parse_arg("[1, 2, 3]", &Type::List(Box::new(Type::Int))).unwrap(),
            Value::List(vec![Value::Int(1), Value::Int(2), Value::Int(3)])
        );
        let addr = format_address(&[7u8; 20], "tcr");
        assert_eq!(parse_arg(&addr, &Type::Address).unwrap(), Value::Address([7u8; 20]));
        assert!(parse_arg("1.2.3tcn", &Type::Int).is_err());
        assert!(parse_arg("abc", &Type::Bytes).is_err());
        for t in ["int", "list[bytes]", "map[address, list[int]]", "list[list[text]]"] {
            let parsed: Type = t.parse().unwrap();
            assert_eq!(parsed.to_string(), t);
        }
    }
}
