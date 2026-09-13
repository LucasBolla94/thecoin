//! Fixed-point amounts. 1 TCN = 100 000 000 motes. Floating point is never used.

/// Motes per TCN.
pub const COIN: u64 = 100_000_000;
/// Number of decimal places of TCN.
pub const DECIMALS: usize = 8;
/// Ticker symbol.
pub const TICKER: &str = "TCN";

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum AmountError {
    #[error("invalid amount format")]
    Format,
    #[error("too many decimal places (max 8)")]
    Precision,
    #[error("amount overflow")]
    Overflow,
}

/// Parses a decimal TCN string such as `"12"`, `"0.5"` or `"1.00000001"` into motes.
pub fn parse_amount(s: &str) -> Result<u64, AmountError> {
    let s = s.trim();
    if s.is_empty() {
        return Err(AmountError::Format);
    }
    let (int_part, frac_part) = match s.split_once('.') {
        Some((i, f)) => (i, f),
        None => (s, ""),
    };
    if int_part.is_empty() && frac_part.is_empty() {
        return Err(AmountError::Format);
    }
    if !int_part.chars().all(|c| c.is_ascii_digit()) || !frac_part.chars().all(|c| c.is_ascii_digit()) {
        return Err(AmountError::Format);
    }
    if frac_part.len() > DECIMALS {
        return Err(AmountError::Precision);
    }
    let int: u64 = if int_part.is_empty() { 0 } else { int_part.parse().map_err(|_| AmountError::Overflow)? };
    let mut frac: u64 = 0;
    if !frac_part.is_empty() {
        frac = frac_part.parse().map_err(|_| AmountError::Format)?;
        frac *= 10u64.pow((DECIMALS - frac_part.len()) as u32);
    }
    int.checked_mul(COIN).and_then(|v| v.checked_add(frac)).ok_or(AmountError::Overflow)
}

/// Formats motes as a TCN decimal string with trailing zeros trimmed (`"1.5"`).
pub fn format_amount(motes: u64) -> String {
    let int = motes / COIN;
    let frac = motes % COIN;
    if frac == 0 {
        return int.to_string();
    }
    let mut f = format!("{frac:08}");
    while f.ends_with('0') {
        f.pop();
    }
    format!("{int}.{f}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_and_format() {
        assert_eq!(parse_amount("1").unwrap(), COIN);
        assert_eq!(parse_amount("0.5").unwrap(), COIN / 2);
        assert_eq!(parse_amount(".00000001").unwrap(), 1);
        assert_eq!(parse_amount("50000000").unwrap(), 50_000_000 * COIN);
        assert_eq!(parse_amount("1.123456789"), Err(AmountError::Precision));
        assert_eq!(parse_amount("-1"), Err(AmountError::Format));
        assert_eq!(parse_amount("1e5"), Err(AmountError::Format));
        assert_eq!(parse_amount("999999999999999"), Err(AmountError::Overflow));
        assert_eq!(format_amount(150_000_000), "1.5");
        assert_eq!(format_amount(1), "0.00000001");
        assert_eq!(format_amount(0), "0");
    }
}
