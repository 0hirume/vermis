use super::{Parsed, Parser};
use crate::{Diagnostic, Kind, Lexer, Span, Token, TokenKind};
use bstr::ByteSlice;

struct Markup<'parser, 'source, const MARKUP: bool> {
    parser: &'parser mut Parser<'source, MARKUP>,
    cursor: usize,
}

impl<const MARKUP: bool> Parser<'_, MARKUP> {
    pub(super) fn markup(&mut self) -> Parsed {
        let cursor = self.current().span.start;
        let mut lexer = self.lexer.take().expect("markup token stream");
        self.tree.tokens.truncate(self.cursor);

        let mut markup = Markup {
            parser: self,
            cursor,
        };

        let result = markup.element();
        let cursor = markup.cursor;
        lexer.resume(cursor);
        self.lexer = Some(lexer);
        self.cursor = self.tree.tokens.len();
        self.end = cursor;
        self.skip_trivia();

        result
    }
}

impl<const MARKUP: bool> Markup<'_, '_, MARKUP> {
    fn source(&self) -> &[u8] {
        self.parser.tree.source.as_bytes()
    }

    fn at(&self, bytes: &[u8]) -> bool {
        self.source()[self.cursor..].starts_with(bytes)
    }

    fn byte(&self) -> Option<u8> {
        self.source().get(self.cursor).copied()
    }

    fn error(&self, message: &'static str) -> Diagnostic {
        Diagnostic {
            span: Span {
                start: self.cursor,
                end: (self.cursor + 1).min(self.source().len()),
            },
            message,
        }
    }

    fn token(&mut self, kind: TokenKind, start: usize) {
        self.parser.tree.tokens.push(Token {
            kind,
            span: Span {
                start,
                end: self.cursor,
            },
        });

        self.parser.cursor = self.parser.tree.tokens.len() - 1;
        self.parser.end = self.cursor;
    }

    fn punctuation(&mut self, bytes: &[u8], message: &'static str) -> Result<(), Diagnostic> {
        if !self.at(bytes) {
            return Err(self.error(message));
        }

        for byte in bytes {
            let start = self.cursor;
            self.cursor += 1;
            self.token(TokenKind::Byte(*byte), start);
        }

        Ok(())
    }

    fn whitespace(&mut self) {
        let start = self.cursor;

        while self.byte().is_some_and(|byte| byte.is_ascii_whitespace()) {
            self.cursor += 1;
        }

        if self.cursor != start {
            self.token(TokenKind::Whitespace, start);
        }
    }

    fn name(&mut self) -> Parsed {
        let start = self.cursor;

        if !self
            .byte()
            .is_some_and(|byte| byte.is_ascii_alphabetic() || byte == b'_')
        {
            return Err(self.error("expected markup name"));
        }

        self.cursor += 1;

        while self
            .byte()
            .is_some_and(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
        {
            self.cursor += 1;
        }

        self.token(TokenKind::Name, start);

        Ok(self.parser.node(Kind::Name, start, []))
    }

    fn qualified(&mut self) -> Parsed {
        let start = self.cursor;
        let mut names = vec![self.name()?];

        while self.at(b".") {
            self.punctuation(b".", "expected member separator")?;
            names.push(self.name()?);
        }

        Ok(self.parser.node(Kind::MarkupName, start, names))
    }

    fn element(&mut self) -> Parsed {
        let start = self.cursor;
        self.punctuation(b"<", "expected opening tag")?;
        self.whitespace();

        let name = if self.at(b">") {
            None
        } else {
            Some(self.qualified()?)
        };

        let attributes = if name.is_some() {
            Some(self.attributes()?)
        } else {
            None
        };

        let closed = self.at(b"/>");

        self.punctuation(
            if closed { b"/>" } else { b">" },
            "expected end of opening tag",
        )?;

        let opening = self
            .parser
            .node(Kind::Opening, start, name.into_iter().chain(attributes));

        let kind = if name.is_some() {
            Kind::Element
        } else {
            Kind::Fragment
        };

        if closed {
            return Ok(self.parser.node(kind, start, [opening]));
        }

        let children = self.children()?;
        let closing_start = self.cursor;
        self.punctuation(b"</", "expected closing tag")?;
        self.whitespace();

        let closing_name = if self.at(b">") {
            None
        } else {
            Some(self.qualified()?)
        };

        self.whitespace();
        self.punctuation(b">", "expected end of closing tag")?;
        let closing = self.parser.node(Kind::Closing, closing_start, closing_name);

        if name.map(|index| self.parser.tree.text(index))
            != closing_name.map(|index| self.parser.tree.text(index))
        {
            self.parser.tree.diagnostics.push(Diagnostic {
                span: self.parser.tree.nodes[closing].span,
                message: "closing tag does not match opening tag",
            });
        }

        Ok(self.parser.node(kind, start, [opening, children, closing]))
    }

    fn attributes(&mut self) -> Parsed {
        let start = self.cursor;
        let mut attributes = Vec::new();

        loop {
            self.whitespace();

            if self.at(b">") || self.at(b"/>") || self.byte().is_none() {
                break;
            }

            let begin = self.cursor;

            if self.at(b"{") {
                let expression = self.hole(true);
                attributes.push(self.parser.node(Kind::MarkupSpread, begin, [expression]));
            } else if self.at(b"=") {
                self.punctuation(b"=", "expected inferred attribute")?;
                self.whitespace();

                if !self.at(b"{") {
                    return Err(self.error("expected expression hole after inferred attribute"));
                }

                let expression = self.hole(true);
                attributes.push(self.parser.node(Kind::MarkupInferred, begin, [expression]));
            } else {
                let name = self.name()?;
                let after_name = self.cursor;
                self.whitespace();

                let value = if self.at(b"=") {
                    self.punctuation(b"=", "expected attribute value")?;
                    self.whitespace();

                    if self.source()[after_name..self.cursor]
                        .first()
                        .is_some_and(u8::is_ascii_whitespace)
                        && self.at(b"{")
                    {
                        self.parser
                            .tree
                            .diagnostics
                            .push(self.error("ambiguous whitespace before inferred attribute"));
                    }

                    Some(if self.at(b"{") {
                        self.hole(true)
                    } else {
                        self.string()?
                    })
                } else {
                    self.parser.end = after_name;

                    None
                };

                attributes.push(self.parser.node(
                    Kind::MarkupAttribute,
                    begin,
                    [name].into_iter().chain(value),
                ));
            }
        }

        Ok(self.parser.node(Kind::MarkupAttributes, start, attributes))
    }

    fn string(&mut self) -> Parsed {
        let start = self.cursor;

        let Some(quote @ (b'\'' | b'"')) = self.byte() else {
            return Err(self.error("expected quoted string or expression hole"));
        };

        self.cursor += 1;

        loop {
            match self.byte() {
                None | Some(b'\n') => {
                    self.token(TokenKind::Error(crate::LexError::BrokenString), start);

                    return Err(self.error("unterminated markup attribute string"));
                }

                Some(byte) if byte == quote => {
                    self.cursor += 1;
                    self.token(TokenKind::QuotedString, start);

                    return Ok(self.parser.node(Kind::String, start, []));
                }

                Some(b'\\') => self.cursor = (self.cursor + 2).min(self.source().len()),
                Some(_) => self.cursor += 1,
            }
        }
    }

    fn hole(&mut self, value_required: bool) -> usize {
        let start = self.cursor;

        self.punctuation(b"{", "expected expression hole")
            .expect("hole starts at brace");

        let content = self.cursor;
        let tokens = self.parser.tree.tokens.len();
        let nodes = self.parser.tree.nodes.len();
        let children = self.parser.tree.children.len();
        self.parser.lexer = Some(Lexer::at(self.parser.tree.source, content));
        self.parser.cursor = tokens;
        self.parser.skip_trivia();

        let comment = self.parser.tree.tokens[tokens..self.parser.cursor]
            .iter()
            .any(|token| matches!(token.kind, TokenKind::Comment | TokenKind::BlockComment));

        let expression = if self.parser.byte(b'}') && comment && !value_required {
            None
        } else {
            let result = if self.parser.byte(b'}') {
                Err(self.parser.error(if comment {
                    "attribute hole requires a value"
                } else {
                    "empty expression hole"
                }))
            } else {
                self.parser.expression(0)
            };

            let result = result.and_then(|expression| {
                if self.parser.byte(b'}') {
                    Ok(expression)
                } else {
                    Err(self.parser.error("expected closing expression hole"))
                }
            });

            Some(match result {
                Ok(expression) => expression,

                Err(error) => {
                    self.parser.tree.nodes.truncate(nodes);
                    self.parser.tree.children.truncate(children);
                    self.parser.tree.diagnostics.push(error);

                    while !self.parser.byte(b'}') && !self.parser.at(TokenKind::Eof) {
                        self.parser.take();
                    }

                    self.parser.end = self.parser.current().span.start;

                    self.parser.node(Kind::Error, content, [])
                }
            })
        };

        self.cursor = self.parser.current().span.end;

        if self.parser.at(TokenKind::Eof) {
            self.parser.tree.tokens.truncate(self.parser.cursor);
        }

        self.parser.lexer = None;
        self.parser.cursor = self.parser.tree.tokens.len() - 1;
        self.parser.end = self.cursor;

        self.parser.node(
            if expression.is_some() {
                Kind::MarkupExpression
            } else {
                Kind::MarkupComment
            },
            start,
            expression,
        )
    }

    fn children(&mut self) -> Parsed {
        let start = self.cursor;
        let mut children = Vec::new();

        while !self.at(b"</") {
            let begin = self.cursor;

            let child = if self.byte().is_none() {
                return Err(self.error("unclosed markup element"));
            } else if self.at(b"<!--") {
                self.cursor += 4;

                while self.byte().is_some() && !self.at(b"-->") {
                    self.cursor += 1;
                }

                let closed = self.at(b"-->");

                if closed {
                    self.cursor += 3;
                }

                self.token(TokenKind::MarkupComment, begin);

                if !closed {
                    return Err(self.error("unterminated markup comment"));
                }

                self.parser.node(Kind::MarkupComment, begin, [])
            } else if self.at(b"<") {
                let mut cursor = self.cursor;

                let result = self.parser.nested(|parser| {
                    let mut markup = Markup { parser, cursor };
                    let result = markup.element();
                    cursor = markup.cursor;

                    result
                });

                self.cursor = cursor;

                result?
            } else if self.at(b"{") {
                self.hole(false)
            } else {
                while self.byte().is_some() && !self.at(b"<") && !self.at(b"{") {
                    self.cursor += if self.at(b"\\") && self.cursor + 1 < self.source().len() {
                        2
                    } else {
                        1
                    };
                }

                self.token(TokenKind::MarkupText, begin);

                self.parser.node(Kind::MarkupText, begin, [])
            };

            children.push(child);
        }

        Ok(self.parser.node(Kind::MarkupChildren, start, children))
    }
}
