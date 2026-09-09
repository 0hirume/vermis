use std::str::{self, Utf8Error};

use bstr::{BStr, ByteSlice};

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

impl Span {
    #[must_use]
    pub const fn len(self) -> usize {
        self.end - self.start
    }

    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.start == self.end
    }

    #[must_use]
    pub fn bytes(self, source: &BStr) -> &BStr {
        BStr::new(&source.as_bytes()[self.start..self.end])
    }

    /// # Errors
    ///
    /// Returns an error when the spanned bytes are not valid UTF-8.
    pub fn utf8(self, source: &BStr) -> Result<&str, Utf8Error> {
        str::from_utf8(self.bytes(source).as_bytes())
    }
}

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub enum Keyword {
    And,
    Break,
    Do,
    Else,
    ElseIf,
    End,
    False,
    For,
    Function,
    If,
    In,
    Local,
    Nil,
    Not,
    Or,
    Repeat,
    Return,
    Then,
    True,
    Until,
    While,
}

impl Keyword {
    pub(crate) fn from_name(name: &BStr) -> Option<Self> {
        match name.as_bytes() {
            b"and" => Some(Self::And),
            b"break" => Some(Self::Break),
            b"do" => Some(Self::Do),
            b"else" => Some(Self::Else),
            b"elseif" => Some(Self::ElseIf),
            b"end" => Some(Self::End),
            b"false" => Some(Self::False),
            b"for" => Some(Self::For),
            b"function" => Some(Self::Function),
            b"if" => Some(Self::If),
            b"in" => Some(Self::In),
            b"local" => Some(Self::Local),
            b"nil" => Some(Self::Nil),
            b"not" => Some(Self::Not),
            b"or" => Some(Self::Or),
            b"repeat" => Some(Self::Repeat),
            b"return" => Some(Self::Return),
            b"then" => Some(Self::Then),
            b"true" => Some(Self::True),
            b"until" => Some(Self::Until),
            b"while" => Some(Self::While),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub enum Operator {
    Equal,
    LessEqual,
    GreaterEqual,
    NotEqual,
    Concat,
    Ellipsis,
    Arrow,
    DoubleColon,
    FloorDivide,
    AddAssign,
    SubtractAssign,
    MultiplyAssign,
    DivideAssign,
    FloorDivideAssign,
    ModuloAssign,
    PowerAssign,
    ConcatAssign,
}

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub enum InterpolatedKind {
    Begin,
    Middle,
    End,
    Simple,
}

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub enum LexError {
    BrokenString,
    BrokenComment,
    BrokenUnicode,
    BrokenInterpolatedDoubleBrace,
}

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub enum TokenKind {
    Eof,
    Whitespace,
    Comment,
    BlockComment,
    Name,
    Number,
    RawString,
    QuotedString,
    Interpolated(InterpolatedKind),
    Attribute,
    AttributeOpen,
    Keyword(Keyword),
    Operator(Operator),
    Byte(u8),
    Error(LexError),
}

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
}

impl Token {
    #[must_use]
    pub fn bytes(self, source: &BStr) -> &BStr {
        self.span.bytes(source)
    }

    /// # Errors
    ///
    /// Returns an error when the token bytes are not valid UTF-8.
    pub fn utf8(self, source: &BStr) -> Result<&str, Utf8Error> {
        self.span.utf8(source)
    }
}
