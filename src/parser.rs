use bstr::{BStr, ByteSlice};

use crate::ast::{
    Attribute, BinaryOperator, Binding, Block, Chunk, ClassMember, ClassMemberKind, Expression,
    ExpressionKind, Function, FunctionName, FunctionSignature, GenericParameter, IfBranch,
    IfCondition, Statement, TableField, TableKey, TypeArgument, TypeExpression, TypeExpressionKind,
    TypeField, TypeIndexer, TypePack, TypePackTail, TypeParameter, UnaryOperator,
};
use crate::lexer::tokenize;
use crate::syntax::{InterpolatedKind, Keyword, Operator, Span, Token, TokenKind};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParseError {
    pub span: Span,
    pub kind: ParseErrorKind,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ParseErrorKind {
    Expected {
        expected: &'static str,
        found: TokenKind,
    },
    UnexpectedToken(TokenKind),
    InvalidAssignmentTarget,
    Lexical(TokenKind),
}

impl ParseError {
    fn expected(span: Span, expected: &'static str, found: TokenKind) -> Self {
        Self {
            span,
            kind: ParseErrorKind::Expected { expected, found },
        }
    }

    fn unexpected(token: Token) -> Self {
        let kind = match token.kind {
            TokenKind::Error(_) => ParseErrorKind::Lexical(token.kind),
            found => ParseErrorKind::UnexpectedToken(found),
        };
        Self {
            span: token.span,
            kind,
        }
    }
}

#[must_use]
pub struct Parser<'source> {
    source: &'source BStr,
    tokens: Vec<Token>,
    cursor: usize,
}

impl<'source> Parser<'source> {
    pub fn new(source: &'source BStr) -> Self {
        Self {
            source,
            tokens: tokenize(source),
            cursor: 0,
        }
    }

    /// # Errors
    ///
    /// Returns the first syntax or lexical error encountered in the source.
    pub fn parse(mut self) -> Result<Chunk, ParseError> {
        let body = self.parse_statement_list(&[], 0)?;
        self.expect_kind(TokenKind::Eof, "end of input")?;

        Ok(Chunk {
            span: Span {
                start: 0,
                end: self.source.len(),
            },
            body: body.body,
        })
    }

    // Statements

    fn parse_statement_list(
        &mut self,
        terminators: &[TokenKind],
        start: usize,
    ) -> Result<Block, ParseError> {
        let mut body = Vec::new();

        loop {
            let token = self.current();
            if terminators.contains(&token.kind) || token.kind == TokenKind::Eof {
                break;
            }

            if let Some(semicolon) = self.consume_byte(b';') {
                body.push(Statement::Empty {
                    span: semicolon.span,
                });
            } else {
                let mut statement = self.parse_statement()?;
                if let Some(semicolon) = self.consume_byte(b';') {
                    extend_statement_span(&mut statement, semicolon.span.end);
                }
                body.push(statement);
            }
        }

        let end = self.current().span.start.max(start);
        Ok(Block {
            span: Span { start, end },
            body,
        })
    }

    fn parse_statement(&mut self) -> Result<Statement, ParseError> {
        let attributes = self.parse_attributes()?;
        let start = attributes.first().map_or_else(
            || self.current().span.start,
            |attribute| attribute.span.start,
        );

        match self.current().kind {
            TokenKind::Keyword(Keyword::Local) => self.parse_local(start, attributes, false),
            TokenKind::Keyword(Keyword::Function) => {
                self.parse_function_statement(start, attributes)
            }

            TokenKind::Keyword(Keyword::If) => self.parse_if(start),
            TokenKind::Keyword(Keyword::While) => self.parse_while(start),
            TokenKind::Keyword(Keyword::Repeat) => self.parse_repeat(start),
            TokenKind::Keyword(Keyword::Do) => self.parse_do(start),
            TokenKind::Keyword(Keyword::For) => self.parse_for(start),

            TokenKind::Keyword(Keyword::Return) => self.parse_return(start),
            TokenKind::Keyword(Keyword::Break) => self.parse_break(start),

            TokenKind::Name
                if self.is_name(b"const") && matches!(self.lookahead(1).kind, TokenKind::Name) =>
            {
                self.parse_local(start, attributes, true)
            }
            TokenKind::Name
                if self.is_name(b"type")
                    && matches!(
                        self.lookahead(1).kind,
                        TokenKind::Name | TokenKind::Keyword(Keyword::Function)
                    ) =>
            {
                self.parse_type_alias(start, false)
            }
            TokenKind::Name
                if self.is_name(b"class") && matches!(self.lookahead(1).kind, TokenKind::Name) =>
            {
                self.parse_class(start, false, false)
            }
            TokenKind::Name if self.is_name(b"open") && self.lookahead_is_name(1, b"class") => {
                self.parse_open_class(start, false)
            }
            TokenKind::Name
                if self.is_name(b"declare")
                    && matches!(
                        self.lookahead(1).kind,
                        TokenKind::Name | TokenKind::Keyword(Keyword::Function)
                    ) =>
            {
                self.parse_declare(start)
            }
            TokenKind::Name if self.is_name(b"export") && self.is_export_start() => {
                self.parse_export(start)
            }

            TokenKind::Name if self.is_name(b"continue") && self.is_bare_continue() => {
                if attributes.is_empty() {
                    let token = self.take();
                    Ok(Statement::Continue { span: token.span })
                } else {
                    Err(ParseError::expected(
                        self.current().span,
                        "local or function statement",
                        self.current().kind,
                    ))
                }
            }
            TokenKind::Name | TokenKind::Byte(b'(') => {
                if attributes.is_empty() {
                    self.parse_assignment_or_call()
                } else {
                    Err(ParseError::expected(
                        self.current().span,
                        "local or function statement",
                        self.current().kind,
                    ))
                }
            }
            _ if attributes.is_empty() => self.parse_assignment_or_call(),
            _ => Err(ParseError::unexpected(self.current())),
        }
    }

    fn parse_local(
        &mut self,
        start: usize,
        attributes: Vec<Attribute>,
        is_const: bool,
    ) -> Result<Statement, ParseError> {
        if is_const {
            self.expect_name("const")?;
        } else {
            self.expect_keyword(Keyword::Local)?;
        }

        if self.consume_keyword(Keyword::Function).is_some() {
            let name = self.expect_name("function name")?;
            let function = self.parse_function_body(start)?;
            return Ok(Statement::LocalFunction {
                span: Span {
                    start,
                    end: function.span.end,
                },
                attributes,
                name: name.span,
                function,
            });
        }

        let bindings = self.parse_bindings(is_const)?;

        let values = if self.consume_byte(b'=').is_some() {
            self.parse_expression_list()?
        } else {
            Vec::new()
        };
        let end = values.last().map_or_else(
            || bindings.last().map_or(start, |binding| binding.span.end),
            |value| value.span.end,
        );

        Ok(Statement::Local {
            span: Span { start, end },
            attributes,
            bindings,
            values,
            is_const,
        })
    }

    fn parse_function_statement(
        &mut self,
        start: usize,
        attributes: Vec<Attribute>,
    ) -> Result<Statement, ParseError> {
        self.expect_keyword(Keyword::Function)?;
        let name = self.parse_function_name()?;
        let function = self.parse_function_body(start)?;
        let end = function.span.end;

        Ok(Statement::Function {
            span: Span { start, end },
            attributes,
            name,
            function,
        })
    }

    fn parse_type_alias(&mut self, start: usize, exported: bool) -> Result<Statement, ParseError> {
        self.expect_name("type")?;
        if self.consume_keyword(Keyword::Function).is_some() {
            let name = self.expect_name("type function name")?;
            let function = self.parse_function_body(start)?;
            return Ok(Statement::TypeFunction {
                span: Span {
                    start,
                    end: function.span.end,
                },
                exported,
                name: name.span,
                function,
            });
        }

        let name = self.expect_name("type name")?;
        let generics = self.parse_generic_parameters()?;
        self.expect_byte(b'=', "type alias")?;
        let value = self.parse_type()?;

        Ok(Statement::TypeAlias {
            span: Span {
                start,
                end: value.span.end,
            },
            exported,
            name: name.span,
            generics,
            value,
        })
    }

    fn parse_function_signature(&mut self, start: usize) -> Result<FunctionSignature, ParseError> {
        let generics = self.parse_generic_parameters()?;
        self.expect_byte(b'(', "function signature")?;
        let mut parameters = Vec::new();
        let mut variadic = None;

        if !self.at_byte(b')') {
            loop {
                if self.consume_operator(Operator::Ellipsis).is_some() {
                    variadic = Some(self.parse_type()?);
                    break;
                }
                parameters.push(self.parse_type_parameter()?);
                if self.consume_byte(b',').is_none() {
                    break;
                }
            }
        }
        self.expect_byte(b')', "closing function signature")?;
        let returns = if self.consume_byte(b':').is_some() {
            self.parse_return_type()?
        } else {
            TypePack {
                span: Span {
                    start: self.previous_end(),
                    end: self.previous_end(),
                },
                types: Vec::new(),
                tail: None,
            }
        };

        Ok(FunctionSignature {
            span: Span {
                start,
                end: returns.span.end.max(self.previous_end()),
            },
            generics,
            parameters,
            variadic,
            returns,
        })
    }

    fn parse_declare(&mut self, start: usize) -> Result<Statement, ParseError> {
        self.expect_name("declare")?;

        if self.is_name(b"global") {
            self.take();
            let name = self.expect_name("global name")?;
            self.expect_byte(b':', "global declaration")?;
            let annotation = self.parse_type()?;
            return Ok(Statement::DeclareGlobal {
                span: Span {
                    start,
                    end: annotation.span.end,
                },
                name: name.span,
                annotation,
            });
        }

        if self.consume_keyword(Keyword::Function).is_some() {
            let name = self.expect_name("declared function name")?;
            let signature = self.parse_function_signature(start)?;
            return Ok(Statement::DeclareFunction {
                span: Span {
                    start,
                    end: signature.span.end,
                },
                name: name.span,
                signature,
            });
        }

        if self.is_name(b"type") {
            return self.parse_type_alias(start, false);
        }

        if self.is_name(b"extern") {
            self.take();
            if self.is_name(b"type") {
                return self.parse_type_alias(start, false);
            }
        }

        Err(ParseError::expected(
            self.current().span,
            "global, function, or type declaration",
            self.current().kind,
        ))
    }

    fn parse_class(
        &mut self,
        start: usize,
        exported: bool,
        open: bool,
    ) -> Result<Statement, ParseError> {
        self.expect_name("class")?;
        let name = self.expect_name("class name")?;
        let superclass = if self.is_name(b"extends") {
            self.take();
            Some(self.parse_type()?)
        } else {
            None
        };
        let mut members = Vec::new();

        while !self.at_keyword(Keyword::End) && self.current().kind != TokenKind::Eof {
            let member_start = self.current().span.start;
            if self.is_name(b"public") {
                self.take();
            }

            if self.consume_keyword(Keyword::Function).is_some() {
                let member_name = self.expect_name("class method name")?;
                let function = self.parse_function_body(member_start)?;
                members.push(ClassMember {
                    span: Span {
                        start: member_start,
                        end: function.span.end,
                    },
                    name: member_name.span,
                    kind: ClassMemberKind::Method { function },
                });
                continue;
            }

            let member_name = self.expect_name("class property name")?;
            let annotation = if self.consume_byte(b':').is_some() {
                Some(self.parse_type()?)
            } else {
                None
            };
            let end = annotation
                .as_ref()
                .map_or(member_name.span.end, |annotation| annotation.span.end);
            members.push(ClassMember {
                span: Span {
                    start: member_start,
                    end,
                },
                name: member_name.span,
                kind: ClassMemberKind::Property { annotation },
            });
        }
        let end = self.expect_keyword(Keyword::End)?;

        Ok(Statement::Class {
            span: Span {
                start,
                end: end.span.end,
            },
            exported,
            open,
            name: name.span,
            superclass,
            members,
        })
    }

    fn parse_open_class(&mut self, start: usize, exported: bool) -> Result<Statement, ParseError> {
        self.expect_name("open")?;
        if !self.is_name(b"class") {
            return Err(ParseError::expected(
                self.current().span,
                "class",
                self.current().kind,
            ));
        }
        self.parse_class(start, exported, true)
    }

    fn parse_export(&mut self, start: usize) -> Result<Statement, ParseError> {
        self.expect_name("export")?;

        if self.is_name(b"type") {
            return self.parse_type_alias(start, true);
        }
        if self.is_name(b"class") {
            return self.parse_class(start, true, false);
        }
        if self.is_name(b"open") {
            return self.parse_open_class(start, true);
        }

        let statement = match self.current().kind {
            TokenKind::Keyword(Keyword::Local) => self.parse_local(start, Vec::new(), false)?,
            TokenKind::Keyword(Keyword::Function) => {
                self.parse_function_statement(start, Vec::new())?
            }
            TokenKind::Name if self.is_name(b"const") => {
                self.parse_local(start, Vec::new(), true)?
            }
            _ => self.parse_assignment_or_call()?,
        };
        let end = statement_span(&statement).end;

        Ok(Statement::Export {
            span: Span { start, end },
            statement: Box::new(statement),
        })
    }

    fn parse_if(&mut self, start: usize) -> Result<Statement, ParseError> {
        self.expect_keyword(Keyword::If)?;
        let mut branches = Vec::new();

        loop {
            let condition = self.parse_if_condition()?;

            self.expect_keyword(Keyword::Then)?;
            let body_start = self.current().span.start;
            let body = self.parse_statement_list(
                &[
                    TokenKind::Keyword(Keyword::ElseIf),
                    TokenKind::Keyword(Keyword::Else),
                    TokenKind::Keyword(Keyword::End),
                ],
                body_start,
            )?;
            branches.push(IfBranch { condition, body });

            if self.consume_keyword(Keyword::ElseIf).is_none() {
                break;
            }
        }

        let else_body = if self.consume_keyword(Keyword::Else).is_some() {
            let body_start = self.current().span.start;

            Some(self.parse_statement_list(&[TokenKind::Keyword(Keyword::End)], body_start)?)
        } else {
            None
        };

        let end = self.expect_keyword(Keyword::End)?;

        Ok(Statement::If {
            span: Span {
                start,
                end: end.span.end,
            },
            branches,
            else_body,
        })
    }

    fn parse_if_condition(&mut self) -> Result<IfCondition, ParseError> {
        if self.consume_keyword(Keyword::Local).is_some() {
            let binding = self.parse_binding(false)?;
            self.expect_byte(b'=', "if local condition")?;
            let value = self.parse_expression(0)?;
            return Ok(IfCondition::Local { binding, value });
        }

        if self.is_name(b"const") {
            self.take();
            let binding = self.parse_binding(true)?;
            self.expect_byte(b'=', "if const condition")?;
            let value = self.parse_expression(0)?;
            return Ok(IfCondition::Local { binding, value });
        }

        Ok(IfCondition::Expression(self.parse_expression(0)?))
    }

    fn parse_while(&mut self, start: usize) -> Result<Statement, ParseError> {
        self.expect_keyword(Keyword::While)?;
        let condition = self.parse_expression(0)?;

        self.expect_keyword(Keyword::Do)?;
        let body_start = self.current().span.start;
        let body = self.parse_statement_list(&[TokenKind::Keyword(Keyword::End)], body_start)?;
        let end = self.expect_keyword(Keyword::End)?;

        Ok(Statement::While {
            span: Span {
                start,
                end: end.span.end,
            },
            condition,
            body,
        })
    }

    fn parse_repeat(&mut self, start: usize) -> Result<Statement, ParseError> {
        self.expect_keyword(Keyword::Repeat)?;
        let body_start = self.current().span.start;
        let body = self.parse_statement_list(&[TokenKind::Keyword(Keyword::Until)], body_start)?;

        self.expect_keyword(Keyword::Until)?;
        let condition = self.parse_expression(0)?;

        Ok(Statement::Repeat {
            span: Span {
                start,
                end: condition.span.end,
            },
            body,
            condition,
        })
    }

    fn parse_do(&mut self, start: usize) -> Result<Statement, ParseError> {
        self.expect_keyword(Keyword::Do)?;
        let body_start = self.current().span.start;
        let body = self.parse_statement_list(&[TokenKind::Keyword(Keyword::End)], body_start)?;

        let end = self.expect_keyword(Keyword::End)?;

        Ok(Statement::Do {
            span: Span {
                start,
                end: end.span.end,
            },
            body,
        })
    }

    fn parse_for(&mut self, start: usize) -> Result<Statement, ParseError> {
        self.expect_keyword(Keyword::For)?;
        let first = self.parse_binding(false)?;

        if self.consume_byte(b'=').is_some() {
            let from = self.parse_expression(0)?;
            self.expect_byte(b',', "comma")?;
            let to = self.parse_expression(0)?;
            let step = if self.consume_byte(b',').is_some() {
                Some(self.parse_expression(0)?)
            } else {
                None
            };

            self.expect_keyword(Keyword::Do)?;
            let body_start = self.current().span.start;
            let body =
                self.parse_statement_list(&[TokenKind::Keyword(Keyword::End)], body_start)?;
            let end = self.expect_keyword(Keyword::End)?;

            return Ok(Statement::NumericFor {
                span: Span {
                    start,
                    end: end.span.end,
                },
                binding: first,
                from: Box::new(from),
                to: Box::new(to),
                step: step.map(Box::new),
                body,
            });
        }

        let mut bindings = vec![first];
        while self.consume_byte(b',').is_some() {
            bindings.push(self.parse_binding(false)?);
        }
        self.expect_keyword(Keyword::In)?;
        let values = self.parse_expression_list()?;

        self.expect_keyword(Keyword::Do)?;
        let body_start = self.current().span.start;
        let body = self.parse_statement_list(&[TokenKind::Keyword(Keyword::End)], body_start)?;
        let end = self.expect_keyword(Keyword::End)?;

        Ok(Statement::GenericFor {
            span: Span {
                start,
                end: end.span.end,
            },
            bindings,
            values,
            body,
        })
    }

    fn parse_return(&mut self, start: usize) -> Result<Statement, ParseError> {
        let return_token = self.expect_keyword(Keyword::Return)?;
        let values = if self.is_statement_end() {
            Vec::new()
        } else {
            self.parse_expression_list()?
        };
        let end = values
            .last()
            .map_or(return_token.span.end, |value| value.span.end);

        Ok(Statement::Return {
            span: Span { start, end },
            values,
        })
    }

    fn parse_break(&mut self, start: usize) -> Result<Statement, ParseError> {
        let token = self.expect_keyword(Keyword::Break)?;
        Ok(Statement::Break {
            span: Span {
                start,
                end: token.span.end,
            },
        })
    }

    fn parse_assignment_or_call(&mut self) -> Result<Statement, ParseError> {
        let first = self.parse_expression(0)?;

        if let Some(operator) = self.compound_assignment_operator() {
            if !is_assignable(&first) {
                return Err(ParseError {
                    span: first.span,
                    kind: ParseErrorKind::InvalidAssignmentTarget,
                });
            }
            self.take();
            let value = self.parse_expression(0)?;

            return Ok(Statement::CompoundAssignment {
                span: Span {
                    start: first.span.start,
                    end: value.span.end,
                },
                target: Box::new(first),
                operator,
                value: Box::new(value),
            });
        }

        if self.at_byte(b'=') || self.at_byte(b',') {
            if !is_assignable(&first) {
                return Err(ParseError {
                    span: first.span,
                    kind: ParseErrorKind::InvalidAssignmentTarget,
                });
            }
            let mut targets = vec![first];

            while self.consume_byte(b',').is_some() {
                let target = self.parse_expression(0)?;
                if !is_assignable(&target) {
                    return Err(ParseError {
                        span: target.span,
                        kind: ParseErrorKind::InvalidAssignmentTarget,
                    });
                }
                targets.push(target);
            }
            self.expect_byte(b'=', "assignment")?;
            let values = self.parse_expression_list()?;

            let end = values.last().map_or_else(
                || {
                    targets
                        .last()
                        .map_or(first_span(&targets), |target| target.span.end)
                },
                |value| value.span.end,
            );
            return Ok(Statement::Assignment {
                span: Span {
                    start: targets[0].span.start,
                    end,
                },
                targets,
                values,
            });
        }

        if matches!(first.kind, ExpressionKind::Call { .. }) {
            return Ok(Statement::Call {
                span: first.span,
                expression: first,
            });
        }

        Err(ParseError::expected(
            self.current().span,
            "assignment or function call",
            self.current().kind,
        ))
    }

    fn parse_bindings(&mut self, is_const: bool) -> Result<Vec<Binding>, ParseError> {
        let mut bindings = vec![self.parse_binding(is_const)?];
        while self.consume_byte(b',').is_some() {
            bindings.push(self.parse_binding(is_const)?);
        }
        Ok(bindings)
    }

    fn parse_binding(&mut self, is_const: bool) -> Result<Binding, ParseError> {
        let token = self.expect_name("name")?;
        let annotation = if self.consume_byte(b':').is_some() {
            Some(self.parse_type()?)
        } else {
            None
        };
        let end = annotation.as_ref().map_or(token.span.end, |ty| ty.span.end);

        Ok(Binding {
            span: Span {
                start: token.span.start,
                end,
            },
            name: token.span,
            annotation,
            is_const,
        })
    }

    fn parse_function_name(&mut self) -> Result<FunctionName, ParseError> {
        let first = self.expect_name("function name")?;
        let start = first.span.start;
        let mut parts = vec![first.span];
        let mut end = first.span.end;

        while self.consume_byte(b'.').is_some() {
            let part = self.expect_name("function name")?;
            end = part.span.end;
            parts.push(part.span);
        }

        let method = if self.consume_byte(b':').is_some() {
            let method = self.expect_name("method name")?;
            end = method.span.end;
            Some(method.span)
        } else {
            None
        };

        Ok(FunctionName {
            span: Span { start, end },
            parts,
            method,
        })
    }

    fn parse_function_body(&mut self, start: usize) -> Result<Function, ParseError> {
        let generics = self.parse_generic_parameters()?;
        self.expect_byte(b'(', "function parameter list")?;
        let mut parameters = Vec::new();
        let mut variadic = false;
        let mut variadic_type = None;

        if !self.at_byte(b')') {
            loop {
                if self.consume_operator(Operator::Ellipsis).is_some() {
                    variadic = true;
                    variadic_type = if self.consume_byte(b':').is_some() {
                        Some(self.parse_return_type()?)
                    } else {
                        None
                    };
                    break;
                }
                parameters.push(self.parse_binding(false)?);
                if self.consume_byte(b',').is_none() {
                    break;
                }
                if self.at_byte(b')') {
                    break;
                }
            }
        }

        let close = self.expect_byte(b')', "closing parenthesis")?;
        let return_types = if self.consume_byte(b':').is_some() {
            Some(self.parse_return_type()?)
        } else {
            None
        };
        let body_start = self.current().span.start;
        let body = self.parse_statement_list(&[TokenKind::Keyword(Keyword::End)], body_start)?;
        let end = self.expect_keyword(Keyword::End)?;

        Ok(Function {
            span: Span {
                start,
                end: end.span.end.max(close.span.end),
            },
            attributes: Vec::new(),
            generics,
            parameters,
            variadic,
            variadic_type,
            return_types,
            body,
        })
    }

    // Types

    fn parse_generic_parameters(&mut self) -> Result<Vec<GenericParameter>, ParseError> {
        if !self.at_byte(b'<') {
            return Ok(Vec::new());
        }

        let open = self.take();
        let mut parameters = Vec::new();
        while !self.at_byte(b'>') {
            let name = self.expect_name("generic parameter")?;
            let is_pack = self.consume_operator(Operator::Ellipsis).is_some();
            let default = if self.consume_byte(b'=').is_some() {
                Some(self.parse_type()?)
            } else {
                None
            };
            let end = default.as_ref().map_or_else(
                || {
                    if is_pack {
                        self.previous_end()
                    } else {
                        name.span.end
                    }
                },
                |ty| ty.span.end,
            );
            parameters.push(GenericParameter {
                span: Span {
                    start: name.span.start,
                    end,
                },
                name: name.span,
                is_pack,
                default,
            });

            if self.consume_byte(b',').is_none() {
                break;
            }
        }
        let close = self.expect_byte(b'>', "closing generic parameter list")?;

        if parameters.is_empty() {
            return Err(ParseError::expected(
                Span {
                    start: open.span.start,
                    end: close.span.end,
                },
                "generic parameter",
                TokenKind::Byte(b'>'),
            ));
        }
        Ok(parameters)
    }

    fn parse_type_arguments(&mut self) -> Result<Vec<TypeArgument>, ParseError> {
        self.expect_byte(b'<', "type arguments")?;
        let mut arguments = Vec::new();
        if !self.at_byte(b'>') {
            loop {
                arguments.push(self.parse_type_argument()?);
                if self.consume_byte(b',').is_none() {
                    break;
                }
            }
        }
        self.expect_byte(b'>', "closing type arguments")?;
        Ok(arguments)
    }

    fn parse_type_argument(&mut self) -> Result<TypeArgument, ParseError> {
        if self.at_byte(b'(') {
            let cursor = self.cursor;
            if let Ok(ty) = self.parse_parenthesized_type()
                && matches!(ty.kind, TypeExpressionKind::Function { .. })
            {
                return Ok(TypeArgument::Type(ty));
            }
            self.cursor = cursor;

            let open = self.take();
            let mut pack = self.parse_type_pack(open.span.start)?;
            self.expect_byte(b')', "closing type argument pack")?;
            pack.span.end = open.span.end;
            return Ok(TypeArgument::Pack(pack));
        }

        let ty = self.parse_type()?;
        if self.consume_operator(Operator::Ellipsis).is_some() {
            let tail = match ty.kind {
                TypeExpressionKind::Name { ref path, .. } if path.len() == 1 => {
                    TypePackTail::Generic(path[0])
                }
                _ => TypePackTail::Variadic(Box::new(ty)),
            };
            let end = self.previous_end();
            return Ok(TypeArgument::Pack(TypePack {
                span: Span {
                    start: match &tail {
                        TypePackTail::Generic(span) => span.start,
                        TypePackTail::Variadic(ty) => ty.span.start,
                    },
                    end,
                },
                types: Vec::new(),
                tail: Some(tail),
            }));
        }
        Ok(TypeArgument::Type(ty))
    }

    fn parse_explicit_type_arguments(&mut self) -> Result<(Vec<TypeArgument>, Span), ParseError> {
        let first = self.expect_byte(b'<', "explicit type arguments")?;
        self.expect_byte(b'<', "second type argument delimiter")?;

        let mut arguments = Vec::new();
        if !self.at_byte(b'>') {
            loop {
                arguments.push(self.parse_type_argument()?);
                if self.consume_byte(b',').is_none() {
                    break;
                }
            }
        }
        self.expect_byte(b'>', "closing explicit type arguments")?;
        self.expect_byte(b'>', "closing explicit type arguments")?;
        if arguments.is_empty() {
            return Err(ParseError::expected(
                first.span,
                "explicit type argument",
                TokenKind::Byte(b'>'),
            ));
        }
        let end = self.previous_end();
        Ok((
            arguments,
            Span {
                start: first.span.start,
                end,
            },
        ))
    }

    fn try_parse_explicit_type_arguments(
        &mut self,
    ) -> Result<Option<(Vec<TypeArgument>, Span)>, ParseError> {
        if !self.at_byte(b'<') || !self.lookahead_is_adjacent_byte(1, b'<') {
            return Ok(None);
        }
        self.parse_explicit_type_arguments().map(Some)
    }

    fn parse_return_type(&mut self) -> Result<TypePack, ParseError> {
        let start = self.current().span.start;
        if self.consume_operator(Operator::Ellipsis).is_some() {
            let ty = self.parse_type()?;
            return Ok(TypePack {
                span: Span {
                    start,
                    end: ty.span.end,
                },
                types: Vec::new(),
                tail: Some(TypePackTail::Variadic(Box::new(ty))),
            });
        }
        if self.at_byte(b'(') {
            let cursor = self.cursor;
            if let Ok(ty) = self.parse_parenthesized_type()
                && matches!(ty.kind, TypeExpressionKind::Function { .. })
            {
                return Ok(TypePack {
                    span: ty.span,
                    types: vec![ty],
                    tail: None,
                });
            }
            self.cursor = cursor;

            self.take();
            let pack = self.parse_type_pack(start)?;
            self.expect_byte(b')', "closing return type list")?;
            let end = self.previous_end();
            return Ok(TypePack {
                span: Span { start, end },
                ..pack
            });
        }

        let ty = self.parse_type()?;
        if self.consume_operator(Operator::Ellipsis).is_some() {
            let tail = match ty.kind {
                TypeExpressionKind::Name { ref path, .. } if path.len() == 1 => {
                    TypePackTail::Generic(path[0])
                }
                _ => TypePackTail::Variadic(Box::new(ty)),
            };
            let end = self.previous_end();
            return Ok(TypePack {
                span: Span {
                    start: match &tail {
                        TypePackTail::Generic(span) => span.start,
                        TypePackTail::Variadic(ty) => ty.span.start,
                    },
                    end,
                },
                types: Vec::new(),
                tail: Some(tail),
            });
        }
        Ok(TypePack {
            span: ty.span,
            types: vec![ty],
            tail: None,
        })
    }

    fn parse_type_pack(&mut self, start: usize) -> Result<TypePack, ParseError> {
        let mut types = Vec::new();
        let mut tail = None;

        if !self.at_byte(b')') {
            loop {
                if self.consume_operator(Operator::Ellipsis).is_some() {
                    let ty = self.parse_type()?;
                    tail = Some(TypePackTail::Variadic(Box::new(ty)));
                    break;
                }

                let ty = self.parse_type()?;
                if self.consume_operator(Operator::Ellipsis).is_some() {
                    let tail_kind = match ty.kind {
                        TypeExpressionKind::Name { ref path, .. } if path.len() == 1 => {
                            TypePackTail::Generic(path[0])
                        }
                        _ => TypePackTail::Variadic(Box::new(ty)),
                    };
                    tail = Some(tail_kind);
                    break;
                }
                types.push(ty);

                if self.consume_byte(b',').is_none() {
                    break;
                }
            }
        }

        let end = tail.as_ref().map_or_else(
            || types.last().map_or(start, |ty| ty.span.end),
            |tail| match tail {
                TypePackTail::Variadic(ty) => ty.span.end,
                TypePackTail::Generic(span) => span.end,
            },
        );
        Ok(TypePack {
            span: Span { start, end },
            types,
            tail,
        })
    }

    fn parse_type(&mut self) -> Result<TypeExpression, ParseError> {
        let first = self.parse_intersection_type()?;
        let mut union = vec![first];

        while self.consume_byte(b'|').is_some() {
            union.push(self.parse_intersection_type()?);
        }

        let mut ty = if union.len() == 1 {
            union.pop().expect("type union has one member")
        } else {
            let span = Span {
                start: union[0].span.start,
                end: union.last().map_or(union[0].span.end, |part| part.span.end),
            };
            TypeExpression {
                span,
                kind: TypeExpressionKind::Union(union),
            }
        };

        while self.consume_byte(b'?').is_some() {
            ty = TypeExpression {
                span: Span {
                    start: ty.span.start,
                    end: self.previous_end(),
                },
                kind: TypeExpressionKind::Optional(Box::new(ty)),
            };
        }
        Ok(ty)
    }

    fn parse_intersection_type(&mut self) -> Result<TypeExpression, ParseError> {
        let first = self.parse_type_primary()?;
        let mut intersection = vec![first];

        while self.consume_byte(b'&').is_some() {
            intersection.push(self.parse_type_primary()?);
        }

        if intersection.len() == 1 {
            return Ok(intersection
                .pop()
                .expect("type intersection has one member"));
        }

        let span = Span {
            start: intersection[0].span.start,
            end: intersection
                .last()
                .map_or(intersection[0].span.end, |part| part.span.end),
        };
        Ok(TypeExpression {
            span,
            kind: TypeExpressionKind::Intersection(intersection),
        })
    }

    fn parse_type_primary(&mut self) -> Result<TypeExpression, ParseError> {
        let token = self.current();
        match token.kind {
            TokenKind::Keyword(Keyword::Nil) => {
                self.take();
                Ok(TypeExpression {
                    span: token.span,
                    kind: TypeExpressionKind::Nil,
                })
            }
            TokenKind::Keyword(Keyword::False | Keyword::True) => {
                self.take();
                Ok(TypeExpression {
                    span: token.span,
                    kind: TypeExpressionKind::Boolean(
                        token.kind == TokenKind::Keyword(Keyword::True),
                    ),
                })
            }
            TokenKind::Name if self.is_name(b"typeof") => {
                self.take();
                self.expect_byte(b'(', "typeof expression")?;
                let expression = self.parse_expression(0)?;
                let close = self.expect_byte(b')', "closing typeof expression")?;
                Ok(TypeExpression {
                    span: Span {
                        start: token.span.start,
                        end: close.span.end,
                    },
                    kind: TypeExpressionKind::Typeof(Box::new(expression)),
                })
            }
            TokenKind::Name => self.parse_named_type(),
            TokenKind::QuotedString | TokenKind::RawString => {
                self.take();
                Ok(TypeExpression {
                    span: token.span,
                    kind: TypeExpressionKind::String,
                })
            }
            TokenKind::Number => {
                self.take();
                Ok(TypeExpression {
                    span: token.span,
                    kind: TypeExpressionKind::Number,
                })
            }
            TokenKind::Byte(b'{') => self.parse_table_type(),
            TokenKind::Byte(b'(') => self.parse_parenthesized_type(),
            TokenKind::Byte(b'<') => self.parse_generic_function_type(),
            _ => Err(ParseError::unexpected(token)),
        }
    }

    fn parse_named_type(&mut self) -> Result<TypeExpression, ParseError> {
        let first = self.expect_name("type name")?;
        let mut path = vec![first.span];
        let mut end = first.span.end;
        while self.consume_byte(b'.').is_some() {
            let part = self.expect_name("type name")?;
            end = part.span.end;
            path.push(part.span);
        }

        let arguments = if self.at_byte(b'<') {
            let arguments = self.parse_type_arguments()?;
            end = self.previous_end();
            arguments
        } else {
            Vec::new()
        };
        Ok(TypeExpression {
            span: Span {
                start: first.span.start,
                end,
            },
            kind: TypeExpressionKind::Name { path, arguments },
        })
    }

    fn parse_generic_function_type(&mut self) -> Result<TypeExpression, ParseError> {
        let generics = self.parse_generic_parameters()?;
        let mut function = self.parse_parenthesized_type()?;
        if let TypeExpressionKind::Function {
            generics: function_generics,
            ..
        } = &mut function.kind
        {
            *function_generics = generics;
            Ok(function)
        } else {
            Err(ParseError::expected(
                function.span,
                "function type arrow",
                self.current().kind,
            ))
        }
    }

    fn parse_parenthesized_type(&mut self) -> Result<TypeExpression, ParseError> {
        let open = self.expect_byte(b'(', "type expression")?;
        if self.at_byte(b')') {
            self.take();
            self.expect_operator(Operator::Arrow, "function type")?;
            let returns = self.parse_return_type()?;
            return Ok(TypeExpression {
                span: Span {
                    start: open.span.start,
                    end: returns.span.end,
                },
                kind: TypeExpressionKind::Function {
                    generics: Vec::new(),
                    parameters: Vec::new(),
                    variadic: None,
                    returns,
                },
            });
        }

        let mut parameters = Vec::new();
        let mut variadic = None;
        loop {
            if self.consume_operator(Operator::Ellipsis).is_some() {
                variadic = Some(Box::new(self.parse_type()?));
                break;
            }
            parameters.push(self.parse_type_parameter()?);
            if self.consume_byte(b',').is_none() {
                break;
            }
        }
        let close = self.expect_byte(b')', "closing type parameters")?;

        if self.consume_operator(Operator::Arrow).is_some() {
            let returns = self.parse_return_type()?;
            return Ok(TypeExpression {
                span: Span {
                    start: open.span.start,
                    end: returns.span.end,
                },
                kind: TypeExpressionKind::Function {
                    generics: Vec::new(),
                    parameters,
                    variadic,
                    returns,
                },
            });
        }

        if parameters.len() != 1 || variadic.is_some() {
            return Err(ParseError::expected(
                Span {
                    start: open.span.start,
                    end: close.span.end,
                },
                "function type arrow",
                self.current().kind,
            ));
        }
        let inner = parameters.remove(0).annotation;
        Ok(TypeExpression {
            span: Span {
                start: open.span.start,
                end: close.span.end,
            },
            kind: TypeExpressionKind::Group(Box::new(inner)),
        })
    }

    fn parse_type_parameter(&mut self) -> Result<TypeParameter, ParseError> {
        let start = self.current().span.start;
        if self.current().kind == TokenKind::Name && self.lookahead(1).kind == TokenKind::Byte(b':')
        {
            let name = self.take().span;
            self.take();
            let annotation = self.parse_type()?;
            return Ok(TypeParameter {
                span: Span {
                    start,
                    end: annotation.span.end,
                },
                name: Some(name),
                annotation,
            });
        }

        let annotation = self.parse_type()?;
        Ok(TypeParameter {
            span: annotation.span,
            name: None,
            annotation,
        })
    }

    fn parse_table_type(&mut self) -> Result<TypeExpression, ParseError> {
        let open = self.expect_byte(b'{', "table type")?;
        let mut fields = Vec::new();
        let mut indexer = None;

        while !self.at_byte(b'}') {
            let start = self.current().span.start;
            if (self.is_name(b"read") || self.is_name(b"write"))
                && self.lookahead(1).kind != TokenKind::Byte(b':')
            {
                self.take();
            }

            if self.at_byte(b'[') {
                self.take();
                let key = self.parse_type()?;
                self.expect_byte(b']', "closing table index")?;
                self.expect_byte(b':', "table index type")?;
                let annotation = self.parse_type()?;
                if matches!(key.kind, TypeExpressionKind::String) {
                    fields.push(TypeField {
                        span: Span {
                            start,
                            end: annotation.span.end,
                        },
                        name: None,
                        key: Some(key),
                        annotation,
                        optional: false,
                    });
                } else {
                    indexer = Some(Box::new(TypeIndexer {
                        span: Span {
                            start,
                            end: annotation.span.end,
                        },
                        index: key,
                        result: annotation,
                        implicit: false,
                    }));
                }
            } else if fields.is_empty()
                && indexer.is_none()
                && (self.current().kind != TokenKind::Name
                    || self.lookahead(1).kind != TokenKind::Byte(b':'))
            {
                let annotation = self.parse_type()?;
                indexer = Some(Box::new(TypeIndexer {
                    span: annotation.span,
                    index: TypeExpression {
                        span: Span {
                            start: open.span.start,
                            end: open.span.start,
                        },
                        kind: TypeExpressionKind::Number,
                    },
                    result: annotation,
                    implicit: true,
                }));
            } else {
                let name = self.expect_name("table field name")?;
                let optional = self.consume_byte(b'?').is_some();
                self.expect_byte(b':', "table field type")?;
                let annotation = self.parse_type()?;
                fields.push(TypeField {
                    span: Span {
                        start,
                        end: annotation.span.end,
                    },
                    name: Some(name.span),
                    key: None,
                    annotation,
                    optional,
                });
            }

            if self.consume_byte(b',').is_none() && self.consume_byte(b';').is_none() {
                break;
            }
        }
        let close = self.expect_byte(b'}', "closing table type")?;

        Ok(TypeExpression {
            span: Span {
                start: open.span.start,
                end: close.span.end,
            },
            kind: TypeExpressionKind::Table { fields, indexer },
        })
    }

    // Expressions

    fn parse_expression_list(&mut self) -> Result<Vec<Expression>, ParseError> {
        let mut expressions = vec![self.parse_expression(0)?];
        while self.consume_byte(b',').is_some() {
            expressions.push(self.parse_expression(0)?);
        }
        Ok(expressions)
    }

    fn parse_expression(&mut self, minimum_binding_power: u8) -> Result<Expression, ParseError> {
        let mut left = self.parse_prefix_expression()?;
        if matches!(left.kind, ExpressionKind::Name | ExpressionKind::Group(_)) {
            self.parse_postfix_expressions(&mut left)?;
        }

        loop {
            if self.consume_operator(Operator::DoubleColon).is_some() {
                let annotation = self.parse_type()?;
                let span = Span {
                    start: left.span.start,
                    end: annotation.span.end,
                };
                left = Expression {
                    span,
                    kind: ExpressionKind::TypeAssertion {
                        expression: Box::new(left),
                        annotation,
                    },
                };
                continue;
            }

            let Some((operator, left_binding_power, right_binding_power)) = self.binary_operator()
            else {
                break;
            };
            if left_binding_power < minimum_binding_power {
                break;
            }
            self.consume_binary_operator(operator);
            let right = self.parse_expression(right_binding_power)?;
            let span = Span {
                start: left.span.start,
                end: right.span.end,
            };
            left = Expression {
                span,
                kind: ExpressionKind::Binary {
                    operator,
                    left: Box::new(left),
                    right: Box::new(right),
                },
            };
        }

        Ok(left)
    }

    fn parse_prefix_expression(&mut self) -> Result<Expression, ParseError> {
        let token = self.current();
        match token.kind {
            TokenKind::Keyword(Keyword::Nil) => {
                self.take();
                Ok(Expression {
                    span: token.span,
                    kind: ExpressionKind::Nil,
                })
            }

            TokenKind::Keyword(Keyword::False | Keyword::True) => {
                self.take();
                Ok(Expression {
                    span: token.span,
                    kind: ExpressionKind::Boolean(token.kind == TokenKind::Keyword(Keyword::True)),
                })
            }
            TokenKind::Name => {
                self.take();
                Ok(Expression {
                    span: token.span,
                    kind: ExpressionKind::Name,
                })
            }

            TokenKind::Number => {
                self.take();
                Ok(Expression {
                    span: token.span,
                    kind: ExpressionKind::Number,
                })
            }

            TokenKind::QuotedString
            | TokenKind::RawString
            | TokenKind::Interpolated(InterpolatedKind::Simple) => {
                self.take();
                Ok(Expression {
                    span: token.span,
                    kind: ExpressionKind::String,
                })
            }
            TokenKind::Interpolated(InterpolatedKind::Begin) => self.parse_interpolated_string(),

            TokenKind::Operator(Operator::Ellipsis) => {
                self.take();
                Ok(Expression {
                    span: token.span,
                    kind: ExpressionKind::Vararg,
                })
            }
            TokenKind::Keyword(Keyword::Not) | TokenKind::Byte(b'-' | b'#' | b'~') => {
                self.parse_unary_expression(token)
            }
            TokenKind::Byte(b'(') => self.parse_group_expression(),
            TokenKind::Byte(b'{') => self.parse_table_expression(),
            TokenKind::Attribute | TokenKind::AttributeOpen => {
                self.parse_attributed_function_expression()
            }
            TokenKind::Keyword(Keyword::Function) => {
                let function = self.parse_function_expression()?;
                let span = function.span;
                Ok(Expression {
                    span,
                    kind: ExpressionKind::Function(function),
                })
            }
            TokenKind::Keyword(Keyword::If) => self.parse_if_expression(),
            _ => Err(ParseError::unexpected(token)),
        }
    }

    fn parse_unary_expression(&mut self, token: Token) -> Result<Expression, ParseError> {
        self.take();
        let operator = match token.kind {
            TokenKind::Keyword(Keyword::Not) => UnaryOperator::Not,
            TokenKind::Byte(b'-') => UnaryOperator::Negate,
            TokenKind::Byte(b'#') => UnaryOperator::Length,
            TokenKind::Byte(b'~') => UnaryOperator::BitNot,
            _ => unreachable!(),
        };
        let operand = self.parse_expression(21)?;

        Ok(Expression {
            span: Span {
                start: token.span.start,
                end: operand.span.end,
            },
            kind: ExpressionKind::Unary {
                operator,
                operand: Box::new(operand),
            },
        })
    }

    fn parse_group_expression(&mut self) -> Result<Expression, ParseError> {
        let open = self.take();
        let expression = self.parse_expression(0)?;
        let close = self.expect_byte(b')', "closing parenthesis")?;

        Ok(Expression {
            span: Span {
                start: open.span.start,
                end: close.span.end,
            },
            kind: ExpressionKind::Group(Box::new(expression)),
        })
    }

    fn parse_attributed_function_expression(&mut self) -> Result<Expression, ParseError> {
        let attributes = self.parse_attributes()?;
        let start = attributes
            .first()
            .map_or(self.current().span.start, |attribute| attribute.span.start);
        self.expect_keyword(Keyword::Function)?;
        let mut function = self.parse_function_body(start)?;
        function.attributes = attributes;
        let span = function.span;

        Ok(Expression {
            span,
            kind: ExpressionKind::Function(function),
        })
    }

    fn parse_if_expression(&mut self) -> Result<Expression, ParseError> {
        let start = self.expect_keyword(Keyword::If)?.span.start;
        self.parse_if_expression_branch(start)
    }

    fn parse_if_expression_branch(&mut self, start: usize) -> Result<Expression, ParseError> {
        let condition = self.parse_expression(0)?;
        self.expect_keyword(Keyword::Then)?;
        let then_expression = self.parse_expression(0)?;

        let else_expression = if let Some(else_if) = self.consume_keyword(Keyword::ElseIf) {
            Box::new(self.parse_if_expression_branch(else_if.span.start)?)
        } else {
            self.expect_keyword(Keyword::Else)?;
            Box::new(self.parse_expression(0)?)
        };
        let end = else_expression.span.end;

        Ok(Expression {
            span: Span { start, end },
            kind: ExpressionKind::IfElse {
                condition: Box::new(condition),
                then_expression: Box::new(then_expression),
                else_expression,
            },
        })
    }

    fn parse_function_expression(&mut self) -> Result<Function, ParseError> {
        let start = self.expect_keyword(Keyword::Function)?.span.start;
        if self.current().kind == TokenKind::Name && self.lookahead(1).kind == TokenKind::Byte(b'(')
        {
            self.take();
        }
        self.parse_function_body(start)
    }

    fn parse_interpolated_string(&mut self) -> Result<Expression, ParseError> {
        let start = self.take().span.start;
        let mut expressions = Vec::new();

        loop {
            if self.current().kind == TokenKind::Interpolated(InterpolatedKind::End) {
                let end = self.take().span.end;
                return Ok(Expression {
                    span: Span { start, end },
                    kind: ExpressionKind::Interpolated(expressions),
                });
            }
            if self.current().kind == TokenKind::Interpolated(InterpolatedKind::Middle) {
                self.take();
            }
            expressions.push(self.parse_expression(0)?);
            if !matches!(
                self.current().kind,
                TokenKind::Interpolated(InterpolatedKind::Middle | InterpolatedKind::End)
            ) {
                return Err(ParseError::expected(
                    self.current().span,
                    "interpolation delimiter",
                    self.current().kind,
                ));
            }
        }
    }

    fn parse_table_expression(&mut self) -> Result<Expression, ParseError> {
        let open = self.expect_byte(b'{', "table constructor")?;
        let mut fields = Vec::new();

        while !self.at_byte(b'}') {
            if self.current().kind == TokenKind::Eof {
                return Err(ParseError::expected(
                    self.current().span,
                    "closing brace",
                    self.current().kind,
                ));
            }

            let field_start = self.current().span.start;

            let key = if self.at_byte(b'[') {
                self.take();
                let expression = self.parse_expression(0)?;
                self.expect_byte(b']', "closing bracket")?;
                self.expect_byte(b'=', "table field assignment")?;
                Some(TableKey::Expression(Box::new(expression)))
            } else if self.current().kind == TokenKind::Name
                && self.lookahead(1).kind == TokenKind::Byte(b'=')
            {
                let name = self.take().span;
                self.expect_byte(b'=', "table field assignment")?;
                Some(TableKey::Name(name))
            } else {
                None
            };

            let value = self.parse_expression(0)?;
            fields.push(TableField {
                span: Span {
                    start: field_start,
                    end: value.span.end,
                },
                key,
                value,
            });

            if self.consume_byte(b',').is_none() && self.consume_byte(b';').is_none() {
                break;
            }
        }

        let close = self.expect_byte(b'}', "closing brace")?;
        Ok(Expression {
            span: Span {
                start: open.span.start,
                end: close.span.end,
            },
            kind: ExpressionKind::Table(fields),
        })
    }

    fn parse_postfix_expressions(&mut self, expression: &mut Expression) -> Result<(), ParseError> {
        loop {
            match self.current().kind {
                TokenKind::Byte(b'.') => {
                    self.take();
                    let name = self.expect_name("field name")?;
                    let object = expression.clone();
                    expression.span.end = name.span.end;
                    expression.kind = ExpressionKind::Field {
                        object: Box::new(object),
                        name: name.span,
                    };
                }
                TokenKind::Byte(b'[') => {
                    self.take();
                    let index = self.parse_expression(0)?;
                    let close = self.expect_byte(b']', "closing bracket")?;
                    let object = expression.clone();
                    expression.span.end = close.span.end;
                    expression.kind = ExpressionKind::Index {
                        object: Box::new(object),
                        index: Box::new(index),
                    };
                }
                TokenKind::Byte(b':') => self.parse_method_call(expression)?,
                TokenKind::Byte(b'<') => {
                    let Some((type_arguments, type_arguments_span)) =
                        self.try_parse_explicit_type_arguments()?
                    else {
                        break;
                    };
                    if !self.at_byte(b'(') {
                        break;
                    }
                    let arguments = self.parse_call_arguments()?;
                    self.finish_call(
                        expression,
                        None,
                        type_arguments,
                        Some(type_arguments_span),
                        arguments,
                    );
                }
                TokenKind::Byte(b'(') => {
                    let arguments = self.parse_call_arguments()?;
                    self.finish_call(expression, None, Vec::new(), None, arguments);
                }
                TokenKind::QuotedString
                | TokenKind::RawString
                | TokenKind::Interpolated(InterpolatedKind::Simple | InterpolatedKind::Begin)
                | TokenKind::Byte(b'{') => {
                    let argument = self.parse_prefix_expression()?;
                    self.finish_call(expression, None, Vec::new(), None, vec![argument]);
                }
                _ => break,
            }
        }
        Ok(())
    }

    fn parse_method_call(&mut self, expression: &mut Expression) -> Result<(), ParseError> {
        self.take();
        let method = self.expect_name("method name")?;
        let (type_arguments, type_arguments_span) = if self.at_byte(b'<') {
            let (arguments, span) = self.parse_explicit_type_arguments()?;
            (arguments, Some(span))
        } else {
            (Vec::new(), None)
        };
        let arguments = if self.at_byte(b'(') {
            self.parse_call_arguments()?
        } else if self.is_call_argument_start() {
            vec![self.parse_prefix_expression()?]
        } else {
            return Err(ParseError::expected(
                self.current().span,
                "call arguments",
                self.current().kind,
            ));
        };
        self.finish_call(
            expression,
            Some(method.span),
            type_arguments,
            type_arguments_span,
            arguments,
        );
        Ok(())
    }

    fn finish_call(
        &mut self,
        expression: &mut Expression,
        method: Option<Span>,
        type_arguments: Vec<TypeArgument>,
        type_arguments_span: Option<Span>,
        arguments: Vec<Expression>,
    ) {
        let end = self.previous_end();
        let function = expression.clone();
        expression.span.end = end;
        expression.kind = ExpressionKind::Call {
            function: Box::new(function),
            method,
            type_arguments,
            type_arguments_span,
            arguments,
        };
    }

    // Token navigation and grammar helpers

    fn is_call_argument_start(&mut self) -> bool {
        matches!(
            self.current().kind,
            TokenKind::QuotedString
                | TokenKind::RawString
                | TokenKind::Interpolated(InterpolatedKind::Simple | InterpolatedKind::Begin)
                | TokenKind::Byte(b'{')
        )
    }

    fn parse_call_arguments(&mut self) -> Result<Vec<Expression>, ParseError> {
        self.expect_byte(b'(', "call arguments")?;
        if self.at_byte(b')') {
            self.take();
            return Ok(Vec::new());
        }
        let arguments = self.parse_expression_list()?;
        self.expect_byte(b')', "closing parenthesis")?;
        Ok(arguments)
    }

    fn binary_operator(&mut self) -> Option<(BinaryOperator, u8, u8)> {
        let operator = match self.current().kind {
            TokenKind::Keyword(Keyword::Or) => BinaryOperator::Or,
            TokenKind::Keyword(Keyword::And) => BinaryOperator::And,

            TokenKind::Operator(Operator::LessEqual) => BinaryOperator::LessEqual,
            TokenKind::Operator(Operator::GreaterEqual) => BinaryOperator::GreaterEqual,
            TokenKind::Operator(Operator::Equal) => BinaryOperator::Equal,
            TokenKind::Operator(Operator::NotEqual) => BinaryOperator::NotEqual,

            TokenKind::Operator(Operator::Concat) => BinaryOperator::Concat,
            TokenKind::Operator(Operator::FloorDivide) => BinaryOperator::FloorDivide,

            TokenKind::Byte(b'<') => {
                if self.lookahead_is_adjacent_byte(1, b'<') {
                    BinaryOperator::ShiftLeft
                } else {
                    BinaryOperator::Less
                }
            }
            TokenKind::Byte(b'>') => {
                if self.lookahead_is_adjacent_byte(1, b'>') {
                    BinaryOperator::ShiftRight
                } else {
                    BinaryOperator::Greater
                }
            }

            TokenKind::Byte(b'|') => BinaryOperator::BitOr,
            TokenKind::Byte(b'~') => BinaryOperator::BitXor,
            TokenKind::Byte(b'&') => BinaryOperator::BitAnd,

            TokenKind::Byte(b'+') => BinaryOperator::Add,
            TokenKind::Byte(b'-') => BinaryOperator::Subtract,
            TokenKind::Byte(b'*') => BinaryOperator::Multiply,
            TokenKind::Byte(b'/') => BinaryOperator::Divide,
            TokenKind::Byte(b'%') => BinaryOperator::Modulo,
            TokenKind::Byte(b'^') => BinaryOperator::Power,
            _ => return None,
        };
        let (left, right) = operator.binding_power();

        Some((operator, left, right))
    }

    fn consume_binary_operator(&mut self, operator: BinaryOperator) {
        self.take();
        if matches!(
            operator,
            BinaryOperator::ShiftLeft | BinaryOperator::ShiftRight
        ) {
            self.take();
        }
    }

    fn parse_attributes(&mut self) -> Result<Vec<Attribute>, ParseError> {
        let mut attributes = Vec::new();

        while matches!(
            self.current().kind,
            TokenKind::Attribute | TokenKind::AttributeOpen
        ) {
            let start = self.take();
            let (name, arguments, end) = if start.kind == TokenKind::Attribute {
                if self.at_byte(b'(') {
                    let arguments = self.parse_call_arguments()?;
                    let end = self.previous_end().max(start.span.end);
                    (Some(start.span), arguments, end)
                } else {
                    (Some(start.span), Vec::new(), start.span.end)
                }
            } else {
                let mut square_depth = 1usize;
                let mut end = start.span.end;
                while square_depth > 0 {
                    let token = self.take();
                    if token.kind == TokenKind::Eof {
                        return Err(ParseError::expected(
                            token.span,
                            "closing attribute bracket",
                            token.kind,
                        ));
                    }
                    end = token.span.end;
                    if token.bytes(self.source) == b"[" {
                        square_depth += 1;
                    } else if token.bytes(self.source) == b"]" {
                        square_depth -= 1;
                    }
                }
                (None, Vec::new(), end)
            };
            attributes.push(Attribute {
                span: Span {
                    start: start.span.start,
                    end,
                },
                name,
                arguments,
            });
        }

        Ok(attributes)
    }

    fn compound_assignment_operator(&mut self) -> Option<Operator> {
        match self.current().kind {
            TokenKind::Operator(
                operator @ (Operator::AddAssign
                | Operator::SubtractAssign
                | Operator::MultiplyAssign
                | Operator::DivideAssign
                | Operator::FloorDivideAssign
                | Operator::ModuloAssign
                | Operator::PowerAssign
                | Operator::ConcatAssign),
            ) => Some(operator),
            _ => None,
        }
    }

    fn is_statement_end(&mut self) -> bool {
        matches!(
            self.current().kind,
            TokenKind::Eof
                | TokenKind::Byte(b';')
                | TokenKind::Keyword(
                    Keyword::End | Keyword::Else | Keyword::ElseIf | Keyword::Until
                )
        )
    }

    fn is_name(&mut self, name: &[u8]) -> bool {
        let token = self.current();
        token.kind == TokenKind::Name && token.bytes(self.source) == name
    }

    fn lookahead_is_name(&mut self, offset: usize, name: &[u8]) -> bool {
        let token = self.lookahead(offset);
        token.kind == TokenKind::Name && token.bytes(self.source) == name
    }

    fn is_export_start(&mut self) -> bool {
        let token = self.lookahead(1);
        match token.kind {
            TokenKind::Keyword(Keyword::Local | Keyword::Function) => true,
            TokenKind::Name => matches!(
                token.bytes(self.source).as_bytes(),
                b"type" | b"class" | b"open" | b"const"
            ),
            _ => false,
        }
    }

    fn is_bare_continue(&mut self) -> bool {
        matches!(
            self.lookahead(1).kind,
            TokenKind::Eof | TokenKind::Name | TokenKind::Keyword(_) | TokenKind::Byte(b';')
        )
    }

    fn current(&mut self) -> Token {
        self.skip_trivia();
        self.tokens
            .get(self.cursor)
            .copied()
            .or_else(|| self.tokens.last().copied())
            .expect("lexer always emits eof")
    }

    fn lookahead(&mut self, offset: usize) -> Token {
        self.skip_trivia();
        let mut index = self.cursor;
        let mut remaining = offset;
        loop {
            let token = self
                .tokens
                .get(index)
                .copied()
                .or_else(|| self.tokens.last().copied())
                .expect("lexer always emits eof");
            if !is_trivia(token.kind) {
                if remaining == 0 {
                    return token;
                }
                remaining -= 1;
            }
            index += 1;
        }
    }

    fn lookahead_is_adjacent_byte(&mut self, offset: usize, byte: u8) -> bool {
        let current = self.current();
        let next = self.lookahead(offset);
        next.kind == TokenKind::Byte(byte) && current.span.end == next.span.start
    }

    fn skip_trivia(&mut self) {
        while self
            .tokens
            .get(self.cursor)
            .is_some_and(|token| is_trivia(token.kind))
        {
            self.cursor += 1;
        }
    }

    fn take(&mut self) -> Token {
        let token = self.current();
        if token.kind != TokenKind::Eof {
            self.cursor += 1;
        }
        token
    }

    fn expect_kind(
        &mut self,
        kind: TokenKind,
        expected: &'static str,
    ) -> Result<Token, ParseError> {
        let token = self.current();
        if token.kind == kind {
            Ok(self.take())
        } else if matches!(token.kind, TokenKind::Error(_)) {
            Err(ParseError::unexpected(token))
        } else {
            Err(ParseError::expected(token.span, expected, token.kind))
        }
    }

    fn expect_keyword(&mut self, keyword: Keyword) -> Result<Token, ParseError> {
        self.expect_kind(TokenKind::Keyword(keyword), "keyword")
    }

    fn expect_operator(
        &mut self,
        operator: Operator,
        expected: &'static str,
    ) -> Result<Token, ParseError> {
        self.expect_kind(TokenKind::Operator(operator), expected)
    }

    fn expect_name(&mut self, expected: &'static str) -> Result<Token, ParseError> {
        let token = self.current();
        if token.kind == TokenKind::Name {
            Ok(self.take())
        } else if matches!(token.kind, TokenKind::Error(_)) {
            Err(ParseError::unexpected(token))
        } else {
            Err(ParseError::expected(token.span, expected, token.kind))
        }
    }

    fn expect_byte(&mut self, byte: u8, expected: &'static str) -> Result<Token, ParseError> {
        self.expect_kind(TokenKind::Byte(byte), expected)
    }

    fn consume_keyword(&mut self, keyword: Keyword) -> Option<Token> {
        (self.current().kind == TokenKind::Keyword(keyword)).then(|| self.take())
    }

    fn consume_operator(&mut self, operator: Operator) -> Option<Token> {
        (self.current().kind == TokenKind::Operator(operator)).then(|| self.take())
    }

    fn consume_byte(&mut self, byte: u8) -> Option<Token> {
        (self.current().kind == TokenKind::Byte(byte)).then(|| self.take())
    }

    fn at_byte(&mut self, byte: u8) -> bool {
        self.current().kind == TokenKind::Byte(byte)
    }

    fn at_keyword(&mut self, keyword: Keyword) -> bool {
        self.current().kind == TokenKind::Keyword(keyword)
    }

    fn previous_end(&self) -> usize {
        self.tokens
            .get(self.cursor.saturating_sub(1))
            .map_or(0, |token| token.span.end)
    }
}

/// # Errors
///
/// Returns the first syntax or lexical error encountered in the source.
pub fn parse(source: &BStr) -> Result<Chunk, ParseError> {
    Parser::new(source).parse()
}

/// # Errors
///
/// Returns the first syntax or lexical error encountered in the expression.
pub fn parse_expression(source: &BStr) -> Result<Expression, ParseError> {
    let mut parser = Parser::new(source);
    let expression = parser.parse_expression(0)?;
    parser.expect_kind(TokenKind::Eof, "end of input")?;
    Ok(expression)
}

/// # Errors
///
/// Returns the first syntax or lexical error encountered in the type.
pub fn parse_type(source: &BStr) -> Result<TypeExpression, ParseError> {
    let mut parser = Parser::new(source);
    let ty = parser.parse_type()?;
    parser.expect_kind(TokenKind::Eof, "end of input")?;
    Ok(ty)
}

fn is_trivia(kind: TokenKind) -> bool {
    matches!(
        kind,
        TokenKind::Whitespace | TokenKind::Comment | TokenKind::BlockComment
    )
}

fn is_assignable(expression: &Expression) -> bool {
    matches!(
        expression.kind,
        ExpressionKind::Name | ExpressionKind::Index { .. } | ExpressionKind::Field { .. }
    )
}

fn extend_statement_span(statement: &mut Statement, end: usize) {
    match statement {
        Statement::Empty { span }
        | Statement::Local { span, .. }
        | Statement::LocalFunction { span, .. }
        | Statement::Assignment { span, .. }
        | Statement::CompoundAssignment { span, .. }
        | Statement::Call { span, .. }
        | Statement::Return { span, .. }
        | Statement::Break { span }
        | Statement::Continue { span }
        | Statement::Do { span, .. }
        | Statement::If { span, .. }
        | Statement::While { span, .. }
        | Statement::Repeat { span, .. }
        | Statement::NumericFor { span, .. }
        | Statement::GenericFor { span, .. }
        | Statement::Function { span, .. }
        | Statement::TypeAlias { span, .. }
        | Statement::TypeFunction { span, .. }
        | Statement::DeclareGlobal { span, .. }
        | Statement::DeclareFunction { span, .. }
        | Statement::Class { span, .. }
        | Statement::Export { span, .. } => span.end = end,
    }
}

fn statement_span(statement: &Statement) -> Span {
    match statement {
        Statement::Empty { span }
        | Statement::Local { span, .. }
        | Statement::LocalFunction { span, .. }
        | Statement::Assignment { span, .. }
        | Statement::CompoundAssignment { span, .. }
        | Statement::Call { span, .. }
        | Statement::Return { span, .. }
        | Statement::Break { span }
        | Statement::Continue { span }
        | Statement::Do { span, .. }
        | Statement::If { span, .. }
        | Statement::While { span, .. }
        | Statement::Repeat { span, .. }
        | Statement::NumericFor { span, .. }
        | Statement::GenericFor { span, .. }
        | Statement::Function { span, .. }
        | Statement::TypeAlias { span, .. }
        | Statement::TypeFunction { span, .. }
        | Statement::DeclareGlobal { span, .. }
        | Statement::DeclareFunction { span, .. }
        | Statement::Class { span, .. }
        | Statement::Export { span, .. } => *span,
    }
}

fn first_span(expressions: &[Expression]) -> usize {
    expressions
        .first()
        .map_or(0, |expression| expression.span.start)
}
