//! Primitive operations shared by constant folding and the VM.

use crate::ast::BinOp;
use crate::error::VmError;
use crate::program::Value;

/// Maximum size of a single text/bytes/list value in bytes.
pub const MAX_VALUE_BYTES: usize = 65_536;
/// Maximum number of elements of a list held in memory.
pub const MAX_LIST_LEN: usize = 4_096;

fn int(v: &Value) -> Result<i128, VmError> {
    match v {
        Value::Int(i) => Ok(*i),
        other => Err(VmError::Type(format!("expected int, got {other:?}"))),
    }
}

/// Applies a non-short-circuit binary operator.
pub fn binary(op: BinOp, l: &Value, r: &Value) -> Result<Value, VmError> {
    Ok(match op {
        BinOp::Add => match (l, r) {
            (Value::Int(a), Value::Int(b)) => Value::Int(a.checked_add(*b).ok_or(VmError::Overflow)?),
            (Value::Text(a), Value::Text(b)) => {
                if a.len() + b.len() > MAX_VALUE_BYTES {
                    return Err(VmError::TooLarge);
                }
                Value::Text(format!("{a}{b}"))
            }
            (Value::Bytes(a), Value::Bytes(b)) => {
                if a.len() + b.len() > MAX_VALUE_BYTES {
                    return Err(VmError::TooLarge);
                }
                let mut v = a.clone();
                v.extend_from_slice(b);
                Value::Bytes(v)
            }
            _ => return Err(VmError::Type("bad operands for +".into())),
        },
        BinOp::Sub => Value::Int(int(l)?.checked_sub(int(r)?).ok_or(VmError::Overflow)?),
        BinOp::Mul => Value::Int(int(l)?.checked_mul(int(r)?).ok_or(VmError::Overflow)?),
        BinOp::Div => {
            let b = int(r)?;
            if b == 0 {
                return Err(VmError::DivisionByZero);
            }
            Value::Int(int(l)?.checked_div(b).ok_or(VmError::Overflow)?)
        }
        BinOp::Rem => {
            let b = int(r)?;
            if b == 0 {
                return Err(VmError::DivisionByZero);
            }
            Value::Int(int(l)?.checked_rem(b).ok_or(VmError::Overflow)?)
        }
        BinOp::Eq => Value::Bool(l == r),
        BinOp::Ne => Value::Bool(l != r),
        BinOp::Lt => Value::Bool(int(l)? < int(r)?),
        BinOp::Le => Value::Bool(int(l)? <= int(r)?),
        BinOp::Gt => Value::Bool(int(l)? > int(r)?),
        BinOp::Ge => Value::Bool(int(l)? >= int(r)?),
        BinOp::And | BinOp::Or => {
            let (Value::Bool(a), Value::Bool(b)) = (l, r) else {
                return Err(VmError::Type("bad operands for and/or".into()));
            };
            Value::Bool(if op == BinOp::And { *a && *b } else { *a || *b })
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arithmetic_is_checked() {
        assert_eq!(binary(BinOp::Add, &Value::Int(2), &Value::Int(3)).unwrap(), Value::Int(5));
        assert_eq!(binary(BinOp::Add, &Value::Int(i128::MAX), &Value::Int(1)), Err(VmError::Overflow));
        assert_eq!(binary(BinOp::Div, &Value::Int(1), &Value::Int(0)), Err(VmError::DivisionByZero));
        assert_eq!(binary(BinOp::Div, &Value::Int(i128::MIN), &Value::Int(-1)), Err(VmError::Overflow));
        assert_eq!(binary(BinOp::Rem, &Value::Int(-7), &Value::Int(2)).unwrap(), Value::Int(-1));
        assert_eq!(binary(BinOp::Add, &Value::Text("a".into()), &Value::Text("b".into())).unwrap(), Value::Text("ab".into()));
    }
}
