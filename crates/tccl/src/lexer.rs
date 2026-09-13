//! Tokenizer. Indentation is significant (spaces only), like Python.

use crate::error::{CompileError, Pos};

#[derive(Clone, Debug, PartialEq)]
pub enum Tok {
    Ident(String),
    Int(i128),
    Text(String),
    Bytes(Vec<u8>),
    // keywords
    Contract,
    Const,
    State,
    Event,
    Init,
    Action,
    View,
    Fn,
    Payable,
    Let,
    If,
    Elif,
    Else,
    While,
    For,
    In,
    Break,
    Continue,
    Return,
    Require,
    Send,
    Emit,
    Destroy,
    Pass,
    True,
    False,
    And,
    Or,
    Not,
    // punctuation
    LParen,
    RParen,
    LBracket,
    RBracket,
    Comma,
    Colon,
    Dot,
    Arrow,
    Assign,
    PlusAssign,
    MinusAssign,
    StarAssign,
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    // layout
    Newline,
    Indent,
    Dedent,
    Eof,
}

#[derive(Clone, Debug)]
pub struct Token {
    pub tok: Tok,
    pub pos: Pos,
}

pub const MAX_SOURCE_BYTES: usize = 48_000;

fn keyword(s: &str) -> Option<Tok> {
    Some(match s {
        "contract" => Tok::Contract,
        "const" => Tok::Const,
        "state" => Tok::State,
        "event" => Tok::Event,
        "init" => Tok::Init,
        "action" => Tok::Action,
        "view" => Tok::View,
        "fn" => Tok::Fn,
        "payable" => Tok::Payable,
        "let" => Tok::Let,
        "if" => Tok::If,
        "elif" => Tok::Elif,
        "else" => Tok::Else,
        "while" => Tok::While,
        "for" => Tok::For,
        "in" => Tok::In,
        "break" => Tok::Break,
        "continue" => Tok::Continue,
        "return" => Tok::Return,
        "require" => Tok::Require,
        "send" => Tok::Send,
        "emit" => Tok::Emit,
        "destroy" => Tok::Destroy,
        "pass" => Tok::Pass,
        "true" => Tok::True,
        "false" => Tok::False,
        "and" => Tok::And,
        "or" => Tok::Or,
        "not" => Tok::Not,
        _ => return None,
    })
}

pub fn tokenize(src: &str) -> Result<Vec<Token>, CompileError> {
    if src.len() > MAX_SOURCE_BYTES {
        return Err(CompileError::new(Pos::default(), format!("source too large ({} bytes, max {MAX_SOURCE_BYTES})", src.len())));
    }
    let mut out: Vec<Token> = Vec::new();
    let mut indents: Vec<usize> = vec![0];
    let mut depth = 0usize; // (), [] nesting: newlines ignored inside
    let mut open_positions: Vec<Pos> = Vec::new();
    for (lineno, raw_line) in src.split('\n').enumerate() {
        let line_no = lineno as u32 + 1;
        let line = raw_line.strip_suffix('\r').unwrap_or(raw_line);
        let chars: Vec<char> = line.chars().collect();
        // Blank or comment-only lines do not affect indentation.
        let first_non_space = chars.iter().position(|c| *c != ' ');
        let is_blank = match first_non_space {
            None => true,
            Some(i) => chars[i] == '#',
        };
        if chars.contains(&'\t') {
            return Err(CompileError::new(Pos { line: line_no, col: 1 }, "tabs are not allowed; indent with spaces"));
        }
        if is_blank {
            continue;
        }
        let indent = first_non_space.unwrap_or(0);
        if depth == 0 {
            let current = *indents.last().expect("non-empty");
            let pos = Pos { line: line_no, col: 1 };
            if indent > current {
                indents.push(indent);
                out.push(Token { tok: Tok::Indent, pos });
            } else if indent < current {
                while indent < *indents.last().expect("non-empty") {
                    indents.pop();
                    out.push(Token { tok: Tok::Dedent, pos });
                }
                if indent != *indents.last().expect("non-empty") {
                    return Err(CompileError::new(pos, "indentation does not match any outer block"));
                }
            }
        }
        let mut i = indent;
        while i < chars.len() {
            let c = chars[i];
            let pos = Pos { line: line_no, col: i as u32 + 1 };
            if c == ' ' {
                i += 1;
                continue;
            }
            if c == '#' {
                break;
            }
            if c.is_ascii_alphabetic() || c == '_' {
                let start = i;
                while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '_') {
                    i += 1;
                }
                let word: String = chars[start..i].iter().collect();
                let tok = keyword(&word).unwrap_or(Tok::Ident(word));
                out.push(Token { tok, pos });
                continue;
            }
            if c.is_ascii_digit() {
                if c == '0' && i + 1 < chars.len() && (chars[i + 1] == 'x' || chars[i + 1] == 'X') {
                    let start = i + 2;
                    i = start;
                    while i < chars.len() && (chars[i].is_ascii_hexdigit() || chars[i] == '_') {
                        i += 1;
                    }
                    let digits: String = chars[start..i].iter().filter(|c| **c != '_').collect();
                    if !digits.len().is_multiple_of(2) {
                        return Err(CompileError::new(pos, "hex bytes literal must have an even number of digits"));
                    }
                    let bytes = hex::decode(&digits).map_err(|_| CompileError::new(pos, "invalid hex literal"))?;
                    out.push(Token { tok: Tok::Bytes(bytes), pos });
                    continue;
                }
                let start = i;
                while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '_') {
                    i += 1;
                }
                if i < chars.len() && (chars[i].is_ascii_alphabetic()) {
                    return Err(CompileError::new(pos, "invalid number literal"));
                }
                let digits: String = chars[start..i].iter().filter(|c| **c != '_').collect();
                let v: i128 = digits.parse().map_err(|_| CompileError::new(pos, "integer literal out of range"))?;
                out.push(Token { tok: Tok::Int(v), pos });
                continue;
            }
            if c == '"' {
                i += 1;
                let mut s = String::new();
                loop {
                    if i >= chars.len() {
                        return Err(CompileError::new(pos, "unterminated text literal"));
                    }
                    let ch = chars[i];
                    if ch == '"' {
                        i += 1;
                        break;
                    }
                    if ch == '\\' {
                        i += 1;
                        let esc = *chars.get(i).ok_or_else(|| CompileError::new(pos, "unterminated escape"))?;
                        s.push(match esc {
                            'n' => '\n',
                            't' => '\t',
                            '"' => '"',
                            '\\' => '\\',
                            _ => return Err(CompileError::new(pos, format!("unknown escape \\{esc}"))),
                        });
                        i += 1;
                        continue;
                    }
                    s.push(ch);
                    i += 1;
                }
                out.push(Token { tok: Tok::Text(s), pos });
                continue;
            }
            let next = chars.get(i + 1).copied();
            let (tok, len) = match (c, next) {
                ('-', Some('>')) => (Tok::Arrow, 2),
                ('=', Some('=')) => (Tok::Eq, 2),
                ('!', Some('=')) => (Tok::Ne, 2),
                ('<', Some('=')) => (Tok::Le, 2),
                ('>', Some('=')) => (Tok::Ge, 2),
                ('+', Some('=')) => (Tok::PlusAssign, 2),
                ('-', Some('=')) => (Tok::MinusAssign, 2),
                ('*', Some('=')) => (Tok::StarAssign, 2),
                ('(', _) => (Tok::LParen, 1),
                (')', _) => (Tok::RParen, 1),
                ('[', _) => (Tok::LBracket, 1),
                (']', _) => (Tok::RBracket, 1),
                (',', _) => (Tok::Comma, 1),
                (':', _) => (Tok::Colon, 1),
                ('.', _) => (Tok::Dot, 1),
                ('=', _) => (Tok::Assign, 1),
                ('+', _) => (Tok::Plus, 1),
                ('-', _) => (Tok::Minus, 1),
                ('*', _) => (Tok::Star, 1),
                ('/', _) => (Tok::Slash, 1),
                ('%', _) => (Tok::Percent, 1),
                ('<', _) => (Tok::Lt, 1),
                ('>', _) => (Tok::Gt, 1),
                _ => return Err(CompileError::new(pos, format!("unexpected character '{c}'"))),
            };
            match tok {
                Tok::LParen | Tok::LBracket => {
                    depth += 1;
                    open_positions.push(pos);
                }
                Tok::RParen | Tok::RBracket => {
                    if depth == 0 {
                        return Err(CompileError::new(pos, "unbalanced closing bracket"));
                    }
                    depth -= 1;
                    open_positions.pop();
                }
                _ => {}
            }
            out.push(Token { tok, pos });
            i += len;
        }
        if depth == 0 {
            out.push(Token { tok: Tok::Newline, pos: Pos { line: line_no, col: chars.len() as u32 + 1 } });
        }
    }
    let end = Pos { line: src.split('\n').count() as u32 + 1, col: 1 };
    if depth != 0 {
        return Err(CompileError::new(open_positions.pop().unwrap_or(end), "this bracket is never closed"));
    }
    while indents.len() > 1 {
        indents.pop();
        out.push(Token { tok: Tok::Dedent, pos: end });
    }
    out.push(Token { tok: Tok::Eof, pos: end });
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn indentation_and_literals() {
        let src = "contract A\naction f():\n    let x: int = 1_000 # c\n    if x > 0:\n        pass\n\n# comment\nview g() -> bytes:\n    return 0xdead_beef\n";
        let toks: Vec<Tok> = tokenize(src).unwrap().into_iter().map(|t| t.tok).collect();
        assert!(toks.contains(&Tok::Int(1000)));
        assert!(toks.contains(&Tok::Bytes(vec![0xde, 0xad, 0xbe, 0xef])));
        assert_eq!(toks.iter().filter(|t| **t == Tok::Indent).count(), 3);
        assert_eq!(toks.iter().filter(|t| **t == Tok::Dedent).count(), 3);
        assert!(tokenize("a\n\tb").is_err());
        assert!(tokenize("f(1,\n  2)").is_ok());
        assert!(tokenize("x = \"unterminated").is_err());
    }
}
