use bstr::BStr;

use crate::ast::{
    Attribute, BinaryOperator, Binding, Block, Chunk, Expression, ExpressionKind, Function,
    FunctionName, IfBranch, Statement, TableField, TableKey, UnaryOperator,
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
                body.push(self.parse_statement()?);
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
            TokenKind::Keyword(Keyword::Local) => self.parse_local(start, attributes),
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
            TokenKind::Name if self.is_name(b"continue") => {
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
    ) -> Result<Statement, ParseError> {
        self.expect_keyword(Keyword::Local)?;

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

        let bindings = self.parse_bindings()?;

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

    fn parse_if(&mut self, start: usize) -> Result<Statement, ParseError> {
        self.expect_keyword(Keyword::If)?;
        let mut branches = Vec::new();

        loop {
            let condition = self.parse_expression(0)?;

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
        let first = self.parse_binding()?;

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
                from,
                to,
                step,
                body,
            });
        }

        let mut bindings = vec![first];
        while self.consume_byte(b',').is_some() {
            bindings.push(self.parse_binding()?);
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
        self.expect_keyword(Keyword::Return)?;
        let values = if self.is_statement_end() {
            Vec::new()
        } else {
            self.parse_expression_list()?
        };
        let end = values.last().map_or(start, |value| value.span.end);

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
                target: first,
                operator,
                value,
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

    fn parse_bindings(&mut self) -> Result<Vec<Binding>, ParseError> {
        let mut bindings = vec![self.parse_binding()?];
        while self.consume_byte(b',').is_some() {
            bindings.push(self.parse_binding()?);
        }
        Ok(bindings)
    }

    fn parse_binding(&mut self) -> Result<Binding, ParseError> {
        let token = self.expect_name("name")?;
        Ok(Binding {
            span: token.span,
            name: token.span,
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
        self.expect_byte(b'(', "function parameter list")?;
        let mut parameters = Vec::new();
        let mut variadic = false;

        if !self.at_byte(b')') {
            loop {
                if self.consume_operator(Operator::Ellipsis).is_some() {
                    variadic = true;
                    break;
                }
                parameters.push(self.parse_binding()?);
                if self.consume_byte(b',').is_none() {
                    break;
                }
                if self.at_byte(b')') {
                    break;
                }
            }
        }

        let close = self.expect_byte(b')', "closing parenthesis")?;
        let body_start = self.current().span.start;
        let body = self.parse_statement_list(&[TokenKind::Keyword(Keyword::End)], body_start)?;
        let end = self.expect_keyword(Keyword::End)?;

        Ok(Function {
            span: Span {
                start,
                end: end.span.end.max(close.span.end),
            },
            parameters,
            variadic,
            body,
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
        self.parse_postfix_expressions(&mut left)?;

        while let Some((operator, left_binding_power, right_binding_power)) = self.binary_operator()
        {
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
            TokenKind::Byte(b'(') => {
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
            TokenKind::Byte(b'{') => self.parse_table_expression(),

            TokenKind::Keyword(Keyword::Function) => {
                let function = self.parse_function_expression()?;
                let span = function.span;
                Ok(Expression {
                    span,
                    kind: ExpressionKind::Function(function),
                })
            }
            _ => Err(ParseError::unexpected(token)),
        }
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
                Some(TableKey::Expression(expression))
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

                TokenKind::Byte(b':') => {
                    self.take();
                    let method = self.expect_name("method name")?;
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
                    let end = self.previous_end();
                    let function = expression.clone();
                    expression.span.end = end;
                    expression.kind = ExpressionKind::Call {
                        function: Box::new(function),
                        method: Some(method.span),
                        arguments,
                    };
                }

                TokenKind::Byte(b'(') => {
                    let arguments = self.parse_call_arguments()?;
                    let end = self.previous_end();
                    let function = expression.clone();
                    expression.span.end = end;
                    expression.kind = ExpressionKind::Call {
                        function: Box::new(function),
                        method: None,
                        arguments,
                    };
                }

                TokenKind::QuotedString
                | TokenKind::RawString
                | TokenKind::Interpolated(InterpolatedKind::Simple | InterpolatedKind::Begin)
                | TokenKind::Byte(b'{') => {
                    let argument = self.parse_prefix_expression()?;
                    expression.span.end = argument.span.end;
                    expression.kind = ExpressionKind::Call {
                        function: Box::new(expression.clone()),
                        method: None,
                        arguments: vec![argument],
                    };
                }
                _ => break,
            }
        }
        Ok(())
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
            let end = if start.kind == TokenKind::Attribute {
                start.span.end
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
                end
            };
            attributes.push(Attribute {
                span: Span {
                    start: start.span.start,
                    end,
                },
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

fn first_span(expressions: &[Expression]) -> usize {
    expressions
        .first()
        .map_or(0, |expression| expression.span.start)
}
