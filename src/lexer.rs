use bstr::{BStr, ByteSlice};

use crate::syntax::{InterpolatedKind, Keyword, LexError, Operator, Span, Token, TokenKind};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BraceKind {
    Interpolated,
    Normal,
}

pub struct Lexer<'source> {
    source: &'source [u8],
    cursor: usize,
    braces: Vec<BraceKind>,
    finished: bool,
}

impl<'source> Lexer<'source> {
    #[must_use]
    pub fn new(source: &'source BStr) -> Self {
        Self {
            source: source.as_bytes(),
            cursor: 0,
            braces: Vec::new(),
            finished: false,
        }
    }

    fn current(&self) -> Option<u8> {
        self.peek(0)
    }

    fn peek(&self, lookahead: usize) -> Option<u8> {
        self.source.get(self.cursor + lookahead).copied()
    }

    fn advance(&mut self) {
        self.cursor += 1;
    }

    fn scan(&mut self) -> TokenKind {
        match self.current().expect("scan is only called before eof") {
            byte if is_space(byte) => self.whitespace(),

            b'-' => self.minus(),
            b'[' => self.bracket(),
            b'{' => self.left_brace(),
            b'}' => self.right_brace(),

            b'=' => self.equal(),
            b'<' => self.less(),
            b'>' => self.greater(),
            b'~' => self.tilde(),

            b'\'' | b'"' => self.quoted_string(),
            b'`' => self.interpolated_string(),

            b'.' => self.dot(),
            b'+' => self.plus(),
            b'/' => self.slash(),
            b'*' => self.star(),
            b'%' => self.percent(),
            b'^' => self.caret(),
            b':' => self.colon(),
            b'@' => self.attribute(),

            byte if byte.is_ascii_digit() => self.number(),
            byte if is_name_start(byte) => self.name(),
            byte if byte & 0x80 != 0 => self.broken_unicode(),

            byte => {
                self.advance();
                TokenKind::Byte(byte)
            }
        }
    }

    fn whitespace(&mut self) -> TokenKind {
        while self.current().is_some_and(is_space) {
            self.advance();
        }

        TokenKind::Whitespace
    }

    fn minus(&mut self) -> TokenKind {
        self.advance();

        match self.current() {
            Some(b'>') => {
                self.advance();
                TokenKind::Operator(Operator::Arrow)
            }
            Some(b'=') => {
                self.advance();
                TokenKind::Operator(Operator::SubtractAssign)
            }
            Some(b'-') => self.comment(),
            _ => TokenKind::Byte(b'-'),
        }
    }

    fn comment(&mut self) -> TokenKind {
        self.advance();

        if self.current() == Some(b'[')
            && let Separator::Valid(depth) = self.long_separator()
        {
            self.advance();
            return self.long_body(depth, TokenKind::BlockComment, LexError::BrokenComment);
        }

        while !matches!(self.current(), None | Some(0 | b'\r' | b'\n')) {
            self.advance();
        }

        TokenKind::Comment
    }

    fn bracket(&mut self) -> TokenKind {
        match self.long_separator() {
            Separator::Valid(depth) => {
                self.advance();
                self.long_body(depth, TokenKind::RawString, LexError::BrokenString)
            }
            Separator::Malformed(0) => TokenKind::Byte(b'['),
            Separator::Malformed(_) => TokenKind::Error(LexError::BrokenString),
        }
    }

    fn long_separator(&mut self) -> Separator {
        let bracket = self
            .current()
            .expect("long separator starts with a bracket");
        self.advance();

        let mut depth = 0;

        while self.current() == Some(b'=') {
            depth += 1;
            self.advance();
        }

        if self.current() == Some(bracket) {
            Separator::Valid(depth)
        } else {
            Separator::Malformed(depth)
        }
    }

    fn long_body(&mut self, depth: usize, complete: TokenKind, broken: LexError) -> TokenKind {
        loop {
            match self.current() {
                None | Some(0) => return TokenKind::Error(broken),
                Some(b']') => {
                    if self.long_separator() == Separator::Valid(depth) {
                        self.advance();
                        return complete;
                    }
                }
                Some(_) => self.advance(),
            }
        }
    }

    fn left_brace(&mut self) -> TokenKind {
        self.advance();

        if !self.braces.is_empty() {
            self.braces.push(BraceKind::Normal);
        }

        TokenKind::Byte(b'{')
    }

    fn right_brace(&mut self) -> TokenKind {
        self.advance();

        match self.braces.pop() {
            Some(BraceKind::Interpolated) => {
                self.interpolated_section(InterpolatedKind::Middle, InterpolatedKind::End)
            }
            Some(BraceKind::Normal) | None => TokenKind::Byte(b'}'),
        }
    }

    fn equal(&mut self) -> TokenKind {
        self.advance();

        if self.current() == Some(b'=') {
            self.advance();
            TokenKind::Operator(Operator::Equal)
        } else {
            TokenKind::Byte(b'=')
        }
    }

    fn less(&mut self) -> TokenKind {
        self.advance();

        if self.current() == Some(b'=') {
            self.advance();
            TokenKind::Operator(Operator::LessEqual)
        } else {
            TokenKind::Byte(b'<')
        }
    }

    fn greater(&mut self) -> TokenKind {
        self.advance();

        if self.current() == Some(b'=') {
            self.advance();
            TokenKind::Operator(Operator::GreaterEqual)
        } else {
            TokenKind::Byte(b'>')
        }
    }

    fn tilde(&mut self) -> TokenKind {
        self.advance();

        if self.current() == Some(b'=') {
            self.advance();
            TokenKind::Operator(Operator::NotEqual)
        } else {
            TokenKind::Byte(b'~')
        }
    }

    fn quoted_string(&mut self) -> TokenKind {
        let delimiter = self.current().expect("quoted string starts with a quote");
        self.advance();

        loop {
            match self.current() {
                None | Some(0 | b'\r' | b'\n') => {
                    return TokenKind::Error(LexError::BrokenString);
                }
                Some(byte) if byte == delimiter => {
                    self.advance();
                    return TokenKind::QuotedString;
                }
                Some(b'\\') => self.backslash(),
                Some(_) => self.advance(),
            }
        }
    }

    fn backslash(&mut self) {
        self.advance();

        match self.current() {
            Some(b'\r') => {
                self.advance();

                if self.current() == Some(b'\n') {
                    self.advance();
                }
            }
            None | Some(0) => {}
            Some(b'z') => {
                self.advance();

                while self.current().is_some_and(is_space) {
                    self.advance();
                }
            }
            Some(_) => self.advance(),
        }
    }

    fn interpolated_string(&mut self) -> TokenKind {
        self.advance();
        self.interpolated_section(InterpolatedKind::Begin, InterpolatedKind::Simple)
    }

    fn interpolated_section(
        &mut self,
        expression: InterpolatedKind,
        complete: InterpolatedKind,
    ) -> TokenKind {
        loop {
            match self.current() {
                None | Some(0 | b'\r' | b'\n') => {
                    return TokenKind::Error(LexError::BrokenString);
                }
                Some(b'\\') if self.peek(1) == Some(b'u') && self.peek(2) == Some(b'{') => {
                    self.advance();
                    self.advance();
                    self.advance();
                }
                Some(b'\\') => self.backslash(),
                Some(b'{') => {
                    self.braces.push(BraceKind::Interpolated);

                    if self.peek(1) == Some(b'{') {
                        self.advance();
                        self.advance();
                        return TokenKind::Error(LexError::BrokenInterpolatedDoubleBrace);
                    }

                    self.advance();
                    return TokenKind::Interpolated(expression);
                }
                Some(b'`') => {
                    self.advance();
                    return TokenKind::Interpolated(complete);
                }
                Some(_) => self.advance(),
            }
        }
    }

    fn dot(&mut self) -> TokenKind {
        self.advance();

        if self.current() == Some(b'.') {
            self.advance();

            return match self.current() {
                Some(b'.') => {
                    self.advance();
                    TokenKind::Operator(Operator::Ellipsis)
                }
                Some(b'=') => {
                    self.advance();
                    TokenKind::Operator(Operator::ConcatAssign)
                }
                _ => TokenKind::Operator(Operator::Concat),
            };
        }

        if self.current().is_some_and(|byte| byte.is_ascii_digit()) {
            return self.number();
        }

        TokenKind::Byte(b'.')
    }

    fn plus(&mut self) -> TokenKind {
        self.assignment(b'+', Operator::AddAssign)
    }

    fn slash(&mut self) -> TokenKind {
        self.advance();

        match self.current() {
            Some(b'=') => {
                self.advance();
                TokenKind::Operator(Operator::DivideAssign)
            }
            Some(b'/') => {
                self.advance();

                if self.current() == Some(b'=') {
                    self.advance();
                    TokenKind::Operator(Operator::FloorDivideAssign)
                } else {
                    TokenKind::Operator(Operator::FloorDivide)
                }
            }
            _ => TokenKind::Byte(b'/'),
        }
    }

    fn star(&mut self) -> TokenKind {
        self.assignment(b'*', Operator::MultiplyAssign)
    }

    fn percent(&mut self) -> TokenKind {
        self.assignment(b'%', Operator::ModuloAssign)
    }

    fn caret(&mut self) -> TokenKind {
        self.assignment(b'^', Operator::PowerAssign)
    }

    fn assignment(&mut self, byte: u8, operator: Operator) -> TokenKind {
        self.advance();

        if self.current() == Some(b'=') {
            self.advance();
            TokenKind::Operator(operator)
        } else {
            TokenKind::Byte(byte)
        }
    }

    fn colon(&mut self) -> TokenKind {
        self.advance();

        if self.current() == Some(b':') {
            self.advance();
            TokenKind::Operator(Operator::DoubleColon)
        } else {
            TokenKind::Byte(b':')
        }
    }

    fn attribute(&mut self) -> TokenKind {
        self.advance();

        if self.current() == Some(b'[') {
            self.advance();
            return TokenKind::AttributeOpen;
        }

        if self.current().is_some_and(is_name_start) {
            self.name_body();
        }

        TokenKind::Attribute
    }

    fn name(&mut self) -> TokenKind {
        let start = self.cursor;
        self.name_body();

        classify_name(BStr::new(&self.source[start..self.cursor]))
    }

    fn name_body(&mut self) {
        self.advance();

        while self.current().is_some_and(is_name_continue) {
            self.advance();
        }
    }

    fn number(&mut self) -> TokenKind {
        while self
            .current()
            .is_some_and(|byte| byte.is_ascii_digit() || matches!(byte, b'.' | b'_'))
        {
            self.advance();
        }

        if matches!(self.current(), Some(b'e' | b'E')) {
            self.advance();

            if matches!(self.current(), Some(b'+' | b'-')) {
                self.advance();
            }
        }

        while self.current().is_some_and(is_name_continue) {
            self.advance();
        }

        TokenKind::Number
    }

    fn broken_unicode(&mut self) -> TokenKind {
        let first = self.current().expect("unicode error starts before eof");

        let size = if first & 0b1110_0000 == 0b1100_0000 {
            2
        } else if first & 0b1111_0000 == 0b1110_0000 {
            3
        } else if first & 0b1111_1000 == 0b1111_0000 {
            4
        } else {
            self.advance();
            return TokenKind::Error(LexError::BrokenUnicode);
        };

        self.advance();

        for _ in 1..size {
            let Some(byte) = self.current() else {
                return TokenKind::Error(LexError::BrokenUnicode);
            };

            if byte & 0b1100_0000 != 0b1000_0000 {
                return TokenKind::Error(LexError::BrokenUnicode);
            }

            self.advance();
        }

        TokenKind::Error(LexError::BrokenUnicode)
    }
}

impl Iterator for Lexer<'_> {
    type Item = Token;

    fn next(&mut self) -> Option<Self::Item> {
        if self.finished {
            return None;
        }

        let start = self.cursor;
        let kind = if self.cursor == self.source.len() {
            self.finished = true;
            TokenKind::Eof
        } else {
            self.scan()
        };

        debug_assert!(kind == TokenKind::Eof || self.cursor > start);

        Some(Token {
            kind,
            span: Span {
                start,
                end: self.cursor,
            },
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Separator {
    Valid(usize),
    Malformed(usize),
}

#[must_use]
pub fn tokenize(source: &BStr) -> Vec<Token> {
    Lexer::new(source).collect()
}

#[must_use]
pub fn classify_name(name: &BStr) -> TokenKind {
    match Keyword::from_name(name) {
        Some(keyword) => TokenKind::Keyword(keyword),
        None => TokenKind::Name,
    }
}

fn is_space(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\r' | b'\n' | 0x0b | 0x0c)
}

fn is_name_start(byte: u8) -> bool {
    byte.is_ascii_alphabetic() || byte == b'_'
}

fn is_name_continue(byte: u8) -> bool {
    is_name_start(byte) || byte.is_ascii_digit()
}
