use std::str::{self, Utf8Error};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
/// Reserved Luau keywords.
pub enum Keyword {
    /// `and`.
    And,

    /// `break`.
    Break,

    /// `do`.
    Do,

    /// `else`.
    Else,

    /// `elseif`.
    ElseIf,

    /// `end`.
    End,

    /// `false`.
    False,

    /// `for`.
    For,

    /// `function`.
    Function,

    /// `if`.
    If,

    /// `in`.
    In,

    /// `local`.
    Local,

    /// `nil`.
    Nil,

    /// `not`.
    Not,

    /// `or`.
    Or,

    /// `repeat`.
    Repeat,

    /// `return`.
    Return,

    /// `then`.
    Then,

    /// `true`.
    True,

    /// `until`.
    Until,

    /// `while`.
    While,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
/// Luau punctuation and operators.
pub enum Symbol {
    /// `(`.
    LeftParenthesis,

    /// `)`.
    RightParenthesis,

    /// `{`.
    LeftBrace,

    /// `}`.
    RightBrace,

    /// `[`.
    LeftBracket,

    /// `]`.
    RightBracket,

    /// `,`.
    Comma,

    /// `;`.
    Semicolon,

    /// `:`.
    Colon,

    /// `::`.
    DoubleColon,

    /// `.`.
    Dot,

    /// `+`.
    Add,

    /// `-`.
    Subtract,

    /// `*`.
    Multiply,

    /// `/`.
    Divide,

    /// `//`.
    FloorDivide,

    /// `%`.
    Modulo,

    /// `^`.
    Power,

    /// `#`.
    Length,

    /// `..`.
    Concatenate,

    /// `...`.
    Ellipsis,

    /// `=`.
    Assignment,

    /// `==`.
    Equal,

    /// `~=`.
    NotEqual,

    /// `<`.
    LessThan,

    /// `<=`.
    LessThanOrEqual,

    /// `>`.
    GreaterThan,

    /// `>=`.
    GreaterThanOrEqual,

    /// `+=`.
    AddAssignment,

    /// `-=`.
    SubtractAssignment,

    /// `*=`.
    MultiplyAssignment,

    /// `/=`.
    DivideAssignment,

    /// `//=`.
    FloorDivideAssignment,

    /// `%=`.
    ModuloAssignment,

    /// `^=`.
    PowerAssignment,

    /// `..=`.
    ConcatenateAssignment,

    /// `->`.
    Arrow,

    /// `?`.
    QuestionMark,

    /// `&`.
    Ampersand,

    /// `|`.
    Pipe,

    /// `@`.
    AtSign,

    /// `@[`.
    AttributeOpen,

    /// `~`.
    Tilde,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
/// Lexical token categories.
pub enum TokenKind {
    /// End of input.
    EndOfFile,

    /// Whitespace bytes.
    Whitespace,

    /// Line comment.
    Comment,

    /// Long bracket comment.
    BlockComment,

    /// Identifier or contextual keyword.
    Name,

    /// Numeric literal.
    Number,

    /// Long bracket string.
    RawString,

    /// Single- or double-quoted string.
    QuotedString,

    /// Interpolation text before the first expression.
    InterpolatedStringStart,

    /// Interpolation text between expressions.
    InterpolatedStringMiddle,

    /// Interpolation text after the last expression.
    InterpolatedStringEnd,

    /// Backtick string without expressions.
    InterpolatedStringSimple,

    /// Attribute name, including `@`.
    Attribute,

    /// Reserved keyword.
    Keyword(Keyword),

    /// Punctuation or operator.
    Symbol(Symbol),

    /// Unterminated or malformed string.
    MalformedString,

    /// Unterminated long comment.
    MalformedComment,

    /// Non-ASCII input outside a string or comment.
    InvalidUnicode {
        /// Decoded code point, or zero if decoding fails.
        codepoint: u32,
    },

    /// Doubled opening brace in interpolation text.
    InvalidInterpolationDoubleBrace,

    /// Unexpected byte.
    InvalidCharacter {
        /// Unexpected source byte.
        byte: u8,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
/// Half-open byte range in the source.
pub struct Span {
    /// Inclusive byte offset.
    pub start: usize,

    /// Exclusive byte offset.
    pub end: usize,
}

impl Span {
    /// Whether the range contains no bytes.
    pub fn is_empty(self) -> bool {
        self.start == self.end
    }

    /// Returns the source bytes in this range.
    pub fn bytes(self, source: &[u8]) -> &[u8] {
        &source[self.start..self.end]
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
/// Token category and source range.
pub struct Token {
    /// Lexical category.
    pub kind: TokenKind,

    /// Source byte range.
    pub span: Span,
}

impl Token {
    /// Returns the token’s source bytes.
    pub fn bytes(self, source: &[u8]) -> &[u8] {
        self.span.bytes(source)
    }

    /// Decodes the token’s source bytes as UTF-8.
    ///
    /// # Errors
    /// Returns an error if the token is not valid UTF-8.
    pub fn utf8(self, source: &[u8]) -> Result<&str, Utf8Error> {
        str::from_utf8(self.bytes(source))
    }

    /// Returns the range used when reporting a lexical error.
    pub fn diagnostic_span(self) -> Span {
        if self.kind == TokenKind::InvalidInterpolationDoubleBrace {
            Span {
                start: self.span.start,
                end: self.span.end - 2,
            }
        } else {
            self.span
        }
    }
}
