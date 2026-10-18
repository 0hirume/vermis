//! Typed Luau output using existing syntax and fixed token kinds.

use std::borrow::Cow;

use crate::{
    lexer::Lexer,
    token::{Keyword, Span, Symbol, TokenKind},
    tree::{NodeIndex, TokenIndex, Tree},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
/// Line ending used for emitted newlines.
pub enum LineEnding {
    /// Line feed.
    LineFeed,

    /// Carriage return followed by line feed.
    CarriageReturnLineFeed,
}

#[derive(Clone, Copy)]
struct EmittedToken<'source> {
    kind: TokenKind,
    bytes: &'source [u8],
}

/// Builds Luau output from typed tokens and original syntax.
pub struct Emitter<'source> {
    source: Option<&'source [u8]>,
    matched: usize,
    output: Vec<u8>,
    previous: Option<EmittedToken<'source>>,
    previous_source: Option<(&'source [u8], usize)>,
    separated: bool,
    line_break_since_previous: bool,
    line_ending: LineEnding,
}

impl<'source> Emitter<'source> {
    /// Creates an emitter with the requested line ending.
    pub fn new(line_ending: LineEnding) -> Self {
        Self {
            source: None,
            matched: 0,
            output: Vec::new(),
            previous: None,
            previous_source: None,
            separated: false,
            line_break_since_previous: false,
            line_ending,
        }
    }

    /// Creates an emitter that borrows identical output from the original tree.
    pub fn from_tree(tree: &Tree<'source>, line_ending: LineEnding) -> Self {
        Self {
            source: Some(tree.source),
            ..Self::new(line_ending)
        }
    }

    /// Emits a reserved keyword with its Luau spelling.
    pub fn keyword(&mut self, keyword: Keyword) {
        self.append(TokenKind::Keyword(keyword), keyword_bytes(keyword), None);
    }

    /// Emits punctuation or an operator with its Luau spelling.
    pub fn symbol(&mut self, symbol: Symbol) {
        self.append(TokenKind::Symbol(symbol), symbol_bytes(symbol), None);
    }

    /// Emits an original token without changing its spelling.
    pub fn token(&mut self, tree: &Tree<'source>, index: TokenIndex) {
        let token = *tree.token(index);

        if token.kind == TokenKind::EndOfFile {
            return;
        }

        let bytes = token.bytes(tree.source);

        if token.kind == TokenKind::Whitespace {
            self.write(bytes);
            self.separated |= !bytes.is_empty();

            self.line_break_since_previous |=
                bytes.iter().any(|&byte| matches!(byte, b'\r' | b'\n'));
        } else {
            self.append(token.kind, bytes, Some((tree.source, token.span)));
        }
    }

    /// Emits a node's original source, including trivia within its token range.
    pub fn node(&mut self, tree: &Tree<'source>, index: NodeIndex) {
        let node = tree.node(index);

        for position in node.tokens.start.get()..node.tokens.end.get() {
            self.token(tree, TokenIndex::new(position));
        }
    }

    /// Emits the requested number of spaces.
    pub fn spaces(&mut self, count: usize) {
        self.write_repeated(b' ', count);
        self.separated |= count > 0;
    }

    /// Emits the requested number of tabs.
    pub fn tabs(&mut self, count: usize) {
        self.write_repeated(b'\t', count);
        self.separated |= count > 0;
    }

    /// Emits a line ending.
    pub fn newline(&mut self) {
        match self.line_ending {
            LineEnding::LineFeed => self.write(b"\n"),
            LineEnding::CarriageReturnLineFeed => self.write(b"\r\n"),
        }

        self.separated = true;
        self.line_break_since_previous = true;
    }

    /// Returns the emitted bytes, borrowing unchanged source when available.
    pub fn finish(self) -> Cow<'source, [u8]> {
        match self.source {
            Some(source) if self.output.is_empty() && self.matched == source.len() => {
                Cow::Borrowed(source)
            }

            Some(source) if self.output.is_empty() => Cow::Owned(source[..self.matched].to_vec()),
            _ => Cow::Owned(self.output),
        }
    }

    fn append(
        &mut self,
        kind: TokenKind,
        bytes: &'source [u8],
        source: Option<(&'source [u8], Span)>,
    ) {
        if bytes.is_empty() {
            return;
        }

        if let Some(previous) = self.previous {
            if previous.kind == TokenKind::Comment && !self.line_break_since_previous {
                self.newline();
            } else if !self.separated
                && !source.is_some_and(|(bytes, span)| {
                    self.previous_source
                        .is_some_and(|(prior, end)| std::ptr::eq(prior, bytes) && end == span.start)
                })
                && could_merge(previous.kind, previous.bytes, bytes)
            {
                let mut joined = Vec::with_capacity(previous.bytes.len() + bytes.len());
                joined.extend_from_slice(previous.bytes);
                joined.extend_from_slice(bytes);

                if Lexer::new(&joined)
                    .next()
                    .is_some_and(|token| token.span.end != previous.bytes.len())
                {
                    self.spaces(1);
                }
            }
        }

        self.write(bytes);
        self.previous = Some(EmittedToken { kind, bytes });
        self.previous_source = source.map(|(bytes, span)| (bytes, span.end));
        self.separated = false;
        self.line_break_since_previous = false;
    }

    fn write(&mut self, bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }

        if self.output.is_empty()
            && let Some(source) = self.source
            && source
                .get(self.matched..)
                .is_some_and(|remaining| remaining.starts_with(bytes))
        {
            self.matched += bytes.len();
        } else {
            self.materialize();
            self.output.extend_from_slice(bytes);
        }
    }

    fn write_repeated(&mut self, byte: u8, count: usize) {
        if count == 0 {
            return;
        }

        if self.output.is_empty()
            && let Some(source) = self.source
            && source
                .get(self.matched..)
                .and_then(|remaining| remaining.get(..count))
                .is_some_and(|bytes| bytes.iter().all(|&candidate| candidate == byte))
        {
            self.matched += count;
        } else {
            self.materialize();
            self.output.extend(std::iter::repeat_n(byte, count));
        }
    }

    fn materialize(&mut self) {
        if self.output.is_empty()
            && let Some(source) = self.source
        {
            self.output = Vec::with_capacity(source.len());
            self.output.extend_from_slice(&source[..self.matched]);
        }
    }
}

fn could_merge(kind: TokenKind, previous: &[u8], next: &[u8]) -> bool {
    let Some((&last, &first)) = previous.last().zip(next.first()) else {
        return false;
    };

    match kind {
        TokenKind::InterpolatedStringEnd => return false,
        TokenKind::InterpolatedStringMiddle => return first == b'{',
        _ => {}
    }

    let name = |byte: u8| byte.is_ascii_alphanumeric() || byte == b'_';

    (name(last) && name(first))
        || (last.is_ascii_punctuation() && first.is_ascii_punctuation())
        || (last == b'.' && first.is_ascii_digit())
        || (last.is_ascii_digit() && first == b'.')
        || (kind == TokenKind::Number && last == b'.' && name(first))
        || (last == b'@' && name(first))
        || (matches!(last, b'e' | b'E') && matches!(first, b'+' | b'-'))
}

fn keyword_bytes(keyword: Keyword) -> &'static [u8] {
    match keyword {
        Keyword::And => b"and",
        Keyword::Break => b"break",
        Keyword::Do => b"do",
        Keyword::Else => b"else",
        Keyword::ElseIf => b"elseif",
        Keyword::End => b"end",
        Keyword::False => b"false",
        Keyword::For => b"for",
        Keyword::Function => b"function",
        Keyword::If => b"if",
        Keyword::In => b"in",
        Keyword::Local => b"local",
        Keyword::Nil => b"nil",
        Keyword::Not => b"not",
        Keyword::Or => b"or",
        Keyword::Repeat => b"repeat",
        Keyword::Return => b"return",
        Keyword::Then => b"then",
        Keyword::True => b"true",
        Keyword::Until => b"until",
        Keyword::While => b"while",
    }
}

fn symbol_bytes(symbol: Symbol) -> &'static [u8] {
    match symbol {
        Symbol::LeftParenthesis => b"(",
        Symbol::RightParenthesis => b")",
        Symbol::LeftBrace => b"{",
        Symbol::RightBrace => b"}",
        Symbol::LeftBracket => b"[",
        Symbol::RightBracket => b"]",
        Symbol::Comma => b",",
        Symbol::Semicolon => b";",
        Symbol::Colon => b":",
        Symbol::DoubleColon => b"::",
        Symbol::Dot => b".",
        Symbol::Add => b"+",
        Symbol::Subtract => b"-",
        Symbol::Multiply => b"*",
        Symbol::Divide => b"/",
        Symbol::FloorDivide => b"//",
        Symbol::Modulo => b"%",
        Symbol::Power => b"^",
        Symbol::Length => b"#",
        Symbol::Concatenate => b"..",
        Symbol::Ellipsis => b"...",
        Symbol::Assignment => b"=",
        Symbol::Equal => b"==",
        Symbol::NotEqual => b"~=",
        Symbol::LessThan => b"<",
        Symbol::LessThanOrEqual => b"<=",
        Symbol::GreaterThan => b">",
        Symbol::GreaterThanOrEqual => b">=",
        Symbol::AddAssignment => b"+=",
        Symbol::SubtractAssignment => b"-=",
        Symbol::MultiplyAssignment => b"*=",
        Symbol::DivideAssignment => b"/=",
        Symbol::FloorDivideAssignment => b"//=",
        Symbol::ModuloAssignment => b"%=",
        Symbol::PowerAssignment => b"^=",
        Symbol::ConcatenateAssignment => b"..=",
        Symbol::Arrow => b"->",
        Symbol::QuestionMark => b"?",
        Symbol::Ampersand => b"&",
        Symbol::Pipe => b"|",
        Symbol::AtSign => b"@",
        Symbol::AttributeOpen => b"@[",
        Symbol::Tilde => b"~",
    }
}
