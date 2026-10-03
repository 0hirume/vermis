use std::iter::FusedIterator;

use crate::token::{Keyword, Span, Symbol, Token, TokenKind};

enum Brace {
    Normal,
    Interpolation,
}

/// Lossless byte lexer, including trivia and a final end-of-input token.
pub struct Lexer<'source> {
    source: &'source [u8],
    cursor: usize,
    braces: Vec<Brace>,
    finished: bool,
}

impl<'source> Lexer<'source> {
    /// Starts lexing the given source bytes.
    pub fn new(source: &'source [u8]) -> Self {
        Self {
            source,
            cursor: 0,
            braces: Vec::new(),
            finished: false,
        }
    }

    fn peek(&self, distance: usize) -> Option<u8> {
        self.source.get(self.cursor + distance).copied()
    }

    fn consume(&mut self, byte: u8) -> bool {
        if self.peek(0) == Some(byte) {
            self.cursor += 1;

            true
        } else {
            false
        }
    }

    fn scan(&mut self, byte: u8) -> TokenKind {
        match byte {
            byte if is_space(byte) => {
                while self.peek(0).is_some_and(is_space) {
                    self.cursor += 1;
                }

                TokenKind::Whitespace
            }

            byte if is_name_start(byte) => self.name(),
            b'0'..=b'9' => self.number(),
            b'\'' | b'"' => self.quoted_string(byte),

            b'`' => {
                self.cursor += 1;

                self.interpolated_string(
                    TokenKind::InterpolatedStringStart,
                    TokenKind::InterpolatedStringSimple,
                )
            }

            b'[' => {
                let (equals, closed) = self.separator(b'[');

                if closed {
                    self.long_string(equals, TokenKind::RawString, TokenKind::MalformedString)
                } else if equals == 0 {
                    TokenKind::Symbol(Symbol::LeftBracket)
                } else {
                    TokenKind::MalformedString
                }
            }

            b'-' => {
                self.cursor += 1;

                if self.consume(b'-') {
                    self.comment()
                } else if self.consume(b'>') {
                    TokenKind::Symbol(Symbol::Arrow)
                } else if self.consume(b'=') {
                    TokenKind::Symbol(Symbol::SubtractAssignment)
                } else {
                    TokenKind::Symbol(Symbol::Subtract)
                }
            }

            b'.' => {
                self.cursor += 1;

                if self.consume(b'.') {
                    let symbol = if self.consume(b'.') {
                        Symbol::Ellipsis
                    } else if self.consume(b'=') {
                        Symbol::ConcatenateAssignment
                    } else {
                        Symbol::Concatenate
                    };

                    TokenKind::Symbol(symbol)
                } else if self.peek(0).is_some_and(|byte| byte.is_ascii_digit()) {
                    self.number()
                } else {
                    TokenKind::Symbol(Symbol::Dot)
                }
            }

            byte => self.punctuation(byte),
        }
    }

    fn punctuation(&mut self, byte: u8) -> TokenKind {
        match byte {
            b'/' => {
                self.cursor += 1;

                let symbol = if self.consume(b'/') {
                    if self.consume(b'=') {
                        Symbol::FloorDivideAssignment
                    } else {
                        Symbol::FloorDivide
                    }
                } else if self.consume(b'=') {
                    Symbol::DivideAssignment
                } else {
                    Symbol::Divide
                };

                TokenKind::Symbol(symbol)
            }

            b'+' => self.assignment(Symbol::Add, Symbol::AddAssignment),
            b'*' => self.assignment(Symbol::Multiply, Symbol::MultiplyAssignment),
            b'%' => self.assignment(Symbol::Modulo, Symbol::ModuloAssignment),
            b'^' => self.assignment(Symbol::Power, Symbol::PowerAssignment),
            b'=' => self.assignment(Symbol::Assignment, Symbol::Equal),
            b'<' => self.assignment(Symbol::LessThan, Symbol::LessThanOrEqual),
            b'>' => self.assignment(Symbol::GreaterThan, Symbol::GreaterThanOrEqual),
            b'~' => self.assignment(Symbol::Tilde, Symbol::NotEqual),

            b':' => {
                self.cursor += 1;

                let symbol = if self.consume(b':') {
                    Symbol::DoubleColon
                } else {
                    Symbol::Colon
                };

                TokenKind::Symbol(symbol)
            }

            b'{' => {
                if !self.braces.is_empty() {
                    self.braces.push(Brace::Normal);
                }

                self.symbol(Symbol::LeftBrace)
            }

            b'}' => {
                self.cursor += 1;

                if matches!(self.braces.pop(), Some(Brace::Interpolation)) {
                    self.interpolated_string(
                        TokenKind::InterpolatedStringMiddle,
                        TokenKind::InterpolatedStringEnd,
                    )
                } else {
                    TokenKind::Symbol(Symbol::RightBrace)
                }
            }

            b'@' => {
                self.cursor += 1;

                if self.consume(b'[') {
                    TokenKind::Symbol(Symbol::AttributeOpen)
                } else {
                    if self.peek(0).is_some_and(is_name_start) {
                        self.name_body();
                    }

                    TokenKind::Attribute
                }
            }

            b'(' => self.symbol(Symbol::LeftParenthesis),
            b')' => self.symbol(Symbol::RightParenthesis),
            b']' => self.symbol(Symbol::RightBracket),
            b',' => self.symbol(Symbol::Comma),
            b';' => self.symbol(Symbol::Semicolon),
            b'#' => self.symbol(Symbol::Length),
            b'?' => self.symbol(Symbol::QuestionMark),
            b'&' => self.symbol(Symbol::Ampersand),
            b'|' => self.symbol(Symbol::Pipe),
            0x80..=0xff => self.unicode(),

            byte => {
                self.cursor += 1;

                TokenKind::InvalidCharacter { byte }
            }
        }
    }

    fn symbol(&mut self, symbol: Symbol) -> TokenKind {
        self.cursor += 1;

        TokenKind::Symbol(symbol)
    }

    fn assignment(&mut self, plain: Symbol, paired: Symbol) -> TokenKind {
        self.cursor += 1;

        TokenKind::Symbol(if self.consume(b'=') { paired } else { plain })
    }

    fn name_body(&mut self) {
        while self
            .peek(0)
            .is_some_and(|byte| is_name_start(byte) || byte.is_ascii_digit())
        {
            self.cursor += 1;
        }
    }

    fn name(&mut self) -> TokenKind {
        let start = self.cursor;
        self.name_body();

        let keyword = match &self.source[start..self.cursor] {
            b"and" => Keyword::And,
            b"break" => Keyword::Break,
            b"do" => Keyword::Do,
            b"else" => Keyword::Else,
            b"elseif" => Keyword::ElseIf,
            b"end" => Keyword::End,
            b"false" => Keyword::False,
            b"for" => Keyword::For,
            b"function" => Keyword::Function,
            b"if" => Keyword::If,
            b"in" => Keyword::In,
            b"local" => Keyword::Local,
            b"nil" => Keyword::Nil,
            b"not" => Keyword::Not,
            b"or" => Keyword::Or,
            b"repeat" => Keyword::Repeat,
            b"return" => Keyword::Return,
            b"then" => Keyword::Then,
            b"true" => Keyword::True,
            b"until" => Keyword::Until,
            b"while" => Keyword::While,
            _ => return TokenKind::Name,
        };

        TokenKind::Keyword(keyword)
    }

    fn number(&mut self) -> TokenKind {
        while self
            .peek(0)
            .is_some_and(|byte| byte.is_ascii_digit() || matches!(byte, b'.' | b'_'))
        {
            self.cursor += 1;
        }

        if matches!(self.peek(0), Some(b'e' | b'E')) {
            self.cursor += 1;

            if matches!(self.peek(0), Some(b'+' | b'-')) {
                self.cursor += 1;
            }
        }

        self.name_body();

        TokenKind::Number
    }

    fn separator(&mut self, bracket: u8) -> (usize, bool) {
        self.cursor += 1;
        let start = self.cursor;

        while self.consume(b'=') {}

        (self.cursor - start, self.peek(0) == Some(bracket))
    }

    fn long_string(&mut self, equals: usize, kind: TokenKind, malformed: TokenKind) -> TokenKind {
        self.cursor += 1;

        while let Some(byte) = self.peek(0) {
            if byte == 0 {
                break;
            }

            if byte == b']' {
                let (closing_equals, closed) = self.separator(b']');

                if closed && closing_equals == equals {
                    self.cursor += 1;

                    return kind;
                }
            } else {
                self.cursor += 1;
            }
        }

        malformed
    }

    fn comment(&mut self) -> TokenKind {
        if self.peek(0) == Some(b'[') {
            let (equals, closed) = self.separator(b'[');

            if closed {
                return self.long_string(
                    equals,
                    TokenKind::BlockComment,
                    TokenKind::MalformedComment,
                );
            }
        }

        while self
            .peek(0)
            .is_some_and(|byte| !matches!(byte, 0 | b'\r' | b'\n'))
        {
            self.cursor += 1;
        }

        TokenKind::Comment
    }

    fn escape(&mut self) {
        self.cursor += 1;

        match self.peek(0) {
            Some(b'\r') => {
                self.cursor += 1;
                self.consume(b'\n');
            }

            Some(b'z') => {
                self.cursor += 1;

                while self.peek(0).is_some_and(is_space) {
                    self.cursor += 1;
                }
            }

            None | Some(0) => {}
            Some(_) => self.cursor += 1,
        }
    }

    fn quoted_string(&mut self, quote: u8) -> TokenKind {
        self.cursor += 1;

        loop {
            match self.peek(0) {
                Some(byte) if byte == quote => {
                    self.cursor += 1;

                    return TokenKind::QuotedString;
                }

                None | Some(0 | b'\r' | b'\n') => return TokenKind::MalformedString,
                Some(b'\\') => self.escape(),
                Some(_) => self.cursor += 1,
            }
        }
    }

    fn interpolated_string(&mut self, continuation: TokenKind, ending: TokenKind) -> TokenKind {
        loop {
            match self.peek(0) {
                Some(b'`') => {
                    self.cursor += 1;

                    return ending;
                }

                None | Some(0 | b'\r' | b'\n') => return TokenKind::MalformedString,

                Some(b'\\') if self.peek(1) == Some(b'u') && self.peek(2) == Some(b'{') => {
                    self.cursor += 3;
                }

                Some(b'\\') => self.escape(),

                Some(b'{') => {
                    self.braces.push(Brace::Interpolation);
                    self.cursor += 1;

                    if self.consume(b'{') {
                        return TokenKind::InvalidInterpolationDoubleBrace;
                    }

                    return continuation;
                }

                Some(_) => self.cursor += 1,
            }
        }
    }

    fn unicode(&mut self) -> TokenKind {
        let first = self.source[self.cursor];
        self.cursor += 1;

        let (width, mut codepoint) = match first {
            0xc0..=0xdf => (2, u32::from(first & 0x1f)),
            0xe0..=0xef => (3, u32::from(first & 0x0f)),
            0xf0..=0xf7 => (4, u32::from(first & 0x07)),
            _ => return TokenKind::InvalidUnicode { codepoint: 0 },
        };

        for _ in 1..width {
            let Some(byte @ 0x80..=0xbf) = self.peek(0) else {
                return TokenKind::InvalidUnicode { codepoint: 0 };
            };

            self.cursor += 1;
            codepoint = (codepoint << 6) | u32::from(byte & 0x3f);
        }

        TokenKind::InvalidUnicode { codepoint }
    }
}

impl Iterator for Lexer<'_> {
    type Item = Token;

    fn next(&mut self) -> Option<Token> {
        if self.finished {
            return None;
        }

        let start = self.cursor;

        let kind = if let Some(byte) = self.peek(0) {
            self.scan(byte)
        } else {
            self.finished = true;

            TokenKind::EndOfFile
        };

        Some(Token {
            kind,
            span: Span {
                start,
                end: self.cursor,
            },
        })
    }
}

impl FusedIterator for Lexer<'_> {}

fn is_name_start(byte: u8) -> bool {
    byte.is_ascii_alphabetic() || byte == b'_'
}

fn is_space(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\r' | b'\n' | 0x0b | 0x0c)
}
