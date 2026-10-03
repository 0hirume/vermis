use std::borrow::Cow;

use super::Parser;
use crate::token::{Keyword, Symbol, TokenKind};
use crate::tree::{Diagnostic, ListEntry, NodeIndex, NodeKind, NodeList, TokenIndex};

impl Parser<'_> {
    /// Parses an expression, recovering missing operands.
    pub fn expression(&mut self) -> NodeIndex {
        self.subexpression(0)
    }

    fn subexpression(&mut self, minimum: u8) -> NodeIndex {
        self.required("expression", |parser| {
            if parser.depth >= 128 {
                return Err(parser.error("syntax nesting limit exceeded"));
            }

            parser.nested(|parser| parser.binary(minimum))
        })
    }

    fn binary(&mut self, minimum: u8) -> Result<NodeIndex, Diagnostic> {
        let start = self.position();

        let mut left = match self.current().kind {
            TokenKind::Symbol(Symbol::Subtract | Symbol::Length)
            | TokenKind::Keyword(Keyword::Not) => {
                let operator = self.take();
                let operand = self.subexpression(8);

                self.append_node(start, NodeKind::Unary { operator, operand })
            }

            _ => {
                let expression = self.atom()?;

                if let Some(operator) = self.consume(TokenKind::Symbol(Symbol::DoubleColon)) {
                    let annotation = self.annotation();

                    self.append_node(
                        start,
                        NodeKind::Assertion {
                            expression,
                            operator,
                            annotation,
                        },
                    )
                } else {
                    expression
                }
            }
        };

        while let Some((priority, right_priority)) = priority(self.current().kind) {
            if priority <= minimum {
                break;
            }

            let operator = self.take();
            let right = self.subexpression(right_priority);

            left = self.append_node(
                start,
                NodeKind::Binary {
                    left,
                    operator,
                    right,
                },
            );
        }

        Ok(left)
    }

    fn atom(&mut self) -> Result<NodeIndex, Diagnostic> {
        let start = self.position();

        let node = match self.current().kind {
            TokenKind::Number => {
                if !valid_number(self.current().bytes(self.source)) {
                    self.diagnose(self.error("malformed number"));
                }

                let token = self.take();

                self.append_node(start, NodeKind::Number { token })
            }

            TokenKind::QuotedString | TokenKind::RawString => self.string(),

            TokenKind::InterpolatedStringStart
            | TokenKind::InterpolatedStringSimple
            | TokenKind::InvalidInterpolationDoubleBrace => self.interpolation(),

            TokenKind::Keyword(Keyword::Nil) => {
                let token = self.take();

                self.append_node(start, NodeKind::Nil { token })
            }

            TokenKind::Keyword(Keyword::True | Keyword::False) => {
                let token = self.take();

                self.append_node(start, NodeKind::Boolean { token })
            }

            TokenKind::Symbol(Symbol::Ellipsis) => {
                let ellipsis = self.take();

                self.append_node(
                    start,
                    NodeKind::Variadic {
                        ellipsis,
                        colon: None,
                        annotation: None,
                    },
                )
            }

            TokenKind::Keyword(Keyword::Function)
            | TokenKind::Attribute
            | TokenKind::Symbol(Symbol::AttributeOpen) => self.function_expression(),

            TokenKind::Keyword(Keyword::If) => self.conditional(),
            TokenKind::Symbol(Symbol::LeftBrace) => self.table(),

            TokenKind::MalformedString
            | TokenKind::MalformedComment
            | TokenKind::InvalidUnicode { .. }
            | TokenKind::InvalidCharacter { .. } => {
                self.diagnose(self.error("expected expression"));
                self.take();

                self.append_node(start, NodeKind::Error)
            }

            _ => return self.primary(),
        };

        Ok(node)
    }

    pub(super) fn primary(&mut self) -> Result<NodeIndex, Diagnostic> {
        let start = self.position();

        let left = if let Some(opening) = self.consume(TokenKind::Symbol(Symbol::LeftParenthesis)) {
            let expression = self.expression();

            let closing = self.expect(
                TokenKind::Symbol(Symbol::RightParenthesis),
                "expected closing expression",
            );

            self.append_node(
                start,
                NodeKind::Group {
                    opening,
                    expression,
                    closing,
                },
            )
        } else if self.at(TokenKind::Name) {
            self.name()
        } else {
            return Err(self.error("expected expression"));
        };

        self.postfix(start, left)
    }

    fn postfix(&mut self, start: TokenIndex, mut left: NodeIndex) -> Result<NodeIndex, Diagnostic> {
        loop {
            let cursor = self.position();

            left = match self.current().kind {
                TokenKind::Symbol(Symbol::Dot) => {
                    let dot = self.take();
                    let name = self.name();

                    self.append_node(
                        start,
                        NodeKind::Field {
                            receiver: left,
                            dot,
                            name,
                        },
                    )
                }

                TokenKind::Symbol(Symbol::LeftBracket) => {
                    let opening = self.take();
                    let key = self.expression();

                    let closing = self.expect(
                        TokenKind::Symbol(Symbol::RightBracket),
                        "expected closing index",
                    );

                    self.append_node(
                        start,
                        NodeKind::Index {
                            receiver: left,
                            opening,
                            key,
                            closing,
                        },
                    )
                }

                TokenKind::Symbol(Symbol::Colon) => {
                    let colon = self.take();
                    let method = self.name();

                    let instantiation = if self.at(TokenKind::Symbol(Symbol::LessThan))
                        && self.lookahead() == TokenKind::Symbol(Symbol::LessThan)
                    {
                        Some(self.instantiation_arguments())
                    } else {
                        None
                    };

                    let arguments = self.required("arguments", Self::arguments);

                    self.append_node(
                        start,
                        NodeKind::MethodCall {
                            receiver: left,
                            colon,
                            method,
                            instantiation,
                            arguments,
                        },
                    )
                }

                TokenKind::Symbol(Symbol::LeftParenthesis | Symbol::LeftBrace)
                | TokenKind::QuotedString
                | TokenKind::RawString => {
                    let arguments = self.arguments()?;

                    self.append_node(
                        start,
                        NodeKind::Call {
                            callee: left,
                            arguments,
                        },
                    )
                }

                TokenKind::Symbol(Symbol::LessThan)
                    if self.lookahead() == TokenKind::Symbol(Symbol::LessThan) =>
                {
                    let arguments = self.instantiation_arguments();

                    self.append_node(
                        start,
                        NodeKind::Instantiate {
                            expression: left,
                            arguments,
                        },
                    )
                }

                _ => break,
            };

            if self.position() == cursor {
                break;
            }
        }

        Ok(left)
    }

    pub(super) fn expressions(&mut self) -> NodeList {
        let mut values = Vec::new();

        loop {
            let node = self.expression();
            let separator = self.consume(TokenKind::Symbol(Symbol::Comma));
            values.push(ListEntry { node, separator });

            if separator.is_none() {
                break;
            }
        }

        self.append_list(values)
    }

    pub(super) fn arguments(&mut self) -> Result<NodeIndex, Diagnostic> {
        let start = self.position();

        let (opening, values, closing) = match self.current().kind {
            TokenKind::Symbol(Symbol::LeftParenthesis) => {
                if self.source[self.end..self.current().span.start].contains(&b'\n') {
                    self.diagnose(self.error("ambiguous call across a newline"));
                }

                let opening = Some(self.take());

                let values = if self.at(TokenKind::Symbol(Symbol::RightParenthesis)) {
                    self.append_list([])
                } else {
                    self.expressions()
                };

                let closing = self.expect(
                    TokenKind::Symbol(Symbol::RightParenthesis),
                    "expected closing arguments",
                );

                (opening, values, closing)
            }

            TokenKind::Symbol(Symbol::LeftBrace) => {
                let node =
                    self.required("table", |parser| parser.nested(|parser| Ok(parser.table())));

                let values = self.append_list([ListEntry {
                    node,
                    separator: None,
                }]);

                (None, values, None)
            }

            TokenKind::QuotedString | TokenKind::RawString => {
                let node = self.string();

                let values = self.append_list([ListEntry {
                    node,
                    separator: None,
                }]);

                (None, values, None)
            }

            _ => return Err(self.error("expected call arguments")),
        };

        Ok(self.append_node(
            start,
            NodeKind::Arguments {
                opening,
                values,
                closing,
            },
        ))
    }

    fn table(&mut self) -> NodeIndex {
        let start = self.position();
        let opening = self.take();
        let mut fields = Vec::new();

        while !self.at(TokenKind::Symbol(Symbol::RightBrace)) && !self.at(TokenKind::EndOfFile) {
            let begin = self.position();
            let field_opening = self.consume(TokenKind::Symbol(Symbol::LeftBracket));

            let (key, closing, assignment) = if field_opening.is_some() {
                let key = Some(self.expression());

                let closing = self.expect(
                    TokenKind::Symbol(Symbol::RightBracket),
                    "expected closing field key",
                );

                let assignment = self.expect(
                    TokenKind::Symbol(Symbol::Assignment),
                    "expected field value",
                );

                (key, closing, assignment)
            } else if self.at(TokenKind::Name)
                && self.lookahead() == TokenKind::Symbol(Symbol::Assignment)
            {
                let key = Some(self.name());
                let assignment = Some(self.take());

                (key, None, assignment)
            } else {
                (None, None, None)
            };

            let value = self.expression();

            let node = self.append_node(
                begin,
                NodeKind::TableField {
                    opening: field_opening,
                    key,
                    closing,
                    assignment,
                    value,
                },
            );

            let separator = self
                .consume(TokenKind::Symbol(Symbol::Comma))
                .or_else(|| self.consume(TokenKind::Symbol(Symbol::Semicolon)));

            fields.push(ListEntry { node, separator });

            if self.position() == begin {
                break;
            }

            if separator.is_none() {
                if self.at(TokenKind::Name) || self.at(TokenKind::Symbol(Symbol::LeftBracket)) {
                    self.diagnose(self.error("expected table field separator"));
                } else {
                    break;
                }
            }
        }

        let closing = self.expect(
            TokenKind::Symbol(Symbol::RightBrace),
            "expected closing table",
        );

        let fields = self.append_list(fields);

        self.append_node(
            start,
            NodeKind::Table {
                opening,
                fields,
                closing,
            },
        )
    }

    fn conditional(&mut self) -> NodeIndex {
        let start = self.position();
        let keyword = self.take();
        let condition = self.condition();
        let then = self.expect(TokenKind::Keyword(Keyword::Then), "expected then");
        let truthy = self.expression();

        let (else_keyword, falsy) = if self.at(TokenKind::Keyword(Keyword::ElseIf)) {
            let falsy = self.required("conditional expression", |parser| {
                parser.nested(|parser| Ok(parser.conditional()))
            });

            (None, falsy)
        } else {
            let else_keyword = self.expect(TokenKind::Keyword(Keyword::Else), "expected else");

            (else_keyword, self.expression())
        };

        self.append_node(
            start,
            NodeKind::Conditional {
                keyword,
                condition,
                then,
                truthy,
                else_keyword,
                falsy,
            },
        )
    }

    pub(super) fn attributes(&mut self) -> NodeIndex {
        let start = self.position();
        let mut attributes = Vec::new();

        while matches!(
            self.current().kind,
            TokenKind::Attribute | TokenKind::Symbol(Symbol::AttributeOpen)
        ) {
            let begin = self.position();

            let node = if self.at(TokenKind::Attribute) {
                let token = self.take();
                let name = self.append_node(begin, NodeKind::Name { token });

                self.append_node(
                    begin,
                    NodeKind::Attribute {
                        name,
                        arguments: None,
                    },
                )
            } else {
                let opening = self.take();
                let mut entries = Vec::new();

                loop {
                    let attribute_start = self.position();
                    let name = self.name();

                    let arguments = if matches!(
                        self.current().kind,
                        TokenKind::Symbol(Symbol::LeftParenthesis | Symbol::LeftBrace)
                            | TokenKind::QuotedString
                            | TokenKind::RawString
                    ) {
                        Some(self.required("arguments", Self::arguments))
                    } else {
                        None
                    };

                    let node =
                        self.append_node(attribute_start, NodeKind::Attribute { name, arguments });

                    let separator = self.consume(TokenKind::Symbol(Symbol::Comma));
                    entries.push(ListEntry { node, separator });

                    if separator.is_none() {
                        break;
                    }
                }

                let closing = self.expect(
                    TokenKind::Symbol(Symbol::RightBracket),
                    "expected closing attributes",
                );

                let attributes = self.append_list(entries);

                self.append_node(
                    begin,
                    NodeKind::AttributeGroup {
                        opening,
                        attributes,
                        closing,
                    },
                )
            };

            attributes.push(ListEntry {
                node,
                separator: None,
            });
        }

        let attributes = self.append_list(attributes);

        self.append_node(start, NodeKind::Attributes { attributes })
    }

    pub(super) fn string(&mut self) -> NodeIndex {
        let start = self.position();

        if self.at(TokenKind::QuotedString) && !valid_escapes(self.current().bytes(self.source)) {
            self.diagnose(self.error("malformed string escape"));
        }

        let token = self.take();

        self.append_node(start, NodeKind::String { token })
    }

    fn interpolation(&mut self) -> NodeIndex {
        let start = self.position();
        let mut segments = Vec::new();

        loop {
            let begin = self.position();
            let kind = self.current().kind;

            let ending = matches!(
                kind,
                TokenKind::InterpolatedStringSimple | TokenKind::InterpolatedStringEnd
            );

            let node = match kind {
                TokenKind::InterpolatedStringStart
                | TokenKind::InterpolatedStringMiddle
                | TokenKind::InterpolatedStringEnd
                | TokenKind::InterpolatedStringSimple => {
                    if !valid_escapes(self.current().bytes(self.source)) {
                        self.diagnose(self.error("malformed interpolation escape"));
                    }

                    let token = self.take();

                    self.append_node(begin, NodeKind::String { token })
                }

                TokenKind::InvalidInterpolationDoubleBrace | TokenKind::MalformedString => {
                    self.diagnose(self.error("expected interpolation segment"));
                    self.take();

                    self.append_node(begin, NodeKind::Error)
                }

                _ => {
                    let node = self.missing(
                        "interpolation segment",
                        self.error("expected interpolation segment"),
                    );

                    segments.push(ListEntry {
                        node,
                        separator: None,
                    });

                    break;
                }
            };

            segments.push(ListEntry {
                node,
                separator: None,
            });

            if ending || kind == TokenKind::MalformedString {
                break;
            }

            let node = self.expression();

            segments.push(ListEntry {
                node,
                separator: None,
            });
        }

        let segments = self.append_list(segments);

        self.append_node(start, NodeKind::Interpolation { segments })
    }
}

fn priority(kind: TokenKind) -> Option<(u8, u8)> {
    Some(match kind {
        TokenKind::Keyword(Keyword::Or) => (1, 1),
        TokenKind::Keyword(Keyword::And) => (2, 2),

        TokenKind::Symbol(
            Symbol::Equal
            | Symbol::NotEqual
            | Symbol::LessThan
            | Symbol::LessThanOrEqual
            | Symbol::GreaterThan
            | Symbol::GreaterThanOrEqual,
        ) => (3, 3),

        TokenKind::Symbol(Symbol::Concatenate) => (5, 4),
        TokenKind::Symbol(Symbol::Add | Symbol::Subtract) => (6, 6),

        TokenKind::Symbol(
            Symbol::Multiply | Symbol::Divide | Symbol::FloorDivide | Symbol::Modulo,
        ) => (7, 7),

        TokenKind::Symbol(Symbol::Power) => (10, 9),
        _ => return None,
    })
}

fn valid_number(bytes: &[u8]) -> bool {
    let normalized = if bytes.contains(&b'_') {
        Cow::Owned(
            bytes
                .iter()
                .copied()
                .filter(|byte| *byte != b'_')
                .collect::<Vec<_>>(),
        )
    } else {
        Cow::Borrowed(bytes)
    };

    let Ok(text) = std::str::from_utf8(&normalized) else {
        return false;
    };

    let integer = text.ends_with('i');
    let digits = text.strip_suffix('i').unwrap_or(text);

    let radix = if digits.starts_with("0x") || digits.starts_with("0X") {
        16
    } else if digits.starts_with("0b") || digits.starts_with("0B") {
        2
    } else {
        10
    };

    if radix != 10 {
        let digits = &digits[2..];

        if integer {
            u64::from_str_radix(digits, radix).is_ok()
        } else {
            !digits.is_empty() && digits.chars().all(|digit| digit.is_digit(radix))
        }
    } else if integer {
        digits.parse::<i64>().is_ok()
    } else {
        digits.parse::<f64>().is_ok()
    }
}

fn valid_escapes(bytes: &[u8]) -> bool {
    let bytes = &bytes[1..bytes.len() - 1];
    let mut cursor = 0;

    while cursor < bytes.len() {
        if bytes[cursor] != b'\\' {
            cursor += 1;
            continue;
        }

        cursor += 1;

        let Some(&escape) = bytes.get(cursor) else {
            return false;
        };

        cursor += 1;

        match escape {
            0 => return false,

            b'x' => {
                if !bytes
                    .get(cursor..cursor + 2)
                    .is_some_and(|digits| digits.iter().all(u8::is_ascii_hexdigit))
                {
                    return false;
                }

                cursor += 2;
            }

            b'u' => {
                if bytes.get(cursor) != Some(&b'{') {
                    return false;
                }

                cursor += 1;
                let begin = cursor;

                while bytes.get(cursor).is_some_and(u8::is_ascii_hexdigit) {
                    cursor += 1;
                }

                if begin == cursor || cursor - begin > 16 || bytes.get(cursor) != Some(&b'}') {
                    return false;
                }

                let Ok(digits) = std::str::from_utf8(&bytes[begin..cursor]) else {
                    return false;
                };

                if !u32::from_str_radix(digits, 16).is_ok_and(|value| value < 0x0011_0000) {
                    return false;
                }

                cursor += 1;
            }

            b'0'..=b'9' => {
                let mut value = u16::from(escape - b'0');

                for _ in 0..2 {
                    let Some(&digit) = bytes.get(cursor).filter(|digit| digit.is_ascii_digit())
                    else {
                        break;
                    };

                    value = value * 10 + u16::from(digit - b'0');
                    cursor += 1;
                }

                if value > 255 {
                    return false;
                }
            }

            _ => {}
        }
    }

    true
}
