use super::context::Rule;
use super::{Keyword, Kind, Operator, Parsed, Parser, TokenKind};

impl<const MARKUP: bool> Parser<'_, MARKUP> {
    pub(super) fn annotation(&mut self) -> Parsed {
        self.annotation_context(false, false)
    }

    pub(super) fn declaration_annotation(&mut self) -> Parsed {
        self.annotation_context(false, true)
    }

    pub(super) fn type_argument(&mut self) -> Parsed {
        self.annotation_context(true, false)
    }

    pub(super) fn annotation_context(&mut self, allow_pack: bool, declaration: bool) -> Parsed {
        self.scoped(
            Rule::Annotation {
                allow_pack,
                declaration,
            },
            |parser| {
                Ok(parser.required("annotation", |parser| {
                    if allow_pack
                        && (parser.at(TokenKind::Operator(Operator::Ellipsis))
                            || (parser.at(TokenKind::Name)
                                && parser.next() == TokenKind::Operator(Operator::Ellipsis)))
                    {
                        parser.pack()
                    } else {
                        parser.nested(|parser| Ok(parser.composite(allow_pack, declaration)))
                    }
                }))
            },
        )
    }

    pub(super) fn pack(&mut self) -> Parsed {
        let start = self.current().span.start;

        if self.consume(TokenKind::Operator(Operator::Ellipsis)) {
            let annotation = self.annotation()?;

            Ok(self.node(Kind::VariadicType, start, [annotation]))
        } else {
            let name = self.name();

            self.expect(
                TokenKind::Operator(Operator::Ellipsis),
                "expected generic pack",
            );

            Ok(self.node(Kind::GenericPack, start, [name]))
        }
    }

    fn composite(&mut self, allow_pack: bool, declaration: bool) -> usize {
        let start = self.current().span.start;

        let leading = if self.byte(b'|') || self.byte(b'&') {
            Some(self.take().kind)
        } else {
            None
        };

        let mut left = self.required("annotation", |parser| {
            parser.simple_type(
                allow_pack && leading.is_none(),
                declaration && leading.is_none(),
            )
        });

        self.inspect(self.builder.nodes[left].span);

        if self.builder.nodes[left].kind == Kind::TypePack {
            return left;
        }

        if let Some(token) = leading {
            left = self.node(
                if token == TokenKind::Byte(b'|') {
                    Kind::TypeUnion
                } else {
                    Kind::TypeIntersection
                },
                start,
                [left],
            );
        }

        let mut separator = leading;

        loop {
            if self.byte(b'?') {
                if separator == Some(TokenKind::Byte(b'&')) {
                    self.diagnose(self.error("optional intersection requires parentheses"));
                }

                separator = Some(TokenKind::Byte(b'|'));
                self.take();
                left = self.node(Kind::TypeOptional, start, [left]);
            } else if self.byte(b'|') || self.byte(b'&') {
                let token = self.take().kind;

                if separator.is_some_and(|previous| previous != token) {
                    self.diagnose(self.error("mixed union and intersection requires parentheses"));
                }

                separator = Some(token);

                let right = self.required("annotation", |parser| {
                    parser.nested(|parser| parser.simple_type(false, false))
                });

                left = self.node(
                    if token == TokenKind::Byte(b'|') {
                        Kind::TypeUnion
                    } else {
                        Kind::TypeIntersection
                    },
                    start,
                    [left, right],
                );
            } else {
                break;
            }
        }

        left
    }

    fn simple_type(&mut self, allow_pack: bool, declaration: bool) -> Parsed {
        let start = self.current().span.start;

        match self.current().kind {
            TokenKind::Keyword(Keyword::Nil) => Ok(self.leaf(Kind::Nil)),
            TokenKind::Keyword(Keyword::True | Keyword::False) => Ok(self.leaf(Kind::Boolean)),
            TokenKind::QuotedString | TokenKind::RawString => Ok(self.string()),
            TokenKind::Byte(b'{') => self.type_table(declaration),
            TokenKind::Byte(b'(' | b'<') => self.function_type(allow_pack),

            TokenKind::Attribute | TokenKind::AttributeOpen => {
                if !declaration {
                    self.diagnose(self.error("function type attributes require a declaration"));
                }

                let attributes = self.attributes()?;
                let function = self.function_type(false)?;

                self.inspect(self.builder.nodes[function].span);

                if self.builder.nodes[function].kind != Kind::TypeFunctionExpression {
                    self.builder.nodes[function].kind = Kind::Parameters;

                    let returns = self.missing(
                        self.error("expected attributed function type"),
                        "annotation",
                    );

                    return Ok(self.node(
                        Kind::TypeFunctionExpression,
                        start,
                        [attributes, function, returns],
                    ));
                }

                self.builder.nodes[function].span.start = start;
                self.prepend(function, attributes);

                Ok(function)
            }

            TokenKind::Name => {
                let typeof_expression =
                    self.named(b"typeof") && self.next() != TokenKind::Byte(b'.');

                let name = self.name();

                if typeof_expression {
                    self.expect(TokenKind::Byte(b'('), "expected typeof expression");
                    let expression = self.expression(0)?;
                    self.expect(TokenKind::Byte(b')'), "expected closing typeof");

                    return Ok(self.node(Kind::TypeOf, start, [name, expression]));
                }

                let member = if self.consume(TokenKind::Byte(b'.')) {
                    Some(self.name())
                } else {
                    None
                };

                let arguments = if self.byte(b'<') {
                    Some(self.type_arguments()?)
                } else {
                    None
                };

                Ok(self.node(
                    Kind::TypeName,
                    start,
                    [name].into_iter().chain(member).chain(arguments),
                ))
            }

            _ => Err(self.error("expected type")),
        }
    }

    pub(super) fn type_arguments(&mut self) -> Parsed {
        self.scoped(Rule::TypeArguments, Self::type_argument_contents)
    }

    fn type_argument_contents(&mut self) -> Parsed {
        let start = self.current().span.start;
        self.expect(TokenKind::Byte(b'<'), "expected type arguments");
        let mut arguments = Vec::new();

        if !self.byte(b'>') {
            arguments.push(self.type_argument()?);

            while self.consume(TokenKind::Byte(b',')) {
                arguments.push(self.type_argument()?);
            }
        }

        self.expect(TokenKind::Byte(b'>'), "expected closing type arguments");

        Ok(self.node(Kind::TypeArguments, start, arguments))
    }

    pub(super) fn generics(&mut self, defaults: bool) -> Parsed {
        self.scoped(Rule::Generics { defaults }, |parser| {
            parser.generic_contents(defaults)
        })
    }

    fn generic_contents(&mut self, defaults: bool) -> Parsed {
        let start = self.take().span.start;
        let mut parameters = Vec::new();
        let mut packs = false;
        let mut defaulted = false;

        loop {
            let begin = self.current().span.start;
            let mut children = vec![self.name()];
            let pack = self.consume(TokenKind::Operator(Operator::Ellipsis));

            if packs && !pack {
                self.diagnose(self.error("type parameters must precede packs"));
            }

            packs |= pack;

            if self.consume(TokenKind::Byte(b'=')) {
                if !defaults {
                    self.diagnose(self.error("generic defaults are only allowed in type aliases"));
                }

                defaulted = true;

                let default = if pack {
                    self.type_argument()?
                } else {
                    self.annotation()?
                };

                if pack {
                    self.inspect(self.builder.nodes[default].span);
                }

                if pack
                    && !matches!(
                        self.builder.nodes[default].kind,
                        Kind::TypePack | Kind::GenericPack | Kind::VariadicType
                    )
                {
                    self.diagnose(self.error("expected type pack default"));
                }

                children.push(default);
            } else if defaulted {
                children
                    .push(self.missing(self.error("expected generic default"), "generic default"));
            }

            parameters.push(self.node(
                if pack {
                    Kind::GenericPack
                } else {
                    Kind::Generic
                },
                begin,
                children,
            ));

            if !self.consume(TokenKind::Byte(b',')) {
                break;
            }
        }

        self.expect(TokenKind::Byte(b'>'), "expected closing generics");

        Ok(self.node(Kind::Generics, start, parameters))
    }

    pub(super) fn type_parameters(&mut self) -> Parsed {
        self.scoped(
            Rule::Parameters { types: true },
            Self::type_parameter_contents,
        )
    }

    fn type_parameter_contents(&mut self) -> Parsed {
        let start = self.current().span.start;
        self.expect(TokenKind::Byte(b'('), "expected type parameters");
        let mut parameters = Vec::new();

        if !self.byte(b')') {
            loop {
                if self.at(TokenKind::Operator(Operator::Ellipsis))
                    || (self.at(TokenKind::Name)
                        && self.next() == TokenKind::Operator(Operator::Ellipsis))
                {
                    parameters.push(self.pack()?);
                    break;
                }

                if self.at(TokenKind::Name) && self.next() == TokenKind::Byte(b':') {
                    let begin = self.current().span.start;
                    let name = self.name();
                    self.take();
                    let annotation = self.annotation()?;
                    parameters.push(self.node(Kind::TypeParameter, begin, [name, annotation]));
                } else {
                    parameters.push(self.annotation()?);
                }

                if !self.consume(TokenKind::Byte(b',')) {
                    break;
                }
            }
        }

        self.expect(TokenKind::Byte(b')'), "expected closing type parameters");

        Ok(self.node(Kind::Parameters, start, parameters))
    }

    fn function_type(&mut self, allow_pack: bool) -> Parsed {
        let start = self.current().span.start;
        let mut children = Vec::new();

        if self.byte(b'<') {
            children.push(self.generics(false)?);
        }

        let parameters = self.type_parameters()?;

        self.inspect(self.builder.nodes[parameters].span);

        let named = self.builder.children[self.builder.nodes[parameters].children.clone()]
            .iter()
            .any(|index| self.builder.nodes[*index].kind == Kind::TypeParameter);

        if self.consume(TokenKind::Operator(Operator::Arrow)) {
            children.push(parameters);
            children.push(self.type_argument()?);

            return Ok(self.node(Kind::TypeFunctionExpression, start, children));
        }

        if !children.is_empty() || named {
            let position = self.current().span.start;

            self.expectation(
                crate::Span {
                    start: position,
                    end: position,
                },
                super::Expected::Token(TokenKind::Operator(Operator::Arrow)),
            );

            children.push(parameters);
            children.push(self.missing(self.error("expected function type arrow"), "annotation"));

            return Ok(self.node(Kind::TypeFunctionExpression, start, children));
        }

        let parts = &self.builder.children[self.builder.nodes[parameters].children.clone()];

        let single = parts.len() == 1
            && !matches!(
                self.builder.nodes[parts[0]].kind,
                Kind::GenericPack | Kind::VariadicType
            );

        if !allow_pack && !single {
            let position = self.current().span.start;

            self.expectation(
                crate::Span {
                    start: position,
                    end: position,
                },
                super::Expected::Token(TokenKind::Operator(Operator::Arrow)),
            );

            children.push(parameters);
            children.push(self.missing(self.error("expected function type arrow"), "annotation"));

            return Ok(self.node(Kind::TypeFunctionExpression, start, children));
        }

        self.builder.nodes[parameters].kind =
            if allow_pack && !(single && (self.byte(b'?') || self.byte(b'|') || self.byte(b'&'))) {
                Kind::TypePack
            } else {
                Kind::TypeGroup
            };

        Ok(parameters)
    }

    fn type_table(&mut self, declaration: bool) -> Parsed {
        let start = self.take().span.start;
        let mut fields = Vec::new();

        while !self.byte(b'}') {
            let begin = self.current().span.start;

            let access = if fields.is_empty()
                && (self.named(b"read") || self.named(b"write"))
                && self.next() != TokenKind::Byte(b':')
            {
                Some(self.leaf(Kind::Operator))
            } else {
                None
            };

            let shorthand = fields.is_empty()
                && !self.byte(b'[')
                && !(self.at(TokenKind::Name) && self.next() == TokenKind::Byte(b':'));

            if shorthand {
                fields.extend(access);
                fields.push(self.annotation()?);
                break;
            }

            let field = self.type_field(declaration)?;

            if let Some(access) = access {
                self.builder.nodes[field].span.start = begin;
                self.prepend(field, access);
            }

            fields.push(field);

            if !self.consume(TokenKind::Byte(b',')) && !self.consume(TokenKind::Byte(b';')) {
                break;
            }
        }

        self.expect(TokenKind::Byte(b'}'), "expected closing table type");

        Ok(self.node(Kind::TypeTable, start, fields))
    }

    pub(super) fn type_field(&mut self, declaration: bool) -> Parsed {
        let start = self.current().span.start;

        let access = if (self.named(b"read") || self.named(b"write"))
            && self.next() != TokenKind::Byte(b':')
        {
            Some(self.leaf(Kind::Operator))
        } else {
            None
        };

        let indexed = self.consume(TokenKind::Byte(b'['));

        let property = indexed
            && matches!(
                self.current().kind,
                TokenKind::QuotedString | TokenKind::RawString
            )
            && self.next() == TokenKind::Byte(b']');

        let key = if indexed {
            let key = self.annotation()?;
            self.expect(TokenKind::Byte(b']'), "expected closing type index");

            key
        } else {
            self.name()
        };

        self.expect(TokenKind::Byte(b':'), "expected field type");

        let annotation = if declaration && !indexed {
            self.declaration_annotation()?
        } else {
            self.annotation()?
        };

        Ok(self.node(
            if indexed && !property {
                Kind::TypeIndexer
            } else {
                Kind::TypeField
            },
            start,
            access.into_iter().chain([key, annotation]),
        ))
    }
}
