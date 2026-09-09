use super::{InterpolatedKind, Keyword, Kind, Operator, Parsed, Parser, TokenKind};
use bstr::ByteSlice;

impl Parser<'_> {
    pub(super) fn expression(&mut self, minimum: u8) -> Parsed {
        self.nested(|parser| parser.binary(minimum))
    }

    fn binary(&mut self, minimum: u8) -> Parsed {
        let start = self.current().span.start;

        let mut left = match self.current().kind {
            TokenKind::Byte(b'-' | b'#') | TokenKind::Keyword(Keyword::Not) => {
                let operator = self.leaf(Kind::Operator);
                let operand = self.expression(8)?;
                self.node(Kind::Unary, start, vec![operator, operand])
            }

            TokenKind::Number => {
                if !number(self.current().bytes(self.tree.source).as_bytes()) {
                    return Err(self.error("malformed number"));
                }

                self.leaf(Kind::Number)
            }

            TokenKind::QuotedString | TokenKind::RawString => self.string()?,
            TokenKind::Interpolated(_) => self.interpolation()?,
            TokenKind::Keyword(Keyword::Nil) => self.leaf(Kind::Nil),
            TokenKind::Keyword(Keyword::True | Keyword::False) => self.leaf(Kind::Boolean),
            TokenKind::Operator(Operator::Ellipsis) => self.leaf(Kind::Variadic),

            TokenKind::Keyword(Keyword::Function) => {
                self.take();
                self.function(start, Kind::Function, Vec::new())?
            }

            TokenKind::Attribute | TokenKind::AttributeOpen => {
                let attributes = self.attributes()?;
                self.expect(
                    TokenKind::Keyword(Keyword::Function),
                    "expected function after attributes",
                )?;
                self.function(start, Kind::Function, vec![attributes])?
            }

            TokenKind::Keyword(Keyword::If) => self.conditional_expression()?,
            TokenKind::Byte(b'{') => self.table()?,
            _ => self.primary()?,
        };

        if self.tree.nodes[left].kind != Kind::Unary
            && self.consume(TokenKind::Operator(Operator::DoubleColon))
        {
            let annotation = self.annotation()?;
            left = self.node(Kind::Assertion, start, vec![left, annotation]);
        }

        while let Some((priority, right)) = priority(self.current().kind) {
            if priority < minimum {
                break;
            }

            let operator = self.leaf(Kind::Operator);
            let operand = self.expression(right)?;
            left = self.node(Kind::Binary, start, vec![left, operator, operand]);
        }

        Ok(left)
    }

    pub(super) fn primary(&mut self) -> Parsed {
        let start = self.current().span.start;
        let mut left = if self.consume(TokenKind::Byte(b'(')) {
            let inner = self.expression(0)?;
            self.expect(TokenKind::Byte(b')'), "expected closing expression")?;
            self.node(Kind::Group, start, vec![inner])
        } else {
            self.name()?
        };

        loop {
            left = match self.current().kind {
                TokenKind::Byte(b'.') => {
                    self.take();
                    let name = self.name()?;
                    self.node(Kind::Field, start, vec![left, name])
                }

                TokenKind::Byte(b'[') => {
                    self.take();
                    let index = self.expression(0)?;
                    self.expect(TokenKind::Byte(b']'), "expected closing index")?;
                    self.node(Kind::Index, start, vec![left, index])
                }

                TokenKind::Byte(b':') => {
                    self.take();
                    let method = self.name()?;
                    let mut children = vec![left, method];

                    if self.byte(b'<') && self.next() == TokenKind::Byte(b'<') {
                        self.take();
                        children.push(self.type_arguments()?);
                        self.expect(TokenKind::Byte(b'>'), "expected closing instantiation")?;
                    }

                    children.push(self.arguments()?);
                    self.node(Kind::MethodCall, start, children)
                }

                TokenKind::Byte(b'(' | b'{') | TokenKind::QuotedString | TokenKind::RawString => {
                    let arguments = self.arguments()?;
                    self.node(Kind::Call, start, vec![left, arguments])
                }

                TokenKind::Byte(b'<') if self.next() == TokenKind::Byte(b'<') => {
                    self.take();
                    let arguments = self.type_arguments()?;
                    self.expect(TokenKind::Byte(b'>'), "expected closing instantiation")?;
                    self.node(Kind::Instantiate, start, vec![left, arguments])
                }

                _ => break,
            };
        }

        Ok(left)
    }

    fn arguments(&mut self) -> Parsed {
        let start = self.current().span.start;

        let children = match self.current().kind {
            TokenKind::Byte(b'(') => {
                self.take();
                let arguments = if self.byte(b')') {
                    Vec::new()
                } else {
                    self.expressions()?
                };

                self.expect(TokenKind::Byte(b')'), "expected closing arguments")?;
                arguments
            }

            TokenKind::Byte(b'{') => vec![self.nested(Self::table)?],
            TokenKind::QuotedString | TokenKind::RawString => vec![self.string()?],
            _ => return Err(self.error("expected call arguments")),
        };

        Ok(self.node(Kind::Arguments, start, children))
    }

    fn conditional_expression(&mut self) -> Parsed {
        let start = self.take().span.start;
        let condition = self.expression(0)?;
        self.expect(TokenKind::Keyword(Keyword::Then), "expected then")?;
        let truthy = self.expression(0)?;

        let falsy = if self.keyword(Keyword::ElseIf) {
            self.nested(Self::conditional_expression)?
        } else {
            self.expect(TokenKind::Keyword(Keyword::Else), "expected else")?;
            self.expression(0)?
        };

        Ok(self.node(Kind::Conditional, start, vec![condition, truthy, falsy]))
    }

    fn table(&mut self) -> Parsed {
        let start = self.take().span.start;
        let mut fields = Vec::new();

        while !self.byte(b'}') {
            let begin = self.current().span.start;
            let mut children = Vec::new();

            if self.consume(TokenKind::Byte(b'[')) {
                children.push(self.expression(0)?);
                self.expect(TokenKind::Byte(b']'), "expected closing field key")?;
                self.expect(TokenKind::Byte(b'='), "expected field value")?;
            } else if self.at(TokenKind::Name) && self.next() == TokenKind::Byte(b'=') {
                children.push(self.name()?);
                self.take();
            }

            children.push(self.expression(0)?);
            fields.push(self.node(Kind::TableField, begin, children));

            if !self.consume(TokenKind::Byte(b',')) && !self.consume(TokenKind::Byte(b';')) {
                break;
            }
        }

        self.expect(TokenKind::Byte(b'}'), "expected closing table")?;
        Ok(self.node(Kind::Table, start, fields))
    }

    pub(super) fn string(&mut self) -> Parsed {
        if self.at(TokenKind::QuotedString)
            && !escapes(self.current().bytes(self.tree.source).as_bytes())
        {
            return Err(self.error("malformed string escape"));
        }

        Ok(self.leaf(Kind::String))
    }

    fn interpolation(&mut self) -> Parsed {
        let start = self.current().span.start;
        let mut children = Vec::new();

        loop {
            let token = self.current();
            let final_segment = matches!(
                token.kind,
                TokenKind::Interpolated(InterpolatedKind::Simple | InterpolatedKind::End)
            );

            if !matches!(token.kind, TokenKind::Interpolated(_)) {
                return Err(self.error("expected interpolation segment"));
            }

            if !escapes(token.bytes(self.tree.source).as_bytes()) {
                return Err(self.error("malformed interpolation escape"));
            }

            children.push(self.leaf(Kind::String));

            if final_segment {
                break;
            }

            children.push(self.expression(0)?);
        }

        Ok(self.node(Kind::Interpolation, start, children))
    }

    pub(super) fn attributes(&mut self) -> Parsed {
        let start = self.current().span.start;
        let mut attributes = Vec::new();

        while matches!(
            self.current().kind,
            TokenKind::Attribute | TokenKind::AttributeOpen
        ) {
            if self.at(TokenKind::Attribute) {
                attributes.push(self.leaf(Kind::Attribute));
            } else {
                self.take();

                loop {
                    let begin = self.current().span.start;
                    let mut children = vec![self.name()?];

                    if self.byte(b'(') {
                        children.push(self.arguments()?);
                    }

                    attributes.push(self.node(Kind::Attribute, begin, children));

                    if !self.consume(TokenKind::Byte(b',')) {
                        break;
                    }
                }

                self.expect(TokenKind::Byte(b']'), "expected closing attributes")?;
            }
        }

        Ok(self.node(Kind::Attributes, start, attributes))
    }
}

fn priority(token: TokenKind) -> Option<(u8, u8)> {
    Some(match token {
        TokenKind::Keyword(Keyword::Or) => (1, 2),
        TokenKind::Keyword(Keyword::And) => (2, 3),
        TokenKind::Byte(b'<' | b'>')
        | TokenKind::Operator(
            Operator::Equal | Operator::NotEqual | Operator::LessEqual | Operator::GreaterEqual,
        ) => (3, 4),
        TokenKind::Operator(Operator::Concat) => (5, 5),
        TokenKind::Byte(b'+' | b'-') => (6, 7),
        TokenKind::Byte(b'*' | b'/' | b'%') | TokenKind::Operator(Operator::FloorDivide) => (7, 8),
        TokenKind::Byte(b'^') => (10, 10),
        _ => return None,
    })
}

fn number(bytes: &[u8]) -> bool {
    let normalized: Vec<_> = bytes.iter().copied().filter(|byte| *byte != b'_').collect();
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

fn escapes(bytes: &[u8]) -> bool {
    let mut cursor = 1;

    while cursor + 1 < bytes.len() {
        if bytes[cursor] != b'\\' {
            cursor += 1;
            continue;
        }

        cursor += 1;

        match bytes[cursor] {
            b'x' => {
                if !bytes
                    .get(cursor + 1..cursor + 3)
                    .is_some_and(|digits| digits.iter().all(u8::is_ascii_hexdigit))
                {
                    return false;
                }

                cursor += 3;
            }

            b'u' => {
                cursor += 1;

                if bytes.get(cursor) != Some(&b'{') {
                    return false;
                }

                cursor += 1;
                let begin = cursor;

                while bytes.get(cursor).is_some_and(u8::is_ascii_hexdigit) {
                    cursor += 1;
                }

                if begin == cursor || bytes.get(cursor) != Some(&b'}') {
                    return false;
                }

                let Ok(text) = std::str::from_utf8(&bytes[begin..cursor]) else {
                    return false;
                };

                if !u32::from_str_radix(text, 16).is_ok_and(|value| value < 0x0011_0000) {
                    return false;
                }

                cursor += 1;
            }

            b'0'..=b'9' => {
                let mut value = 0u16;

                for _ in 0..3 {
                    if !bytes.get(cursor).is_some_and(u8::is_ascii_digit) {
                        break;
                    }

                    value = value * 10 + u16::from(bytes[cursor] - b'0');
                    cursor += 1;
                }

                if value > 255 {
                    return false;
                }
            }

            _ => cursor += 1,
        }
    }

    true
}
