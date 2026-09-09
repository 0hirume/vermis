use super::{Diagnostic, Keyword, Kind, Operator, Parsed, Parser, TokenKind};

impl Parser<'_> {
    pub(super) fn statement(&mut self) -> Parsed {
        let start = self.current().span.start;

        match self.current().kind {
            TokenKind::Attribute | TokenKind::AttributeOpen => {
                let attributes = self.attributes()?;

                if self.named(b"declare") && self.next() == TokenKind::Keyword(Keyword::Function) {
                    let declaration = self.declaration()?;
                    self.tree.nodes[declaration].children.insert(0, attributes);
                    self.tree.nodes[declaration].span.start = start;
                    return Ok(declaration);
                }

                let local = self.consume(TokenKind::Keyword(Keyword::Local));
                self.expect(
                    TokenKind::Keyword(Keyword::Function),
                    "expected function after attributes",
                )?;

                let name = if local {
                    self.name()?
                } else {
                    self.function_name()?
                };

                self.function(
                    start,
                    if local {
                        Kind::LocalFunction
                    } else {
                        Kind::Function
                    },
                    vec![attributes, name],
                )
            }

            TokenKind::Keyword(Keyword::Local) => self.local(false),
            TokenKind::Keyword(Keyword::Function) => {
                self.take();
                let name = self.function_name()?;
                self.function(start, Kind::Function, vec![name])
            }

            TokenKind::Keyword(Keyword::If) => self.conditional(),
            TokenKind::Keyword(Keyword::While) => {
                self.take();
                let condition = self.expression(0)?;
                self.expect(TokenKind::Keyword(Keyword::Do), "expected do")?;
                let body = self.block(&[Keyword::End]);
                self.close(Keyword::End);
                Ok(self.node(Kind::While, start, vec![condition, body]))
            }

            TokenKind::Keyword(Keyword::Repeat) => {
                self.take();
                let body = self.block(&[Keyword::Until]);
                self.expect(TokenKind::Keyword(Keyword::Until), "expected until")?;
                let condition = self.expression(0)?;
                Ok(self.node(Kind::Repeat, start, vec![body, condition]))
            }

            TokenKind::Keyword(Keyword::Do) => {
                self.take();
                let body = self.block(&[Keyword::End]);
                self.close(Keyword::End);
                Ok(self.node(Kind::Do, start, vec![body]))
            }

            TokenKind::Keyword(Keyword::For) => self.for_statement(),
            TokenKind::Keyword(Keyword::Return) => {
                self.take();
                let values = if self.block_end() || self.byte(b';') {
                    Vec::new()
                } else {
                    self.expressions()?
                };
                Ok(self.node(Kind::Return, start, values))
            }

            TokenKind::Keyword(Keyword::Break) => Ok(self.leaf(Kind::Break)),
            TokenKind::Name => self.contextual(),
            _ => self.assignment(),
        }
    }

    fn contextual(&mut self) -> Parsed {
        let start = self.current().span.start;

        match self.current().kind {
            TokenKind::Name if self.named(b"const") && self.next() == TokenKind::Name => {
                self.local(true)
            }

            TokenKind::Name
                if self.named(b"type")
                    && matches!(
                        self.next(),
                        TokenKind::Name | TokenKind::Keyword(Keyword::Function)
                    ) =>
            {
                self.alias()
            }

            TokenKind::Name if self.named(b"class") && self.next() == TokenKind::Name => {
                self.class(false)
            }

            TokenKind::Name if self.named(b"open") && self.next() == TokenKind::Name => {
                self.take();
                if !self.named(b"class") {
                    return Err(self.error("expected class after open"));
                }
                let class = self.class(false)?;
                self.tree.nodes[class].span.start = start;
                Ok(class)
            }

            TokenKind::Name
                if self.named(b"declare")
                    && matches!(
                        self.next(),
                        TokenKind::Name | TokenKind::Keyword(Keyword::Function)
                    ) =>
            {
                self.declaration()
            }

            TokenKind::Name
                if self.named(b"export")
                    && matches!(
                        self.next(),
                        TokenKind::Name | TokenKind::Keyword(Keyword::Local | Keyword::Function)
                    ) =>
            {
                self.take();
                let statement = self.nested(Self::statement)?;

                if !matches!(
                    self.tree.nodes[statement].kind,
                    Kind::Local
                        | Kind::Constant
                        | Kind::Function
                        | Kind::TypeAlias
                        | Kind::TypeFunction
                        | Kind::Class
                ) {
                    return Err(self.error("expected exportable declaration"));
                }

                Ok(self.node(Kind::Export, start, vec![statement]))
            }

            TokenKind::Name
                if self.named(b"continue")
                    && !matches!(
                        self.next(),
                        TokenKind::Byte(b'=' | b'(' | b'.' | b'[' | b':' | b'{' | b',' | b'<')
                            | TokenKind::Operator(_)
                            | TokenKind::QuotedString
                            | TokenKind::RawString
                    ) =>
            {
                Ok(self.leaf(Kind::Continue))
            }
            _ => self.assignment(),
        }
    }

    fn local(&mut self, constant: bool) -> Parsed {
        let start = self.take().span.start;

        if !constant && self.consume(TokenKind::Keyword(Keyword::Function)) {
            let name = self.name()?;
            return self.function(start, Kind::LocalFunction, vec![name]);
        }

        let mut children = vec![self.binding()?];
        while self.consume(TokenKind::Byte(b',')) {
            children.push(self.binding()?);
        }

        if self.consume(TokenKind::Byte(b'=')) {
            children.extend(self.expressions()?);
        } else if constant {
            return Err(self.error("expected constant initializer"));
        }

        Ok(self.node(
            if constant {
                Kind::Constant
            } else {
                Kind::Local
            },
            start,
            children,
        ))
    }

    fn assignment(&mut self) -> Parsed {
        let start = self.current().span.start;
        let first = self.primary()?;
        let mut targets = vec![first];

        while self.consume(TokenKind::Byte(b',')) {
            targets.push(self.primary()?);
        }

        let compound = matches!(
            self.current().kind,
            TokenKind::Operator(
                Operator::AddAssign
                    | Operator::SubtractAssign
                    | Operator::MultiplyAssign
                    | Operator::DivideAssign
                    | Operator::FloorDivideAssign
                    | Operator::ModuloAssign
                    | Operator::PowerAssign
                    | Operator::ConcatAssign
            )
        );

        if self.byte(b'=') || compound {
            if targets.iter().any(|index| {
                !matches!(
                    self.tree.nodes[*index].kind,
                    Kind::Name | Kind::Field | Kind::Index
                )
            }) {
                return Err(self.error("invalid assignment target"));
            }

            if compound && targets.len() != 1 {
                return Err(self.error("compound assignment requires one target"));
            }

            targets.push(self.leaf(Kind::Operator));
            if compound {
                targets.push(self.expression(0)?);
            } else {
                targets.extend(self.expressions()?);
            }

            Ok(self.node(
                if compound {
                    Kind::CompoundAssignment
                } else {
                    Kind::Assignment
                },
                start,
                targets,
            ))
        } else if targets.len() == 1
            && matches!(self.tree.nodes[first].kind, Kind::Call | Kind::MethodCall)
        {
            Ok(self.node(Kind::CallStatement, start, targets))
        } else {
            Err(self.error("expected assignment or call"))
        }
    }

    fn conditional(&mut self) -> Parsed {
        let start = self.current().span.start;
        let mut branches = Vec::new();

        loop {
            let begin = self.take().span.start;
            let condition = if self.keyword(Keyword::Local) || self.named(b"const") {
                let constant = self.named(b"const");
                let begin = self.take().span.start;
                let binding = self.binding()?;
                self.expect(TokenKind::Byte(b'='), "expected condition initializer")?;
                let value = self.expression(0)?;
                self.node(
                    if constant {
                        Kind::Constant
                    } else {
                        Kind::Local
                    },
                    begin,
                    vec![binding, value],
                )
            } else {
                self.expression(0)?
            };

            self.expect(TokenKind::Keyword(Keyword::Then), "expected then")?;
            let body = self.block(&[Keyword::ElseIf, Keyword::Else, Keyword::End]);
            branches.push(self.node(Kind::Branch, begin, vec![condition, body]));

            if !self.keyword(Keyword::ElseIf) {
                break;
            }
        }

        if self.keyword(Keyword::Else) {
            let begin = self.take().span.start;
            let body = self.block(&[Keyword::End]);
            branches.push(self.node(Kind::Else, begin, vec![body]));
        }

        self.close(Keyword::End);

        Ok(self.node(Kind::If, start, branches))
    }

    fn for_statement(&mut self) -> Parsed {
        let start = self.take().span.start;
        let mut children = vec![self.binding()?];
        let numeric = self.consume(TokenKind::Byte(b'='));

        if numeric {
            children.push(self.expression(0)?);
            self.expect(TokenKind::Byte(b','), "expected range separator")?;
            children.push(self.expression(0)?);
            if self.consume(TokenKind::Byte(b',')) {
                children.push(self.expression(0)?);
            }
        } else {
            while self.consume(TokenKind::Byte(b',')) {
                children.push(self.binding()?);
            }
            self.expect(TokenKind::Keyword(Keyword::In), "expected in")?;
            children.extend(self.expressions()?);
        }

        self.expect(TokenKind::Keyword(Keyword::Do), "expected do")?;
        children.push(self.block(&[Keyword::End]));
        self.close(Keyword::End);

        Ok(self.node(
            if numeric {
                Kind::NumericFor
            } else {
                Kind::GenericFor
            },
            start,
            children,
        ))
    }

    fn function_name(&mut self) -> Parsed {
        let start = self.current().span.start;
        let mut children = vec![self.name()?];
        while self.consume(TokenKind::Byte(b'.')) {
            children.push(self.name()?);
        }

        if self.byte(b':') {
            children.push(self.leaf(Kind::Operator));
            children.push(self.name()?);
        }

        Ok(self.node(Kind::FunctionName, start, children))
    }

    pub(super) fn function(&mut self, start: usize, kind: Kind, children: Vec<usize>) -> Parsed {
        let mut children = self.signature(children)?;
        children.push(self.block(&[Keyword::End]));
        self.close(Keyword::End);

        Ok(self.node(kind, start, children))
    }

    fn signature(&mut self, mut children: Vec<usize>) -> Result<Vec<usize>, Diagnostic> {
        if self.byte(b'<') {
            children.push(self.generics(false)?);
        }

        let begin = self.current().span.start;
        self.expect(TokenKind::Byte(b'('), "expected parameters")?;
        let mut parameters = Vec::new();

        if !self.byte(b')') {
            loop {
                if self.at(TokenKind::Operator(Operator::Ellipsis)) {
                    let begin = self.take().span.start;
                    let mut annotation = Vec::new();

                    if self.consume(TokenKind::Byte(b':')) {
                        annotation.push(
                            if self.at(TokenKind::Name)
                                && self.next() == TokenKind::Operator(Operator::Ellipsis)
                            {
                                self.pack()?
                            } else {
                                self.annotation()?
                            },
                        );
                    }

                    parameters.push(self.node(Kind::Variadic, begin, annotation));
                    break;
                }

                parameters.push(self.binding()?);
                if !self.consume(TokenKind::Byte(b',')) {
                    break;
                }
            }
        }

        self.expect(TokenKind::Byte(b')'), "expected closing parameters")?;
        children.push(self.node(Kind::Parameters, begin, parameters));

        if self.consume(TokenKind::Byte(b':')) {
            let begin = self.current().span.start;
            let returns = self.type_argument()?;
            children.push(self.node(Kind::Returns, begin, vec![returns]));
        }

        Ok(children)
    }

    fn alias(&mut self) -> Parsed {
        let start = self.take().span.start;

        if self.consume(TokenKind::Keyword(Keyword::Function)) {
            let name = self.name()?;
            return self.function(start, Kind::TypeFunction, vec![name]);
        }

        let mut children = vec![self.name()?];
        if self.byte(b'<') {
            children.push(self.generics(true)?);
        }

        self.expect(TokenKind::Byte(b'='), "expected type definition")?;
        children.push(self.annotation()?);

        Ok(self.node(Kind::TypeAlias, start, children))
    }

    fn declaration(&mut self) -> Parsed {
        let start = self.take().span.start;

        if self.named(b"extern") {
            self.take();
            if !self.named(b"type") {
                return Err(self.error("expected extern type"));
            }
            let class = self.class(true)?;
            return Ok(self.node(Kind::Declaration, start, vec![class]));
        }

        let function = self.consume(TokenKind::Keyword(Keyword::Function));
        let mut children = vec![self.name()?];

        if function {
            children = self.signature(children)?;
        } else {
            self.expect(TokenKind::Byte(b':'), "expected declared type")?;
            children.push(self.annotation()?);
        }

        Ok(self.node(Kind::Declaration, start, children))
    }

    fn class(&mut self, external: bool) -> Parsed {
        let start = self.take().span.start;
        let mut children = vec![self.name()?];

        if self.named(b"extends") {
            let begin = self.take().span.start;
            let mut path = vec![self.name()?];
            while self.consume(TokenKind::Byte(b'.')) {
                path.push(self.name()?);
            }
            children.push(self.node(Kind::Extends, begin, path));
        }

        if external {
            if !self.named(b"with") {
                return Err(self.error("expected with"));
            }
            self.take();
        }

        while !self.block_end() {
            let begin = self.current().span.start;
            let public = self.named(b"public");

            if public
                || (external
                    && (self.named(b"read") || self.named(b"write"))
                    && self.next() != TokenKind::Byte(b':'))
            {
                self.take();
            }

            if self.consume(TokenKind::Keyword(Keyword::Function)) {
                let name = self.name()?;
                let method = if external {
                    let parts = self.signature(vec![name])?;
                    self.node(Kind::Method, begin, parts)
                } else {
                    self.function(begin, Kind::Method, vec![name])?
                };
                children.push(method);
            } else if public || external {
                children.push(if external {
                    self.type_field()?
                } else {
                    let binding = self.binding()?;
                    self.node(Kind::Property, begin, vec![binding])
                });
            } else {
                return Err(self.error("expected class member"));
            }
        }

        self.close(Keyword::End);

        Ok(self.node(Kind::Class, start, children))
    }

    pub(super) fn block_end(&self) -> bool {
        matches!(
            self.current().kind,
            TokenKind::Eof
                | TokenKind::Keyword(
                    Keyword::End | Keyword::Else | Keyword::ElseIf | Keyword::Until
                )
        )
    }
}
