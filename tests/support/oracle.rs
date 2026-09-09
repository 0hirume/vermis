use std::io::{self, BufReader, Read, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use bstr::{BStr, ByteSlice};
use vermis::{InterpolatedKind, Keyword, LexError, Operator, Token, TokenKind};

const EOF: u8 = 0;
const COMMENT: u8 = 2;
const BLOCK_COMMENT: u8 = 3;
const NAME: u8 = 4;
const NUMBER: u8 = 5;
const RAW_STRING: u8 = 6;
const QUOTED_STRING: u8 = 7;
const INTERPOLATED: u8 = 8;
const ATTRIBUTE: u8 = 9;
const ATTRIBUTE_OPEN: u8 = 10;
const KEYWORD: u8 = 11;
const OPERATOR: u8 = 12;
const ERROR: u8 = 13;
const BYTE: u8 = 14;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OracleToken {
    pub kind: u8,
    pub value: u8,
    pub start: usize,
    pub end: usize,
    pub payload: u32,
}

pub struct Oracle {
    child: Child,
    input: ChildStdin,
    output: BufReader<ChildStdout>,
}

impl Oracle {
    pub fn spawn(path: &Path) -> io::Result<Self> {
        let mut child = Command::new(path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()?;

        let input = child
            .stdin
            .take()
            .ok_or_else(|| io::Error::other("oracle stdin was not piped"))?;
        let output = child
            .stdout
            .take()
            .ok_or_else(|| io::Error::other("oracle stdout was not piped"))?;

        Ok(Self {
            child,
            input,
            output: BufReader::new(output),
        })
    }

    pub fn lex(&mut self, source: &[u8]) -> io::Result<Vec<OracleToken>> {
        self.input.write_all(&(source.len() as u64).to_le_bytes())?;
        self.input.write_all(source)?;
        self.input.flush()?;

        let count = self.read_u32()? as usize;
        let mut tokens = Vec::with_capacity(count);

        for _ in 0..count {
            let mut header = [0; 4];
            self.output.read_exact(&mut header)?;

            let mut start = [0; 8];
            let mut end = [0; 8];
            let mut payload = [0; 4];

            self.output.read_exact(&mut start)?;
            self.output.read_exact(&mut end)?;
            self.output.read_exact(&mut payload)?;

            tokens.push(OracleToken {
                kind: header[0],
                value: header[1],
                start: u64::from_le_bytes(start) as usize,
                end: u64::from_le_bytes(end) as usize,
                payload: u32::from_le_bytes(payload),
            });
        }

        Ok(tokens)
    }

    fn read_u32(&mut self) -> io::Result<u32> {
        let mut bytes = [0; 4];
        self.output.read_exact(&mut bytes)?;
        Ok(u32::from_le_bytes(bytes))
    }
}

impl Drop for Oracle {
    fn drop(&mut self) {
        drop(self.child.kill());
        drop(self.child.wait());
    }
}

pub fn compare(oracle: &mut Oracle, source: &[u8]) -> Result<(), String> {
    validate_full_source(source)?;

    let oracle_tokens = oracle
        .lex(source)
        .map_err(|error| format!("oracle I/O failed: {error}"))?;
    let limit = source
        .iter()
        .position(|&byte| byte == 0)
        .unwrap_or(source.len());

    let vermis_tokens = vermis_tokens(source, limit);
    if vermis_tokens == oracle_tokens {
        return Ok(());
    }

    let mismatch = vermis_tokens
        .iter()
        .zip(&oracle_tokens)
        .position(|(vermis, oracle)| vermis != oracle)
        .unwrap_or(vermis_tokens.len().min(oracle_tokens.len()));

    Err(format!(
        "token mismatch at index {mismatch} for {source:?}\nvermis: {vermis_tokens:?}\noracle: {oracle_tokens:?}"
    ))
}

fn validate_full_source(source: &[u8]) -> Result<(), String> {
    let source_bstr = BStr::new(source);
    let mut end = 0;
    let mut previous_whitespace = false;

    for token in vermis::tokenize(source_bstr) {
        if token.kind == TokenKind::Eof {
            if token.span.start != source.len() || !token.span.is_empty() {
                return Err(format!("invalid eof span for {source:?}: {:?}", token.span));
            }

            continue;
        }

        if token.span.start != end || token.span.end > source.len() || token.span.is_empty() {
            return Err(format!("invalid token partition for {source:?}: {token:?}"));
        }

        if token.kind == TokenKind::Whitespace {
            let bytes = token.bytes(source_bstr).as_bytes();
            if previous_whitespace || bytes.iter().any(|&byte| !is_space(byte)) {
                return Err(format!(
                    "invalid whitespace span for {source:?}: {:?}",
                    token.span
                ));
            }
            previous_whitespace = true;
        } else {
            previous_whitespace = false;
        }

        end = token.span.end;
    }

    if end == source.len() {
        Ok(())
    } else {
        Err(format!(
            "token partition ended at {end} of {} for {source:?}",
            source.len()
        ))
    }
}

fn is_space(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\r' | b'\n' | 0x0b | 0x0c)
}

fn vermis_tokens(source: &[u8], limit: usize) -> Vec<OracleToken> {
    let source = BStr::new(source);
    let mut result = Vec::new();

    for token in vermis::tokenize(source) {
        if token.kind == TokenKind::Whitespace {
            continue;
        }

        if token.span.start >= limit {
            break;
        }

        let span = if token.kind == TokenKind::Error(LexError::BrokenInterpolatedDoubleBrace) {
            token.diagnostic_span()
        } else {
            token.span
        };

        result.push(normalize_token(token, span));
    }

    result.push(OracleToken {
        kind: EOF,
        value: 0,
        start: limit,
        end: limit,
        payload: 0,
    });

    result
}

fn normalize_token(token: Token, span: vermis::Span) -> OracleToken {
    let (kind, value, payload) = match token.kind {
        TokenKind::Eof => (EOF, 0, 0),
        TokenKind::Whitespace => unreachable!(),
        TokenKind::Comment => (COMMENT, 0, 0),
        TokenKind::BlockComment => (BLOCK_COMMENT, 0, 0),
        TokenKind::Name => (NAME, 0, 0),
        TokenKind::Number => (NUMBER, 0, 0),
        TokenKind::RawString => (RAW_STRING, 0, 0),
        TokenKind::QuotedString => (QUOTED_STRING, 0, 0),
        TokenKind::Interpolated(kind) => (INTERPOLATED, interpolated_value(kind), 0),
        TokenKind::Attribute => (ATTRIBUTE, 0, 0),
        TokenKind::AttributeOpen => (ATTRIBUTE_OPEN, 0, 0),
        TokenKind::Keyword(keyword) => (KEYWORD, keyword_value(keyword), 0),
        TokenKind::Operator(operator) => (OPERATOR, operator_value(operator), 0),
        TokenKind::Byte(byte) => (BYTE, byte, 0),
        TokenKind::Error(error) => {
            let (value, payload) = match error {
                LexError::BrokenString => (0, 0),
                LexError::BrokenComment => (1, 0),
                LexError::BrokenUnicode { codepoint } => (2, codepoint),
                LexError::BrokenInterpolatedDoubleBrace => (3, 0),
            };

            (ERROR, value, payload)
        }
    };

    OracleToken {
        kind,
        value,
        start: span.start,
        end: span.end,
        payload,
    }
}

fn interpolated_value(kind: InterpolatedKind) -> u8 {
    match kind {
        InterpolatedKind::Begin => 0,
        InterpolatedKind::Middle => 1,
        InterpolatedKind::End => 2,
        InterpolatedKind::Simple => 3,
    }
}

fn keyword_value(keyword: Keyword) -> u8 {
    match keyword {
        Keyword::And => 0,
        Keyword::Break => 1,
        Keyword::Do => 2,
        Keyword::Else => 3,
        Keyword::ElseIf => 4,
        Keyword::End => 5,
        Keyword::False => 6,
        Keyword::For => 7,
        Keyword::Function => 8,
        Keyword::If => 9,
        Keyword::In => 10,
        Keyword::Local => 11,
        Keyword::Nil => 12,
        Keyword::Not => 13,
        Keyword::Or => 14,
        Keyword::Repeat => 15,
        Keyword::Return => 16,
        Keyword::Then => 17,
        Keyword::True => 18,
        Keyword::Until => 19,
        Keyword::While => 20,
    }
}

fn operator_value(operator: Operator) -> u8 {
    match operator {
        Operator::Equal => 0,
        Operator::LessEqual => 1,
        Operator::GreaterEqual => 2,
        Operator::NotEqual => 3,
        Operator::Concat => 4,
        Operator::Ellipsis => 5,
        Operator::Arrow => 6,
        Operator::DoubleColon => 7,
        Operator::FloorDivide => 8,
        Operator::AddAssign => 9,
        Operator::SubtractAssign => 10,
        Operator::MultiplyAssign => 11,
        Operator::DivideAssign => 12,
        Operator::FloorDivideAssign => 13,
        Operator::ModuloAssign => 14,
        Operator::PowerAssign => 15,
        Operator::ConcatAssign => 16,
    }
}
