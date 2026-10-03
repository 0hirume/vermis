use super::Parser;
use crate::token::{Keyword, Symbol, TokenKind};
use crate::tree::{Diagnostic, ListEntry, NodeIndex, NodeKind, TokenIndex};

impl Parser<'_> {
    /// Parses a type annotation.
    pub fn annotation(&mut self) -> NodeIndex {
        self.annotation_context(false, false)
    }

    /// Parses a declaration annotation, allowing function type attributes.
    pub fn declaration_annotation(&mut self) -> NodeIndex {
        self.annotation_context(false, true)
    }

    /// Parses a type argument, including type packs.
    pub fn type_argument(&mut self) -> NodeIndex {
        self.annotation_context(true, false)
    }

    fn annotation_context(&mut self, allow_pack: bool, declaration: bool) -> NodeIndex {
        self.required("annotation", |parser| {
            parser.nested(|parser| {
                if allow_pack && parser.at_pack() {
                    Ok(parser.pack())
                } else {
                    Ok(parser.composite(allow_pack, declaration))
                }
            })
        })
    }

    fn at_pack(&self) -> bool {
        self.at(TokenKind::Symbol(Symbol::Ellipsis))
            || (self.at(TokenKind::Name) && self.lookahead() == TokenKind::Symbol(Symbol::Ellipsis))
    }

    pub(super) fn pack(&mut self) -> NodeIndex {
        let start = self.position();

        if let Some(ellipsis) = self.consume(TokenKind::Symbol(Symbol::Ellipsis)) {
            let annotation = self.annotation();

            self.append_node(
                start,
                NodeKind::VariadicType {
                    ellipsis,
                    annotation,
                },
            )
        } else {
            let name = self.name();

            let ellipsis =
                self.expect(TokenKind::Symbol(Symbol::Ellipsis), "expected generic pack");

            self.append_node(start, NodeKind::GenericPack { name, ellipsis })
        }
    }

    fn composite(&mut self, allow_pack: bool, declaration: bool) -> NodeIndex {
        let start = self.position();

        let leading = if matches!(
            self.current().kind,
            TokenKind::Symbol(Symbol::Pipe | Symbol::Ampersand)
        ) {
            Some(self.take())
        } else {
            None
        };

        let left = self.required("annotation", |parser| {
            parser.simple_type(
                allow_pack && leading.is_none(),
                declaration && leading.is_none(),
            )
        });

        if matches!(self.node(left).kind, NodeKind::TypePack { .. }) {
            return left;
        }

        self.type_suffix(start, left, leading)
    }

    fn type_suffix(
        &mut self,
        start: TokenIndex,
        mut left: NodeIndex,
        leading: Option<TokenIndex>,
    ) -> NodeIndex {
        let mut separator = leading.map(|token| self.tokens[token.get()].kind);

        if let Some(operator) = leading {
            let kind = if separator == Some(TokenKind::Symbol(Symbol::Pipe)) {
                NodeKind::TypeUnion {
                    left: None,
                    operator,
                    right: left,
                }
            } else {
                NodeKind::TypeIntersection {
                    left: None,
                    operator,
                    right: left,
                }
            };

            left = self.append_node(start, kind);
        }

        loop {
            match self.current().kind {
                TokenKind::Symbol(Symbol::QuestionMark) => {
                    if separator == Some(TokenKind::Symbol(Symbol::Ampersand)) {
                        self.diagnose(self.error("optional intersection requires parentheses"));
                    }

                    separator = Some(TokenKind::Symbol(Symbol::Pipe));
                    let question_mark = self.take();

                    left = self.append_node(
                        start,
                        NodeKind::TypeOptional {
                            annotation: left,
                            question_mark,
                        },
                    );
                }

                kind @ TokenKind::Symbol(Symbol::Pipe | Symbol::Ampersand) => {
                    if separator.is_some_and(|previous| previous != kind) {
                        self.diagnose(
                            self.error("mixed union and intersection requires parentheses"),
                        );
                    }

                    separator = Some(kind);
                    let operator = self.take();

                    let right = self.required("annotation", |parser| {
                        parser.nested(|parser| parser.simple_type(false, false))
                    });

                    let kind = if kind == TokenKind::Symbol(Symbol::Pipe) {
                        NodeKind::TypeUnion {
                            left: Some(left),
                            operator,
                            right,
                        }
                    } else {
                        NodeKind::TypeIntersection {
                            left: Some(left),
                            operator,
                            right,
                        }
                    };

                    left = self.append_node(start, kind);
                }

                _ => break,
            }
        }

        left
    }

    fn simple_type(
        &mut self,
        allow_pack: bool,
        declaration: bool,
    ) -> Result<NodeIndex, Diagnostic> {
        let start = self.position();

        let node = match self.current().kind {
            TokenKind::Keyword(Keyword::Nil) => {
                let token = self.take();

                self.append_node(start, NodeKind::Nil { token })
            }

            TokenKind::Keyword(Keyword::True | Keyword::False) => {
                let token = self.take();

                self.append_node(start, NodeKind::Boolean { token })
            }

            TokenKind::QuotedString | TokenKind::RawString => self.string(),
            TokenKind::Symbol(Symbol::LeftBrace) => self.type_table(declaration),

            TokenKind::Symbol(Symbol::LeftParenthesis | Symbol::LessThan) => {
                self.function_type(allow_pack, None)
            }

            TokenKind::Attribute | TokenKind::Symbol(Symbol::AttributeOpen) => {
                if !declaration {
                    self.diagnose(self.error("function type attributes require a declaration"));
                }

                let attributes = self.attributes();

                self.function_type(false, Some(attributes))
            }

            TokenKind::Name => self.type_reference(),

            _ => return Err(self.error("expected type")),
        };

        Ok(node)
    }

    fn type_reference(&mut self) -> NodeIndex {
        let start = self.position();

        if self.named(b"typeof") && self.lookahead() != TokenKind::Symbol(Symbol::Dot) {
            let keyword = self.take();

            let opening = self.expect(
                TokenKind::Symbol(Symbol::LeftParenthesis),
                "expected typeof expression",
            );

            let expression = self.expression();

            let closing = self.expect(
                TokenKind::Symbol(Symbol::RightParenthesis),
                "expected closing typeof",
            );

            return self.append_node(
                start,
                NodeKind::TypeOf {
                    keyword,
                    opening,
                    expression,
                    closing,
                },
            );
        }

        let first = self.name();
        let dot = self.consume(TokenKind::Symbol(Symbol::Dot));

        let (namespace, name) = if dot.is_some() {
            (Some(first), self.name())
        } else {
            (None, first)
        };

        let arguments = if self.at(TokenKind::Symbol(Symbol::LessThan)) {
            Some(self.type_arguments())
        } else {
            None
        };

        self.append_node(
            start,
            NodeKind::TypeName {
                namespace,
                dot,
                name,
                arguments,
            },
        )
    }

    pub(super) fn type_arguments(&mut self) -> NodeIndex {
        let start = self.position();
        let opening = self.take();
        let mut arguments = Vec::new();

        if !self.at(TokenKind::Symbol(Symbol::GreaterThan)) {
            loop {
                let node = self.type_argument();
                let separator = self.consume(TokenKind::Symbol(Symbol::Comma));
                arguments.push(ListEntry { node, separator });

                if separator.is_none() {
                    break;
                }
            }
        }

        let closing = self.expect(
            TokenKind::Symbol(Symbol::GreaterThan),
            "expected closing type arguments",
        );

        let arguments = self.append_list(arguments);

        self.append_node(
            start,
            NodeKind::TypeArguments {
                opening,
                arguments,
                closing,
            },
        )
    }

    pub(super) fn instantiation_arguments(&mut self) -> NodeIndex {
        let start = self.position();
        let opening = self.take();
        let arguments = self.type_arguments();

        let closing = self.expect(
            TokenKind::Symbol(Symbol::GreaterThan),
            "expected closing instantiation",
        );

        self.append_node(
            start,
            NodeKind::InstantiationArguments {
                opening,
                arguments,
                closing,
            },
        )
    }

    /// Parses generic parameters, permitting defaults when requested.
    pub fn generics(&mut self, defaults: bool) -> NodeIndex {
        let start = self.position();

        let Some(opening) = self.consume(TokenKind::Symbol(Symbol::LessThan)) else {
            return self.missing("generics", self.error("expected generics"));
        };

        let mut parameters = Vec::new();
        let mut packs = false;
        let mut defaulted = false;

        loop {
            let begin = self.position();
            let name = self.name();
            let ellipsis = self.consume(TokenKind::Symbol(Symbol::Ellipsis));

            if packs && ellipsis.is_none() {
                self.diagnose(self.error("type parameters must precede packs"));
            }

            packs |= ellipsis.is_some();
            let assignment = self.consume(TokenKind::Symbol(Symbol::Assignment));

            let default = if assignment.is_some() {
                if !defaults {
                    self.diagnose(self.error("generic defaults are only allowed in type aliases"));
                }

                defaulted = true;

                let default = if ellipsis.is_some() {
                    self.type_argument()
                } else {
                    self.annotation()
                };

                if ellipsis.is_some()
                    && !matches!(
                        self.node(default).kind,
                        NodeKind::TypePack { .. }
                            | NodeKind::GenericPack { .. }
                            | NodeKind::VariadicType { .. }
                            | NodeKind::Missing { .. }
                    )
                {
                    self.diagnose(self.error("expected type pack default"));
                }

                Some(default)
            } else if defaulted {
                Some(self.missing("generic default", self.error("expected generic default")))
            } else {
                None
            };

            let node = self.append_node(
                begin,
                NodeKind::Generic {
                    name,
                    ellipsis,
                    assignment,
                    default,
                },
            );

            let separator = self.consume(TokenKind::Symbol(Symbol::Comma));
            parameters.push(ListEntry { node, separator });

            if separator.is_none() {
                break;
            }
        }

        let closing = self.expect(
            TokenKind::Symbol(Symbol::GreaterThan),
            "expected closing generics",
        );

        let parameters = self.append_list(parameters);

        self.append_node(
            start,
            NodeKind::Generics {
                opening,
                parameters,
                closing,
            },
        )
    }

    fn function_type(&mut self, allow_pack: bool, attributes: Option<NodeIndex>) -> NodeIndex {
        let start = attributes.map_or_else(|| self.position(), |node| self.node(node).tokens.start);

        let generics = if self.at(TokenKind::Symbol(Symbol::LessThan)) {
            Some(self.generics(false))
        } else {
            None
        };

        let parameter_start = self.position();

        let opening = self.expect(
            TokenKind::Symbol(Symbol::LeftParenthesis),
            "expected type parameters",
        );

        let (entries, named) = self.type_parameters();

        let closing = self.expect(
            TokenKind::Symbol(Symbol::RightParenthesis),
            "expected closing type parameters",
        );

        let single = entries.len() == 1
            && !matches!(
                self.node(entries[0].node).kind,
                NodeKind::GenericPack { .. } | NodeKind::VariadicType { .. }
            );

        let function = self.at(TokenKind::Symbol(Symbol::Arrow))
            || attributes.is_some()
            || generics.is_some()
            || named
            || opening.is_none()
            || (!allow_pack && !single);

        if function {
            let parameters = self.append_list(entries);

            let parameters = self.append_node(
                parameter_start,
                NodeKind::Parameters {
                    opening,
                    parameters,
                    closing,
                },
            );

            let arrow = self.expect(
                TokenKind::Symbol(Symbol::Arrow),
                "expected function type arrow",
            );

            let returns = if arrow.is_some() {
                self.type_argument()
            } else {
                self.missing("annotation", self.error("expected function return type"))
            };

            return self.append_node(
                start,
                NodeKind::TypeFunction {
                    attributes,
                    generics,
                    parameters,
                    arrow,
                    returns,
                },
            );
        }

        if allow_pack
            && !(single
                && matches!(
                    self.current().kind,
                    TokenKind::Symbol(Symbol::QuestionMark | Symbol::Pipe | Symbol::Ampersand)
                ))
        {
            let types = self.append_list(entries);

            self.append_node(
                start,
                NodeKind::TypePack {
                    opening,
                    types,
                    closing,
                },
            )
        } else {
            self.append_node(
                start,
                NodeKind::TypeGroup {
                    opening: opening.expect("group has an opening parenthesis"),
                    annotation: entries[0].node,
                    closing,
                },
            )
        }
    }

    fn type_parameters(&mut self) -> (Vec<ListEntry>, bool) {
        let mut entries = Vec::new();
        let mut named = false;

        if !self.at(TokenKind::Symbol(Symbol::RightParenthesis)) {
            loop {
                let begin = self.position();
                let pack = self.at_pack();

                let node = if pack {
                    self.pack()
                } else if self.at(TokenKind::Name)
                    && self.lookahead() == TokenKind::Symbol(Symbol::Colon)
                {
                    named = true;
                    let name = self.name();
                    let colon = self.take();
                    let annotation = self.annotation();

                    self.append_node(
                        begin,
                        NodeKind::TypeParameter {
                            name,
                            colon,
                            annotation,
                        },
                    )
                } else {
                    self.annotation()
                };

                let separator = if pack {
                    None
                } else {
                    self.consume(TokenKind::Symbol(Symbol::Comma))
                };

                entries.push(ListEntry { node, separator });

                if separator.is_none() {
                    break;
                }
            }
        }

        (entries, named)
    }

    fn access(&mut self) -> Option<TokenIndex> {
        if (self.named(b"read") || self.named(b"write"))
            && self.lookahead() != TokenKind::Symbol(Symbol::Colon)
        {
            Some(self.take())
        } else {
            None
        }
    }

    fn type_table(&mut self, declaration: bool) -> NodeIndex {
        let start = self.position();
        let opening = self.take();
        let mut fields = Vec::new();
        let mut table_access = None;
        let mut element = None;
        let mut indexed = false;

        while !self.at(TokenKind::Symbol(Symbol::RightBrace)) && !self.at(TokenKind::EndOfFile) {
            let begin = self.position();
            let access = self.access();

            if fields.is_empty()
                && !self.at(TokenKind::Symbol(Symbol::LeftBracket))
                && !(self.at(TokenKind::Name)
                    && self.lookahead() == TokenKind::Symbol(Symbol::Colon))
            {
                table_access = access;
                element = Some(self.annotation());
                break;
            }

            let node = self.type_field_contents(begin, access, declaration);

            if matches!(self.node(node).kind, NodeKind::TypeIndexer { .. }) {
                if indexed {
                    self.diagnose(Diagnostic {
                        span: self.node(node).span,
                        message: "table type has more than one indexer",
                    });
                }

                indexed = true;
            }

            let separator = self
                .consume(TokenKind::Symbol(Symbol::Comma))
                .or_else(|| self.consume(TokenKind::Symbol(Symbol::Semicolon)));

            fields.push(ListEntry { node, separator });

            if separator.is_none() || self.position() == begin {
                break;
            }
        }

        let closing = self.expect(
            TokenKind::Symbol(Symbol::RightBrace),
            "expected closing table type",
        );

        let fields = self.append_list(fields);

        self.append_node(
            start,
            NodeKind::TypeTable {
                opening,
                access: table_access,
                element,
                fields,
                closing,
            },
        )
    }

    /// Parses a table type field, optionally in a declaration context.
    pub fn type_field(&mut self, declaration: bool) -> NodeIndex {
        let start = self.position();
        let access = self.access();

        self.type_field_contents(start, access, declaration)
    }

    fn type_field_contents(
        &mut self,
        start: TokenIndex,
        access: Option<TokenIndex>,
        declaration: bool,
    ) -> NodeIndex {
        let opening = self.consume(TokenKind::Symbol(Symbol::LeftBracket));

        let property = opening.is_some()
            && matches!(
                self.current().kind,
                TokenKind::QuotedString | TokenKind::RawString
            )
            && self.lookahead() == TokenKind::Symbol(Symbol::RightBracket);

        let key = if opening.is_some() {
            self.annotation()
        } else {
            self.name()
        };

        let closing = if opening.is_some() {
            self.expect(
                TokenKind::Symbol(Symbol::RightBracket),
                "expected closing type index",
            )
        } else {
            None
        };

        let colon = self.expect(TokenKind::Symbol(Symbol::Colon), "expected field type");

        let annotation = if declaration && opening.is_none() {
            self.declaration_annotation()
        } else {
            self.annotation()
        };

        let kind = match opening {
            Some(opening) if !property => NodeKind::TypeIndexer {
                access,
                opening,
                key,
                closing,
                colon,
                annotation,
            },

            _ => NodeKind::TypeField {
                access,
                opening,
                key,
                closing,
                colon,
                annotation,
            },
        };

        self.append_node(start, kind)
    }
}
