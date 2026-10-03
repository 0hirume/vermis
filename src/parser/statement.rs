use super::Parser;
use crate::token::{Keyword, Symbol, TokenKind};
use crate::tree::{Diagnostic, ListEntry, NodeIndex, NodeKind, NodeList, TokenIndex};

impl Parser<'_> {
    pub(super) fn statement(&mut self) -> Result<NodeIndex, Diagnostic> {
        self.nested(Self::statement_contents)
    }

    fn statement_contents(&mut self) -> Result<NodeIndex, Diagnostic> {
        match self.current().kind {
            TokenKind::Keyword(Keyword::Local) => Ok(self.local(false, None)),

            TokenKind::Keyword(Keyword::Function) => {
                let start = self.position();
                let keyword = Some(self.take());
                let name = Some(self.function_name());

                Ok(self.function(start, None, None, keyword, name))
            }

            TokenKind::Attribute | TokenKind::Symbol(Symbol::AttributeOpen) => {
                self.attributed_statement()
            }

            TokenKind::Keyword(Keyword::If) => Ok(self.if_statement()),
            TokenKind::Keyword(Keyword::While) => Ok(self.while_statement()),
            TokenKind::Keyword(Keyword::Repeat) => Ok(self.repeat_statement()),
            TokenKind::Keyword(Keyword::Do) => Ok(self.do_statement()),
            TokenKind::Keyword(Keyword::For) => Ok(self.for_statement()),
            TokenKind::Keyword(Keyword::Return) => Ok(self.return_statement()),

            TokenKind::Keyword(Keyword::Break) => {
                let keyword = self.position();

                if self.loop_depth == 0 {
                    self.diagnose(self.error("break statement must be inside a loop"));
                }

                self.take();

                Ok(self.append_node(keyword, NodeKind::Break { keyword }))
            }

            TokenKind::Name => self.contextual_statement(),
            _ => self.assignment_statement(),
        }
    }

    fn attributed_statement(&mut self) -> Result<NodeIndex, Diagnostic> {
        let start = self.position();
        let attributes = Some(self.attributes());

        if self.named(b"export") {
            self.export_statement(attributes)
        } else if self.named(b"declare") {
            self.declaration_statement(attributes)
        } else if self.at(TokenKind::Keyword(Keyword::Local)) {
            if self.lookahead() != TokenKind::Keyword(Keyword::Function) {
                return Err(self.error("expected function after attributes"));
            }

            Ok(self.local(false, attributes))
        } else if self.named(b"const") {
            if self.lookahead() != TokenKind::Keyword(Keyword::Function) {
                return Err(self.error("expected function after attributes"));
            }

            Ok(self.local(true, attributes))
        } else if self.at(TokenKind::Keyword(Keyword::Function)) || self.at(TokenKind::EndOfFile) {
            let keyword = self.expect(
                TokenKind::Keyword(Keyword::Function),
                "expected function declaration after attributes",
            );

            let name = Some(self.function_name());

            Ok(self.function(start, attributes, None, keyword, name))
        } else {
            Err(self.error("expected function declaration after attributes"))
        }
    }

    fn contextual_statement(&mut self) -> Result<NodeIndex, Diagnostic> {
        let next = self.lookahead();

        let declaration = matches!(
            next,
            TokenKind::Name | TokenKind::Keyword(Keyword::Function) | TokenKind::EndOfFile
        );

        if self.named(b"const") && declaration {
            Ok(self.local(true, None))
        } else if self.named(b"type") && declaration {
            Ok(self.alias_statement())
        } else if self.named(b"declare") && declaration {
            self.declaration_statement(None)
        } else if self.named(b"export")
            && matches!(
                next,
                TokenKind::Name
                    | TokenKind::Keyword(Keyword::Local | Keyword::Function)
                    | TokenKind::EndOfFile
            )
        {
            self.export_statement(None)
        } else if self.named(b"class") && matches!(next, TokenKind::Name | TokenKind::EndOfFile) {
            Ok(self.class_statement(false, None))
        } else if self.named(b"open") && matches!(next, TokenKind::Name | TokenKind::EndOfFile) {
            let open = Some(self.take());

            if !self.named(b"class") {
                return Err(self.error("expected class after open"));
            }

            Ok(self.class_statement(false, open))
        } else if self.named(b"continue") && !continues_expression(next) {
            let keyword = self.position();

            if self.loop_depth == 0 {
                self.diagnose(self.error("continue statement must be inside a loop"));
            }

            self.take();

            Ok(self.append_node(keyword, NodeKind::Continue { keyword }))
        } else {
            self.assignment_statement()
        }
    }

    fn local(&mut self, constant: bool, attributes: Option<NodeIndex>) -> NodeIndex {
        let start = attributes.map_or_else(|| self.position(), |node| self.node(node).tokens.start);
        let keyword = self.take();

        if self.at(TokenKind::Keyword(Keyword::Function)) {
            let function_keyword = Some(self.take());
            let name = Some(self.name());

            return self.function(start, attributes, Some(keyword), function_keyword, name);
        }

        if attributes.is_some() {
            self.diagnose(self.error("expected function after attributes"));
        }

        let bindings = self.bindings();
        let assignment = self.consume(TokenKind::Symbol(Symbol::Assignment));

        let values = if assignment.is_some() {
            self.expressions()
        } else if constant {
            let node = self.missing("initializer", self.error("expected constant initializer"));

            self.append_list([ListEntry {
                node,
                separator: None,
            }])
        } else {
            self.append_list([])
        };

        if constant && assignment.is_some() {
            let entries = &self.lists[values.0.clone()];

            let expandable = entries.last().is_some_and(|entry| {
                matches!(
                    self.node(entry.node).kind,
                    NodeKind::Call { .. } | NodeKind::MethodCall { .. } | NodeKind::Variadic { .. }
                )
            });

            if entries.len() < bindings.0.len() && !expandable {
                self.diagnose(self.error("not enough constant initializers"));
            } else if entries.len() > bindings.0.len() && !expandable {
                self.diagnose(self.error("too many constant initializers"));
            }
        }

        let kind = if constant {
            NodeKind::Constant {
                keyword,
                bindings,
                assignment,
                values,
            }
        } else {
            NodeKind::Local {
                keyword,
                bindings,
                assignment,
                values,
            }
        };

        self.append_node(start, kind)
    }

    fn binding(&mut self, declaration: bool) -> NodeIndex {
        let start = self.position();
        let name = self.name();
        let colon = self.consume(TokenKind::Symbol(Symbol::Colon));

        let annotation = colon.map(|_| {
            if declaration {
                self.declaration_annotation()
            } else {
                self.annotation()
            }
        });

        self.append_node(
            start,
            NodeKind::Binding {
                name,
                colon,
                annotation,
            },
        )
    }

    fn bindings(&mut self) -> NodeList {
        let mut bindings = Vec::new();

        loop {
            let node = self.binding(false);
            let separator = self.consume(TokenKind::Symbol(Symbol::Comma));
            bindings.push(ListEntry { node, separator });

            if separator.is_none() {
                break;
            }
        }

        self.append_list(bindings)
    }

    pub(super) fn condition(&mut self) -> NodeIndex {
        self.required("condition", |parser| {
            parser.nested(|parser| Ok(parser.condition_contents()))
        })
    }

    fn condition_contents(&mut self) -> NodeIndex {
        let constant = self.named(b"const") && self.lookahead() == TokenKind::Name;

        if !constant && !self.at(TokenKind::Keyword(Keyword::Local)) {
            return self.expression();
        }

        let start = self.position();
        let keyword = self.take();
        let node = self.binding(false);

        let bindings = self.append_list([ListEntry {
            node,
            separator: None,
        }]);

        let assignment = self.expect(
            TokenKind::Symbol(Symbol::Assignment),
            "expected condition initializer; only one binding is allowed",
        );

        let node = self.expression();

        let values = self.append_list([ListEntry {
            node,
            separator: None,
        }]);

        let kind = if constant {
            NodeKind::Constant {
                keyword,
                bindings,
                assignment,
                values,
            }
        } else {
            NodeKind::Local {
                keyword,
                bindings,
                assignment,
                values,
            }
        };

        self.append_node(start, kind)
    }

    fn assignment_statement(&mut self) -> Result<NodeIndex, Diagnostic> {
        let start = self.position();
        let first = self.primary()?;

        if matches!(
            self.node(first).kind,
            NodeKind::Call { .. } | NodeKind::MethodCall { .. }
        ) && !self.at(TokenKind::Symbol(Symbol::Comma))
            && !self.at(TokenKind::Symbol(Symbol::Assignment))
            && !compound(self.current().kind)
        {
            return Ok(self.append_node(start, NodeKind::CallStatement { call: first }));
        }

        let mut targets = Vec::new();
        let mut node = first;

        loop {
            let separator = self.consume(TokenKind::Symbol(Symbol::Comma));
            targets.push(ListEntry { node, separator });

            if separator.is_none() {
                break;
            }

            node = self.required("assignment target", Self::primary);
        }

        if !self.at(TokenKind::Symbol(Symbol::Assignment)) && !compound(self.current().kind) {
            return Err(self.error("expected assignment or call"));
        }

        for entry in &targets {
            if !matches!(
                self.node(entry.node).kind,
                NodeKind::Name { .. }
                    | NodeKind::Field { .. }
                    | NodeKind::Index { .. }
                    | NodeKind::Missing { .. }
            ) {
                self.diagnose(Diagnostic {
                    span: self.node(entry.node).span,
                    message: "invalid assignment target",
                });
            }
        }

        if compound(self.current().kind) {
            if targets.len() != 1 {
                return Err(self.error("compound assignment requires one target"));
            }

            let operator = self.take();
            let value = self.expression();

            Ok(self.append_node(
                start,
                NodeKind::CompoundAssignment {
                    target: first,
                    operator,
                    value,
                },
            ))
        } else {
            let assignment = Some(self.take());
            let targets = self.append_list(targets);
            let values = self.expressions();

            Ok(self.append_node(
                start,
                NodeKind::Assignment {
                    targets,
                    assignment,
                    values,
                },
            ))
        }
    }

    fn if_statement(&mut self) -> NodeIndex {
        let start = self.position();
        let mut branches = Vec::new();

        loop {
            let begin = self.position();
            let keyword = self.take();
            let condition = self.condition();
            let then = self.expect(TokenKind::Keyword(Keyword::Then), "expected then");
            let body = self.block(&[Keyword::ElseIf, Keyword::Else, Keyword::End]);

            let node = self.append_node(
                begin,
                NodeKind::Branch {
                    keyword,
                    condition,
                    then,
                    body,
                },
            );

            branches.push(ListEntry {
                node,
                separator: None,
            });

            if !self.at(TokenKind::Keyword(Keyword::ElseIf)) {
                break;
            }
        }

        let otherwise = if self.at(TokenKind::Keyword(Keyword::Else)) {
            let begin = self.position();
            let keyword = self.take();
            let body = self.block(&[Keyword::End]);

            Some(self.append_node(begin, NodeKind::Else { keyword, body }))
        } else {
            None
        };

        let end = self.expect(TokenKind::Keyword(Keyword::End), "expected end");
        let branches = self.append_list(branches);

        self.append_node(
            start,
            NodeKind::If {
                branches,
                otherwise,
                end,
            },
        )
    }

    fn while_statement(&mut self) -> NodeIndex {
        let start = self.position();
        let keyword = self.take();
        let condition = self.expression();
        let do_keyword = self.expect(TokenKind::Keyword(Keyword::Do), "expected do");
        self.loop_depth += 1;
        let body = self.block(&[Keyword::End]);
        self.loop_depth -= 1;
        let end = self.expect(TokenKind::Keyword(Keyword::End), "expected end");

        self.append_node(
            start,
            NodeKind::While {
                keyword,
                condition,
                do_keyword,
                body,
                end,
            },
        )
    }

    fn repeat_statement(&mut self) -> NodeIndex {
        let start = self.position();
        let keyword = self.take();
        self.loop_depth += 1;
        let body = self.block(&[Keyword::Until]);
        self.loop_depth -= 1;
        let until = self.expect(TokenKind::Keyword(Keyword::Until), "expected until");
        let condition = self.expression();

        self.append_node(
            start,
            NodeKind::Repeat {
                keyword,
                body,
                until,
                condition,
            },
        )
    }

    fn do_statement(&mut self) -> NodeIndex {
        let start = self.position();
        let keyword = self.take();
        let body = self.block(&[Keyword::End]);
        let end = self.expect(TokenKind::Keyword(Keyword::End), "expected end");

        self.append_node(start, NodeKind::Do { keyword, body, end })
    }

    fn for_statement(&mut self) -> NodeIndex {
        let start = self.position();
        let keyword = self.take();
        let binding = self.binding(false);

        if self.at(TokenKind::Symbol(Symbol::Assignment)) {
            self.numeric_for(start, keyword, binding)
        } else {
            self.generic_for(start, keyword, binding)
        }
    }

    fn numeric_for(
        &mut self,
        begin: TokenIndex,
        keyword: TokenIndex,
        binding: NodeIndex,
    ) -> NodeIndex {
        let assignment = Some(self.take());
        let start = self.expression();

        let range_separator =
            self.expect(TokenKind::Symbol(Symbol::Comma), "expected range separator");

        let end = self.expression();
        let step_separator = self.consume(TokenKind::Symbol(Symbol::Comma));
        let step = step_separator.map(|_| self.expression());
        let do_keyword = self.expect(TokenKind::Keyword(Keyword::Do), "expected do");
        self.loop_depth += 1;
        let body = self.block(&[Keyword::End]);
        self.loop_depth -= 1;
        let end_keyword = self.expect(TokenKind::Keyword(Keyword::End), "expected end");

        self.append_node(
            begin,
            NodeKind::NumericFor {
                keyword,
                binding,
                assignment,
                start,
                range_separator,
                end,
                step_separator,
                step,
                do_keyword,
                body,
                end_keyword,
            },
        )
    }

    fn generic_for(
        &mut self,
        start: TokenIndex,
        keyword: TokenIndex,
        first: NodeIndex,
    ) -> NodeIndex {
        let mut bindings = Vec::new();
        let mut node = first;

        loop {
            let separator = self.consume(TokenKind::Symbol(Symbol::Comma));
            bindings.push(ListEntry { node, separator });

            if separator.is_none() {
                break;
            }

            node = self.binding(false);
        }

        let bindings = self.append_list(bindings);
        let in_keyword = self.expect(TokenKind::Keyword(Keyword::In), "expected in");
        let values = self.expressions();
        let do_keyword = self.expect(TokenKind::Keyword(Keyword::Do), "expected do");
        self.loop_depth += 1;
        let body = self.block(&[Keyword::End]);
        self.loop_depth -= 1;
        let end = self.expect(TokenKind::Keyword(Keyword::End), "expected end");

        self.append_node(
            start,
            NodeKind::GenericFor {
                keyword,
                bindings,
                in_keyword,
                values,
                do_keyword,
                body,
                end,
            },
        )
    }

    fn return_statement(&mut self) -> NodeIndex {
        let start = self.position();
        let keyword = self.take();

        let values = if self.block_end() || self.at(TokenKind::Symbol(Symbol::Semicolon)) {
            self.append_list([])
        } else {
            self.expressions()
        };

        self.append_node(start, NodeKind::Return { keyword, values })
    }

    fn function_name(&mut self) -> NodeIndex {
        let start = self.position();
        let mut path = Vec::new();
        let mut node = self.name();

        loop {
            let separator = self.consume(TokenKind::Symbol(Symbol::Dot));
            path.push(ListEntry { node, separator });

            if separator.is_none() {
                break;
            }

            node = self.name();
        }

        let path = self.append_list(path);
        let colon = self.consume(TokenKind::Symbol(Symbol::Colon));
        let method = colon.map(|_| self.name());

        self.append_node(
            start,
            NodeKind::FunctionName {
                path,
                colon,
                method,
            },
        )
    }

    pub(super) fn function_expression(&mut self) -> NodeIndex {
        self.required("function", |parser| {
            parser.nested(|parser| {
                let start = parser.position();

                let attributes = if parser.at_attributes() {
                    Some(parser.attributes())
                } else {
                    None
                };

                let keyword = parser.expect(
                    TokenKind::Keyword(Keyword::Function),
                    "expected function after attributes",
                );

                Ok(parser.function(start, attributes, None, keyword, None))
            })
        })
    }

    fn function(
        &mut self,
        start: TokenIndex,
        attributes: Option<NodeIndex>,
        prefix: Option<TokenIndex>,
        keyword: Option<TokenIndex>,
        name: Option<NodeIndex>,
    ) -> NodeIndex {
        self.required("function", |parser| {
            parser.nested(|parser| {
                let (generics, parameters, returns) = parser.signature(false, false);
                let loop_depth = parser.loop_depth;
                parser.loop_depth = 0;
                let body = Some(parser.block(&[Keyword::End]));
                parser.loop_depth = loop_depth;
                let end = parser.expect(TokenKind::Keyword(Keyword::End), "expected end");

                Ok(parser.append_node(
                    start,
                    NodeKind::Function {
                        attributes,
                        prefix,
                        keyword,
                        name,
                        generics,
                        parameters,
                        returns,
                        body,
                        end,
                    },
                ))
            })
        })
    }

    fn signature(
        &mut self,
        declaration: bool,
        method: bool,
    ) -> (Option<NodeIndex>, NodeIndex, Option<NodeIndex>) {
        let generics = if self.at(TokenKind::Symbol(Symbol::LessThan)) {
            if declaration && method {
                self.diagnose(self.error("extern methods cannot have generic parameters"));
            }

            Some(self.generics(false))
        } else {
            None
        };

        let parameters = self.parameters(declaration, method);

        let returns = if matches!(
            self.current().kind,
            TokenKind::Symbol(Symbol::Colon | Symbol::Arrow)
        ) {
            let start = self.position();

            if self.at(TokenKind::Symbol(Symbol::Arrow)) {
                self.diagnose(self.error("function return annotations require a colon"));
            }

            let colon = self.take();
            let annotation = self.type_argument();

            Some(self.append_node(start, NodeKind::Returns { colon, annotation }))
        } else {
            None
        };

        (generics, parameters, returns)
    }

    fn parameters(&mut self, declaration: bool, method: bool) -> NodeIndex {
        self.required("parameters", |parser| {
            parser.nested(|parser| Ok(parser.parameter_contents(declaration, method)))
        })
    }

    fn parameter_contents(&mut self, declaration: bool, method: bool) -> NodeIndex {
        let start = self.position();

        let opening = self.expect(
            TokenKind::Symbol(Symbol::LeftParenthesis),
            "expected parameters",
        );

        let mut parameters = Vec::new();

        if !self.at(TokenKind::Symbol(Symbol::RightParenthesis)) {
            loop {
                let begin = self.position();
                let variadic = self.at(TokenKind::Symbol(Symbol::Ellipsis));

                let node = if variadic {
                    if method && parameters.is_empty() {
                        self.diagnose(self.error("self must be the first method parameter"));
                    }

                    let ellipsis = self.take();
                    let colon = self.consume(TokenKind::Symbol(Symbol::Colon));

                    let annotation = colon.map(|_| {
                        if self.at(TokenKind::Name)
                            && self.lookahead() == TokenKind::Symbol(Symbol::Ellipsis)
                        {
                            self.pack()
                        } else {
                            self.annotation()
                        }
                    });

                    if declaration && annotation.is_none() {
                        self.diagnose(self.error("declaration parameters must be annotated"));
                    }

                    self.append_node(
                        begin,
                        NodeKind::Variadic {
                            ellipsis,
                            colon,
                            annotation,
                        },
                    )
                } else {
                    let binding = self.binding(false);

                    if let NodeKind::Binding {
                        name, annotation, ..
                    } = self.node(binding).kind
                    {
                        let first_self = method && parameters.is_empty();

                        if first_self {
                            if !matches!(self.node(name).kind, NodeKind::Name { token } if self.tokens[token.get()].bytes(self.source) == b"self")
                                || annotation.is_some()
                            {
                                self.diagnose(Diagnostic {
                                    span: self.node(binding).span,
                                    message: "self must be the unannotated first method parameter",
                                });
                            }
                        } else if declaration && annotation.is_none() {
                            self.diagnose(Diagnostic {
                                span: self.node(binding).span,
                                message: "declaration parameters must be annotated",
                            });
                        }
                    }

                    binding
                };

                let separator = if variadic {
                    None
                } else {
                    self.consume(TokenKind::Symbol(Symbol::Comma))
                };

                parameters.push(ListEntry { node, separator });

                if separator.is_none() {
                    break;
                }
            }
        }

        if method && parameters.is_empty() {
            self.diagnose(self.error("method declaration requires a self parameter"));
        }

        let closing = self.expect(
            TokenKind::Symbol(Symbol::RightParenthesis),
            "expected closing parameters",
        );

        let parameters = self.append_list(parameters);

        self.append_node(
            start,
            NodeKind::Parameters {
                opening,
                parameters,
                closing,
            },
        )
    }

    fn alias_statement(&mut self) -> NodeIndex {
        let start = self.position();
        let keyword = self.take();

        if self.at(TokenKind::Keyword(Keyword::Function)) {
            let function_keyword = Some(self.take());
            let name = Some(self.name());

            return self.function(start, None, Some(keyword), function_keyword, name);
        }

        let name = self.name();

        let generics = if self.at(TokenKind::Symbol(Symbol::LessThan)) {
            Some(self.generics(true))
        } else {
            None
        };

        let assignment = self.expect(
            TokenKind::Symbol(Symbol::Assignment),
            "expected type definition",
        );

        let annotation = self.annotation();

        self.append_node(
            start,
            NodeKind::TypeAlias {
                keyword,
                name,
                generics,
                assignment,
                annotation,
            },
        )
    }

    fn export_statement(&mut self, attributes: Option<NodeIndex>) -> Result<NodeIndex, Diagnostic> {
        let start = attributes.map_or_else(|| self.position(), |node| self.node(node).tokens.start);
        let keyword = self.take();

        let declaration = if self.at(TokenKind::Keyword(Keyword::Function)) {
            let begin = self.position();
            let function_keyword = Some(self.take());
            let name = Some(self.name());

            self.function(begin, None, None, function_keyword, name)
        } else {
            if attributes.is_some() {
                return Err(self.error("expected exported function after attributes"));
            }

            self.statement()?
        };

        if !matches!(
            self.node(declaration).kind,
            NodeKind::Local { .. }
                | NodeKind::Constant { .. }
                | NodeKind::Function { .. }
                | NodeKind::TypeAlias { .. }
                | NodeKind::Class { .. }
                | NodeKind::Missing { .. }
        ) {
            return Err(self.error("expected exportable declaration"));
        }

        if let NodeKind::Function {
            prefix: Some(prefix),
            ..
        } = self.node(declaration).kind
        {
            let token = self.tokens[prefix.get()];

            if token.bytes(self.source) == b"declare" {
                return Err(self.error("declared functions cannot be exported"));
            }

            if token.kind == TokenKind::Keyword(Keyword::Local)
                || token.bytes(self.source) == b"const"
            {
                self.diagnose(Diagnostic {
                    span: token.span,
                    message: "exported functions must not have a local or const prefix",
                });
            }
        }

        Ok(self.append_node(
            start,
            NodeKind::Export {
                attributes,
                keyword,
                declaration,
            },
        ))
    }

    fn declaration_statement(
        &mut self,
        attributes: Option<NodeIndex>,
    ) -> Result<NodeIndex, Diagnostic> {
        let start = attributes.map_or_else(|| self.position(), |node| self.node(node).tokens.start);
        let keyword = self.take();

        let external = if self.named(b"extern") {
            Some(self.take())
        } else {
            None
        };

        let declaration = if external.is_some() {
            if attributes.is_some() {
                return Err(self.error("expected declared function after attributes"));
            }

            if !self.named(b"type") {
                return Err(self.error("expected extern type"));
            }

            self.class_statement(true, None)
        } else if self.at(TokenKind::Keyword(Keyword::Function)) {
            let function_keyword = Some(self.take());
            let name = Some(self.name());

            return Ok(self.declared_function(
                start,
                attributes,
                Some(keyword),
                function_keyword,
                name,
                false,
            ));
        } else {
            if attributes.is_some() {
                return Err(self.error("expected declared function after attributes"));
            }

            let begin = self.position();
            let name = self.name();
            let colon = self.expect(TokenKind::Symbol(Symbol::Colon), "expected declared type");
            let annotation = Some(self.declaration_annotation());

            self.append_node(
                begin,
                NodeKind::Binding {
                    name,
                    colon,
                    annotation,
                },
            )
        };

        Ok(self.append_node(
            start,
            NodeKind::Declaration {
                keyword,
                external,
                declaration,
            },
        ))
    }

    fn declared_function(
        &mut self,
        start: TokenIndex,
        attributes: Option<NodeIndex>,
        prefix: Option<TokenIndex>,
        keyword: Option<TokenIndex>,
        name: Option<NodeIndex>,
        method: bool,
    ) -> NodeIndex {
        let (generics, parameters, returns) = self.signature(true, method);

        self.append_node(
            start,
            NodeKind::Function {
                attributes,
                prefix,
                keyword,
                name,
                generics,
                parameters,
                returns,
                body: None,
                end: None,
            },
        )
    }

    fn class_statement(&mut self, external: bool, open: Option<TokenIndex>) -> NodeIndex {
        let start = open.unwrap_or_else(|| self.position());
        let keyword = Some(self.take());
        let name = self.name();

        let extends = if self.named(b"extends") {
            let begin = self.position();
            let keyword = self.take();

            let superclass = if external {
                self.name()
            } else {
                self.class_reference()
            };

            Some(self.append_node(
                begin,
                NodeKind::Extends {
                    keyword,
                    superclass,
                },
            ))
        } else {
            None
        };

        let with = if external {
            if self.named(b"with") {
                Some(self.take())
            } else {
                self.diagnose(self.error("expected with"));

                None
            }
        } else {
            None
        };

        let mut members = Vec::new();
        let mut indexed = false;

        while !self.block_end() {
            let begin = self.position();
            let nodes = self.nodes.len();
            let lists = self.lists.len();
            let end = self.end;
            let token_end = self.token_end;

            let node = match self.nested(|parser| parser.class_member(external)) {
                Ok(node) => node,

                Err(diagnostic) => {
                    self.nodes.truncate(nodes);
                    self.lists.truncate(lists);
                    self.cursor = begin.get();
                    self.end = end;
                    self.token_end = token_end;
                    let node = self.missing("class member", diagnostic);

                    members.push(ListEntry {
                        node,
                        separator: None,
                    });

                    self.recover(
                        begin,
                        &[
                            TokenKind::Keyword(Keyword::End),
                            TokenKind::Keyword(Keyword::Else),
                            TokenKind::Keyword(Keyword::ElseIf),
                            TokenKind::Keyword(Keyword::Until),
                        ],
                    )
                }
            };

            if matches!(self.node(node).kind, NodeKind::TypeIndexer { .. }) {
                if indexed {
                    self.diagnose(Diagnostic {
                        span: self.node(node).span,
                        message: "extern type has more than one indexer",
                    });
                }

                indexed = true;
            }

            let separator = self.consume(TokenKind::Symbol(Symbol::Semicolon));
            members.push(ListEntry { node, separator });

            if self.position() == begin {
                let token = self.take();
                let node = self.append_node(token, NodeKind::Error);

                members.push(ListEntry {
                    node,
                    separator: None,
                });
            }
        }

        let end = self.expect(TokenKind::Keyword(Keyword::End), "expected end");
        let members = self.append_list(members);

        self.append_node(
            start,
            NodeKind::Class {
                open,
                keyword,
                name,
                extends,
                with,
                members,
                end,
            },
        )
    }

    fn class_member(&mut self, external: bool) -> Result<NodeIndex, Diagnostic> {
        let start = self.position();

        let attributes = if self.at_attributes() {
            if !external {
                return Err(self.error("class method attributes are not allowed"));
            }

            Some(self.attributes())
        } else {
            None
        };

        let public = if !external && self.named(b"public") {
            Some(self.take())
        } else {
            None
        };

        if self.at(TokenKind::Keyword(Keyword::Function)) {
            let keyword = Some(self.take());
            let name = Some(self.name());

            let node = if external {
                self.declared_function(start, attributes, None, keyword, name, true)
            } else {
                self.function(start, None, public, keyword, name)
            };

            if !external {
                self.validate_method_self(node);
            }

            Ok(node)
        } else if attributes.is_some() {
            Err(self.error("expected method after attributes"))
        } else if let Some(public) = public {
            let binding = self.binding(false);

            Ok(self.append_node(start, NodeKind::Property { public, binding }))
        } else if external {
            Ok(self.type_field(false))
        } else {
            Err(self.error("expected class member"))
        }
    }

    fn validate_method_self(&mut self, function: NodeIndex) {
        let NodeKind::Function { parameters, .. } = self.node(function).kind else {
            return;
        };

        let NodeKind::Parameters { ref parameters, .. } = self.node(parameters).kind else {
            return;
        };

        let Some(entry) = self.lists[parameters.0.clone()].first() else {
            return;
        };

        let NodeKind::Binding {
            name,
            annotation: Some(annotation),
            ..
        } = self.node(entry.node).kind
        else {
            return;
        };

        if matches!(self.node(name).kind, NodeKind::Name { token } if self.tokens[token.get()].bytes(self.source) == b"self")
        {
            self.diagnose(Diagnostic {
                span: self.node(annotation).span,
                message: "self parameter cannot have a type annotation",
            });
        }
    }

    fn class_reference(&mut self) -> NodeIndex {
        let start = self.position();
        let mut receiver = self.name();

        loop {
            if let Some(dot) = self.consume(TokenKind::Symbol(Symbol::Dot)) {
                let name = self.name();

                receiver = self.append_node(
                    start,
                    NodeKind::Field {
                        receiver,
                        dot,
                        name,
                    },
                );
            } else if let Some(opening) = self.consume(TokenKind::Symbol(Symbol::LeftBracket)) {
                let key = self.expression();

                let closing = self.expect(
                    TokenKind::Symbol(Symbol::RightBracket),
                    "expected closing superclass index",
                );

                receiver = self.append_node(
                    start,
                    NodeKind::Index {
                        receiver,
                        opening,
                        key,
                        closing,
                    },
                );
            } else {
                break;
            }
        }

        receiver
    }

    fn at_attributes(&self) -> bool {
        matches!(
            self.current().kind,
            TokenKind::Attribute | TokenKind::Symbol(Symbol::AttributeOpen)
        )
    }
}

fn compound(kind: TokenKind) -> bool {
    matches!(
        kind,
        TokenKind::Symbol(
            Symbol::AddAssignment
                | Symbol::SubtractAssignment
                | Symbol::MultiplyAssignment
                | Symbol::DivideAssignment
                | Symbol::FloorDivideAssignment
                | Symbol::ModuloAssignment
                | Symbol::PowerAssignment
                | Symbol::ConcatenateAssignment
        )
    )
}

fn continues_expression(kind: TokenKind) -> bool {
    compound(kind)
        || matches!(
            kind,
            TokenKind::QuotedString
                | TokenKind::RawString
                | TokenKind::Symbol(
                    Symbol::Assignment
                        | Symbol::LeftParenthesis
                        | Symbol::Dot
                        | Symbol::LeftBracket
                        | Symbol::Colon
                        | Symbol::LeftBrace
                        | Symbol::Comma
                        | Symbol::LessThan
                        | Symbol::GreaterThan
                        | Symbol::DoubleColon
                        | Symbol::Add
                        | Symbol::Subtract
                        | Symbol::Multiply
                        | Symbol::Divide
                        | Symbol::FloorDivide
                        | Symbol::Modulo
                        | Symbol::Power
                        | Symbol::Concatenate
                        | Symbol::Equal
                        | Symbol::NotEqual
                        | Symbol::LessThanOrEqual
                        | Symbol::GreaterThanOrEqual
                        | Symbol::Ellipsis
                )
        )
}
