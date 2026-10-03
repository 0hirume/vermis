use crate::parser::control::Execution;
use crate::syntax::{InterpolatedKind, Keyword, LexError, Operator, Span, Token, TokenKind};
use std::{collections::HashSet, fmt, sync::Arc};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Brace {
    Interpolated,
    Normal,
}

struct Link {
    brace: Brace,
    tail: Option<Arc<Link>>,
}

impl Drop for Link {
    fn drop(&mut self) {
        let mut tail = self.tail.take();

        while let Some(mut link) = tail.and_then(Arc::into_inner) {
            tail = link.tail.take();
        }
    }
}

#[derive(Clone, Default)]
pub(crate) struct Braces {
    head: Option<Arc<Link>>,
    length: usize,
}

impl Braces {
    pub(crate) fn len(&self) -> usize {
        self.length
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.length == 0
    }

    pub(crate) fn equivalent(&self, other: &Self, pairs: &mut HashSet<(usize, usize)>) -> bool {
        if self.length != other.length {
            return false;
        }

        let mut left = self.head.as_ref();
        let mut right = other.head.as_ref();
        let mut verified = Vec::new();

        while let (Some(first), Some(second)) = (left, right) {
            if Arc::ptr_eq(first, second) {
                break;
            }

            let pair = (Arc::as_ptr(first) as usize, Arc::as_ptr(second) as usize);

            if pairs.contains(&pair) {
                break;
            }

            if first.brace != second.brace {
                return false;
            }

            verified.push(pair);
            left = first.tail.as_ref();
            right = second.tail.as_ref();
        }

        pairs.extend(verified);

        true
    }

    fn push(&mut self, brace: Brace) {
        self.head = Some(Arc::new(Link {
            brace,
            tail: self.head.take(),
        }));

        self.length += 1;
    }

    fn pop(&mut self) -> Option<Brace> {
        let head = self.head.take()?;
        self.head.clone_from(&head.tail);
        self.length -= 1;

        Some(head.brace)
    }
}

impl<const LENGTH: usize> From<[Brace; LENGTH]> for Braces {
    fn from(braces: [Brace; LENGTH]) -> Self {
        let mut stack = Self::default();

        for brace in braces {
            stack.push(brace);
        }

        stack
    }
}

impl PartialEq for Braces {
    fn eq(&self, other: &Self) -> bool {
        if self.length != other.length {
            return false;
        }

        let mut left = self.head.as_ref();
        let mut right = other.head.as_ref();

        while let (Some(first), Some(second)) = (left, right) {
            if Arc::ptr_eq(first, second) {
                return true;
            }

            if first.brace != second.brace {
                return false;
            }

            left = first.tail.as_ref();
            right = second.tail.as_ref();
        }

        true
    }
}

impl Eq for Braces {}

impl fmt::Debug for Braces {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut list = formatter.debug_list();
        let mut cursor = self.head.as_ref();

        while let Some(link) = cursor {
            list.entry(&link.brace);
            cursor = link.tail.as_ref();
        }

        list.finish()
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct State {
    pub braces: Braces,
}

impl State {
    pub(crate) fn equivalent(&self, other: &Self, pairs: &mut HashSet<(usize, usize)>) -> bool {
        self.braces.equivalent(&other.braces, pairs)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Checkpoint {
    pub cursor: usize,
    pub state: State,
    pub finished: bool,
}

impl Checkpoint {
    pub(crate) fn equivalent(&self, other: &Self, pairs: &mut HashSet<(usize, usize)>) -> bool {
        self.cursor == other.cursor
            && self.finished == other.finished
            && self.state.equivalent(&other.state, pairs)
    }
}

pub struct Lexer<'source> {
    source: &'source [u8],
    cursor: usize,
    braces: Braces,
    finished: bool,
    execution: Option<Arc<Execution>>,
}

impl<'source> Lexer<'source> {
    #[must_use]
    pub fn new(source: &'source [u8]) -> Self {
        Self {
            source,
            cursor: 0,
            braces: Braces::default(),
            finished: false,
            execution: None,
        }
    }

    pub(crate) fn state(&self) -> State {
        State {
            braces: self.braces.clone(),
        }
    }

    pub(crate) fn checkpoint(&self) -> Checkpoint {
        Checkpoint {
            cursor: self.cursor,
            state: self.state(),
            finished: self.finished,
        }
    }

    pub(crate) fn restore(&mut self, checkpoint: &Checkpoint) {
        self.cursor = checkpoint.cursor;
        self.braces.clone_from(&checkpoint.state.braces);
        self.finished = checkpoint.finished;
    }

    pub(crate) fn from_state(source: &'source [u8], cursor: usize, state: &State) -> Self {
        let mut lexer = Self::new(source);

        lexer.restore(&Checkpoint {
            cursor,
            state: state.clone(),
            finished: false,
        });

        lexer
    }

    pub(crate) fn controlled(
        source: &'source [u8],
        cursor: usize,
        state: &State,
        execution: Option<Arc<Execution>>,
    ) -> Self {
        let mut lexer = Self::from_state(source, cursor, state);
        lexer.execution = execution;

        lexer
    }

    fn active(&self) -> bool {
        self.execution
            .as_ref()
            .is_none_or(|execution| execution.poll())
    }

    fn required(&self) -> u8 {
        self.current().unwrap_or_else(|| {
            assert!(!self.active(), "scanner requires an input byte");

            0
        })
    }

    fn brace(&mut self, brace: Brace) {
        if self
            .execution
            .as_ref()
            .is_none_or(|execution| execution.depth(self.braces.len().saturating_add(1)))
        {
            self.braces.push(brace);
        }
    }

    fn current(&self) -> Option<u8> {
        self.peek(0)
    }

    fn peek(&self, lookahead: usize) -> Option<u8> {
        if !self.active() {
            return None;
        }

        self.source.get(self.cursor + lookahead).copied()
    }

    fn advance(&mut self) {
        self.cursor += 1;
    }

    fn scan(&mut self) -> TokenKind {
        match self.required() {
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
        let bracket = self.required();

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
            self.brace(Brace::Normal);
        }

        TokenKind::Byte(b'{')
    }

    fn right_brace(&mut self) -> TokenKind {
        self.advance();

        match self.braces.pop() {
            Some(Brace::Interpolated) => {
                self.interpolated_section(InterpolatedKind::Middle, InterpolatedKind::End)
            }

            Some(Brace::Normal) | None => TokenKind::Byte(b'}'),
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
        let delimiter = self.required();
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
                    self.brace(Brace::Interpolated);

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

        classify_name(&self.source[start..self.cursor])
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
        let first = self.required();

        let (size, prefix) = if first & 0b1110_0000 == 0b1100_0000 {
            (2, first & 0b0001_1111)
        } else if first & 0b1111_0000 == 0b1110_0000 {
            (3, first & 0b0000_1111)
        } else if first & 0b1111_1000 == 0b1111_0000 {
            (4, first & 0b0000_0111)
        } else {
            self.advance();

            return TokenKind::Error(LexError::BrokenUnicode { codepoint: 0 });
        };

        let mut codepoint = u32::from(prefix);

        self.advance();

        for _ in 1..size {
            let Some(byte) = self.current() else {
                return TokenKind::Error(LexError::BrokenUnicode { codepoint: 0 });
            };

            if byte & 0b1100_0000 != 0b1000_0000 {
                return TokenKind::Error(LexError::BrokenUnicode { codepoint: 0 });
            }

            codepoint = (codepoint << 6) | u32::from(byte & 0b0011_1111);

            self.advance();
        }

        TokenKind::Error(LexError::BrokenUnicode { codepoint })
    }
}

impl Iterator for Lexer<'_> {
    type Item = Token;

    fn next(&mut self) -> Option<Self::Item> {
        if self.finished || !self.active() {
            return None;
        }

        let start = self.cursor;

        let kind = if self.cursor == self.source.len() {
            self.finished = true;

            TokenKind::Eof
        } else {
            self.scan()
        };

        if !self.active() {
            self.finished = true;

            return None;
        }

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
pub fn classify_name(name: &[u8]) -> TokenKind {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::control::{Control, ParseError};
    use std::sync::atomic::{AtomicBool, Ordering};

    #[test]
    fn cancelled_long_body_scanner_does_not_advance() {
        let cancellation = Arc::new(AtomicBool::new(false));

        let execution = Execution::new(&Control {
            cancellation: Some(cancellation.clone()),
            ..Control::default()
        });

        let mut lexer = Lexer::controlled(
            b"[[long body]]",
            2,
            &State::default(),
            Some(execution.clone()),
        );

        cancellation.store(true, Ordering::Relaxed);

        assert_eq!(
            lexer.long_body(0, TokenKind::RawString, LexError::BrokenString),
            TokenKind::Error(LexError::BrokenString)
        );

        assert_eq!(lexer.cursor, 2);
        assert_eq!(execution.error(), Some(ParseError::Cancelled));
        assert!(lexer.next().is_none());
    }

    #[test]
    fn persistent_braces_drop_deep_unique_and_shared_tails_without_recursion() {
        std::thread::Builder::new()
            .stack_size(64 * 1024)
            .spawn(|| {
                let mut braces = Braces::default();

                for _ in 0..100_000 {
                    braces.push(Brace::Normal);
                }

                let snapshot = braces.clone();

                for _ in 0..50_000 {
                    assert_eq!(braces.pop(), Some(Brace::Normal));
                }

                assert_eq!(braces.len(), 50_000);
                assert_eq!(snapshot.len(), 100_000);
                drop(snapshot);
                drop(braces);
            })
            .unwrap()
            .join()
            .unwrap();
    }

    #[test]
    fn equivalent_checkpoints_cache_verified_tails_without_accepting_partial_matches() {
        let mut left = Braces::default();
        let mut right = Braces::default();
        let mut prefixes = Vec::new();

        for index in 0..10_000 {
            let brace = if index % 2 == 0 {
                Brace::Normal
            } else {
                Brace::Interpolated
            };

            left.push(brace);
            right.push(brace);
            prefixes.push((left.clone(), right.clone()));
        }

        assert_eq!(left, right);
        let mut pairs = HashSet::new();
        assert!(left.equivalent(&right, &mut pairs));
        assert!(!pairs.is_empty());
        assert!(pairs.len() <= prefixes.len());

        for (left, right) in &prefixes {
            assert!(left.equivalent(right, &mut pairs));
        }

        assert!(pairs.len() <= prefixes.len());

        let old = Checkpoint {
            cursor: 7,
            state: State { braces: left },
            finished: false,
        };

        let mut new = Checkpoint {
            cursor: 8,
            state: State { braces: right },
            finished: false,
        };

        assert!(!old.equivalent(&new, &mut pairs));
        new.cursor = 7;
        new.finished = true;
        assert!(!old.equivalent(&new, &mut pairs));
        new.finished = false;
        assert!(old.equivalent(&new, &mut pairs));

        let left = Braces::from([Brace::Interpolated, Brace::Normal, Brace::Normal]);
        let right = Braces::from([Brace::Normal, Brace::Normal, Brace::Normal]);
        let mut pairs = HashSet::new();
        assert!(!left.equivalent(&right, &mut pairs));
        assert!(pairs.is_empty());
        assert!(!left.equivalent(&right, &mut pairs));
        assert!(pairs.is_empty());
    }
}
