use super::context::{Expectation, Expected, Rule};
use super::{Parsed, Parser};
use crate::lexer::{Checkpoint, Mode, State};
use crate::{Diagnostic, Kind, Lexer, Span, Token, TokenKind};

struct Markup<'parser, 'source, const MARKUP: bool> {
    parser: &'parser mut Parser<'source, MARKUP>,
    cursor: usize,
}

impl<const MARKUP: bool> Parser<'_, MARKUP> {
    pub(super) fn markup(&mut self) -> Parsed {
        let cursor = self.raw_current().span.start;
        let lexical = self.entry_state();
        let raw = matches!(lexical.mode, Mode::MarkupTag | Mode::MarkupChildren);

        if !raw {
            self.enter(Rule::Markup, cursor, lexical.clone());
        }

        let mut lexer = self.lexer.take().expect("markup token stream");
        self.lexical = lexical.clone();
        self.truncate_tokens(self.cursor);

        let mut markup = Markup {
            parser: self,
            cursor,
        };

        let result = markup.element();
        let cursor = markup.cursor;
        lexer.resume(cursor);
        self.lexer = Some(lexer);
        self.end = cursor;

        if raw {
            if result.is_err() {
                self.cursor = self.builder.tokens.len();
                self.skip_trivia();
            } else {
                self.cursor = self.builder.tokens.len().saturating_sub(1);
            }

            return result;
        }

        self.cursor = self.builder.tokens.len();
        self.skip_trivia();

        self.leave(result, self.raw_current(), self.entry_state())
    }
}

impl<const MARKUP: bool> Markup<'_, '_, MARKUP> {
    fn source(&self) -> &[u8] {
        self.parser.builder.source
    }

    fn at(&self, bytes: &[u8]) -> bool {
        if !self.parser.active() {
            return false;
        }

        self.parser.inspect(Span {
            start: self.cursor,
            end: (self.cursor + bytes.len()).min(self.source().len()),
        });

        if self.cursor + bytes.len() > self.source().len() {
            let end = self.source().len();
            self.parser.inspect(Span { start: end, end });
        }

        self.source()[self.cursor..].starts_with(bytes)
    }

    fn byte(&self) -> Option<u8> {
        if !self.parser.active() {
            return None;
        }

        self.parser.inspect(Span {
            start: self.cursor,
            end: (self.cursor + 1).min(self.source().len()),
        });

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

    fn missing(&mut self, error: Diagnostic, role: &'static str) -> usize {
        let diagnostic = self.parser.builder.diagnostics.len();
        self.parser.diagnose(error);
        let node = self.parser.node(Kind::Missing, self.cursor, []);
        self.parser.builder.nodes[node].span.end = self.cursor;

        self.parser.builder.nodes[node].recovery.push(Expectation {
            span: Span {
                start: self.cursor,
                end: self.cursor,
            },
            expected: Expected::Role(role),
        });

        self.parser.claim(node, diagnostic);

        node
    }

    fn token(&mut self, kind: TokenKind, start: usize) {
        if let Some(execution) = &self.parser.execution {
            execution.token();
        }

        self.parser.builder.checkpoints.push(Checkpoint {
            cursor: start,
            state: self.parser.lexical.clone(),
            finished: false,
        });

        self.parser.builder.tokens.push(Token {
            kind,
            span: Span {
                start,
                end: self.cursor,
            },
        });

        self.parser.cursor = self.parser.builder.tokens.len() - 1;
        self.parser.end = self.cursor;
    }

    fn punctuation(&mut self, bytes: &[u8], message: &'static str) -> Result<(), Diagnostic> {
        if !self.at(bytes) {
            for byte in bytes {
                self.parser.expectation(
                    Span {
                        start: self.cursor,
                        end: self.cursor,
                    },
                    Expected::Token(TokenKind::Byte(*byte)),
                );
            }

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

    fn name(&mut self) -> usize {
        let start = self.cursor;

        if !self
            .byte()
            .is_some_and(|byte| byte.is_ascii_alphabetic() || byte == b'_')
        {
            return self.missing(self.error("expected markup name"), "markup name");
        }

        self.cursor += 1;

        while self
            .byte()
            .is_some_and(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
        {
            self.cursor += 1;
        }

        self.token(TokenKind::Name, start);

        self.parser.node(Kind::Name, start, [])
    }

    fn qualified(&mut self) -> Parsed {
        let start = self.cursor;
        let mut names = vec![self.name()];

        while self.at(b".") {
            self.punctuation(b".", "expected member separator")?;
            names.push(self.name());
        }

        Ok(self.parser.node(Kind::MarkupName, start, names))
    }

    fn element(&mut self) -> Parsed {
        let lexical = self.parser.lexical.clone();

        self.parser
            .enter(Rule::Markup, self.cursor, lexical.clone());

        self.parser.lexical.mode = Mode::MarkupTag;
        let result = self.element_contents();
        self.parser.lexical = lexical.clone();

        let current = Token {
            kind: self
                .source()
                .get(self.cursor)
                .map_or(TokenKind::Eof, |byte| TokenKind::Byte(*byte)),
            span: Span {
                start: self.cursor,
                end: (self.cursor + 1).min(self.source().len()),
            },
        };

        self.parser.inspect(current.span);

        self.parser.leave(result, current, lexical)
    }

    fn element_contents(&mut self) -> Parsed {
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

        let terminator = self.punctuation(
            if closed { b"/>" } else { b">" },
            "expected end of opening tag",
        );

        let opening = self
            .parser
            .node(Kind::Opening, start, name.into_iter().chain(attributes));

        let kind = if name.is_some() {
            Kind::Element
        } else {
            Kind::Fragment
        };

        if let Err(error) = terminator {
            self.parser.diagnose(error);

            return Ok(self.parser.node(kind, start, [opening]));
        }

        if closed {
            return Ok(self.parser.node(kind, start, [opening]));
        }

        self.parser.lexical.mode = Mode::MarkupChildren;
        let children = self.children();
        let closing_start = self.cursor;

        if let Err(error) = self.punctuation(b"</", "expected closing tag") {
            self.parser.diagnose(error);

            return Ok(self.parser.node(kind, start, [opening, children]));
        }

        self.parser.lexical.mode = Mode::MarkupTag;
        self.whitespace();

        let closing_name = if self.at(b">") {
            None
        } else {
            Some(self.qualified()?)
        };

        self.whitespace();

        if let Err(error) = self.punctuation(b">", "expected end of closing tag") {
            self.parser.diagnose(error);
        }

        let closing = self.parser.node(Kind::Closing, closing_start, closing_name);

        for index in name.into_iter().chain(closing_name) {
            self.parser.inspect(self.parser.builder.nodes[index].span);
        }

        if name.map(|index| self.parser.builder.text(index))
            != closing_name.map(|index| self.parser.builder.text(index))
        {
            self.parser.diagnose(Diagnostic {
                span: self.parser.builder.nodes[closing].span,
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

                let expression = if self.at(b"{") {
                    self.hole(true)
                } else {
                    self.parser.expectation(
                        Span {
                            start: self.cursor,
                            end: self.cursor,
                        },
                        Expected::Token(TokenKind::Byte(b'{')),
                    );

                    self.missing(
                        self.error("expected expression hole after inferred attribute"),
                        "expression hole",
                    )
                };

                attributes.push(self.parser.node(Kind::MarkupInferred, begin, [expression]));
            } else {
                let name = self.name();
                let after_name = self.cursor;
                self.whitespace();

                let value = if self.at(b"=") {
                    self.punctuation(b"=", "expected attribute value")?;
                    self.whitespace();

                    self.parser.inspect(Span {
                        start: after_name,
                        end: (after_name + 1).min(self.source().len()),
                    });

                    if self.source()[after_name..self.cursor]
                        .first()
                        .is_some_and(u8::is_ascii_whitespace)
                        && self.at(b"{")
                    {
                        self.parser
                            .diagnose(self.error("ambiguous whitespace before inferred attribute"));
                    }

                    Some(if self.at(b"{") {
                        self.hole(true)
                    } else {
                        self.string()
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

            if self.cursor == begin {
                break;
            }
        }

        Ok(self.parser.node(Kind::MarkupAttributes, start, attributes))
    }

    fn string(&mut self) -> usize {
        let start = self.cursor;

        let Some(quote @ (b'\'' | b'"')) = self.byte() else {
            return self.missing(
                self.error("expected quoted string or expression hole"),
                "attribute value",
            );
        };

        self.cursor += 1;

        loop {
            match self.byte() {
                None | Some(b'\n') => {
                    self.token(TokenKind::Error(crate::LexError::BrokenString), start);

                    self.parser
                        .diagnose(self.error("unterminated markup attribute string"));

                    return self.parser.node(Kind::String, start, []);
                }

                Some(byte) if byte == quote => {
                    self.cursor += 1;
                    self.token(TokenKind::QuotedString, start);

                    return self.parser.node(Kind::String, start, []);
                }

                Some(b'\\') => {
                    self.parser.inspect(Span {
                        start: self.cursor,
                        end: (self.cursor + 2).min(self.source().len()),
                    });

                    self.cursor = (self.cursor + 2).min(self.source().len());
                }

                Some(_) => self.cursor += 1,
            }
        }
    }

    fn hole(&mut self, value_required: bool) -> usize {
        let start = self.cursor;

        if let Err(error) = self.punctuation(b"{", "expected expression hole") {
            return self.missing(error, "expression hole");
        }

        let content = self.cursor;
        let tokens = self.parser.builder.tokens.len();
        let lexical = self.parser.lexical.clone();

        let state = State {
            braces: crate::lexer::Braces::default(),
            mode: Mode::MarkupHole,
        };

        self.parser.lexical = state.clone();

        self.parser.lexer = Some(Lexer::controlled(
            self.parser.source,
            content,
            &state,
            self.parser.execution.clone(),
        ));

        self.parser.cursor = tokens;
        self.parser.skip_trivia();

        for token in &self.parser.builder.tokens[tokens..self.parser.cursor] {
            self.parser.inspect(token.span);
        }

        let comment = self.parser.builder.tokens[tokens..self.parser.cursor]
            .iter()
            .any(|token| matches!(token.kind, TokenKind::Comment | TokenKind::BlockComment));

        let expression = if self.parser.byte(b'}') && comment && !value_required {
            None
        } else {
            let expression = if self.parser.byte(b'}') {
                self.parser.missing(
                    self.parser.error(if comment {
                        "attribute hole requires a value"
                    } else {
                        "empty expression hole"
                    }),
                    "expression",
                )
            } else {
                self.parser
                    .expression(0)
                    .expect("required expression recovers")
            };

            if !self.parser.byte(b'}') {
                let position = self.parser.current().span.start;

                self.parser.expectation(
                    Span {
                        start: position,
                        end: position,
                    },
                    Expected::Token(TokenKind::Byte(b'}')),
                );

                self.parser
                    .diagnose(self.parser.error("expected closing expression hole"));

                while !self.parser.byte(b'}') && !self.parser.at(TokenKind::Eof) {
                    self.parser.take();
                }
            }

            Some(expression)
        };

        self.cursor = self.parser.current().span.end;

        if self.parser.at(TokenKind::Eof) {
            self.parser.truncate_tokens(self.parser.cursor);
        }

        self.parser.lexer = None;
        self.parser.lexical = lexical;
        self.parser.cursor = self.parser.builder.tokens.len() - 1;
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

    fn children(&mut self) -> usize {
        let start = self.cursor;
        let mut children = Vec::new();

        while !self.at(b"</") {
            let begin = self.cursor;

            let child = if self.byte().is_none() {
                break;
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
                    self.parser
                        .diagnose(self.error("unterminated markup comment"));
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

                match result {
                    Ok(element) => element,

                    Err(error) => {
                        let diagnostic = self.parser.builder.diagnostics.len();
                        self.parser.diagnose(error);

                        if let Err(error) = self.punctuation(b"<", "expected opening tag") {
                            self.parser.diagnose(error);
                        }

                        let recovery = self.parser.node(Kind::Error, begin, []);
                        self.parser.claim(recovery, diagnostic);

                        recovery
                    }
                }
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

        self.parser.node(Kind::MarkupChildren, start, children)
    }
}
