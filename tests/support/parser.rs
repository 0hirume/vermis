use bstr::BStr;
use vermis::{
    Attribute, BinaryOperator, Block, ClassMemberKind, Expression, ExpressionKind, Function,
    FunctionName, GenericParameter, IfCondition, Operator, Span, Statement, TableKey, TypeArgument,
    TypeExpression, TypeExpressionKind, TypePack, TypePackTail, TypeParameter, UnaryOperator,
};

use super::oracle::{Oracle, ParseMode, ParseOutput};

pub const CHUNK: u8 = 1;
pub const BLOCK: u8 = 2;
pub const LOCAL: u8 = 3;
pub const LOCAL_FUNCTION: u8 = 4;
pub const ASSIGNMENT: u8 = 5;
pub const COMPOUND_ASSIGNMENT: u8 = 6;
pub const CALL_STATEMENT: u8 = 7;
pub const RETURN: u8 = 8;
pub const BREAK: u8 = 9;
pub const CONTINUE: u8 = 10;
pub const DO: u8 = 11;
pub const IF: u8 = 12;
pub const WHILE: u8 = 13;
pub const REPEAT: u8 = 14;
pub const NUMERIC_FOR: u8 = 15;
pub const GENERIC_FOR: u8 = 16;
pub const FUNCTION_STATEMENT: u8 = 17;
pub const TYPE_ALIAS: u8 = 18;
pub const TYPE_FUNCTION_STATEMENT: u8 = 19;
pub const DECLARE_GLOBAL: u8 = 20;
pub const DECLARE_FUNCTION: u8 = 21;
pub const DECLARE_EXTERN_TYPE: u8 = 22;
pub const CLASS: u8 = 23;
pub const EXPORT: u8 = 24;
pub const BINDING: u8 = 25;
pub const FUNCTION_NAME: u8 = 26;
pub const CLASS_MEMBER: u8 = 27;
pub const TYPE_PARAMETER: u8 = 28;
pub const GENERIC_PARAMETER: u8 = 29;
pub const ATTRIBUTE: u8 = 30;
pub const NIL: u8 = 31;
pub const BOOLEAN: u8 = 32;
pub const NUMBER: u8 = 33;
pub const STRING: u8 = 34;
pub const NAME: u8 = 35;
pub const VARARG: u8 = 36;
pub const UNARY: u8 = 37;
pub const BINARY: u8 = 38;
pub const GROUP: u8 = 39;
pub const IF_ELSE: u8 = 40;
pub const TYPE_ASSERTION: u8 = 41;
pub const INTERPOLATED: u8 = 42;
pub const TABLE: u8 = 43;
pub const TABLE_FIELD: u8 = 44;
pub const TABLE_KEY_NAME: u8 = 45;
pub const TABLE_KEY_EXPRESSION: u8 = 46;
pub const CALL: u8 = 47;
pub const INDEX: u8 = 48;
pub const FIELD: u8 = 49;
pub const FUNCTION: u8 = 50;
pub const INSTANTIATE: u8 = 51;
pub const TYPE_NAME: u8 = 52;
pub const TYPE_NIL: u8 = 53;
pub const TYPE_TABLE: u8 = 54;
pub const TYPE_FUNCTION: u8 = 55;
pub const TYPEOF: u8 = 56;
pub const TYPE_OPTIONAL: u8 = 57;
pub const TYPE_UNION: u8 = 58;
pub const TYPE_INTERSECTION: u8 = 59;
pub const TYPE_BOOLEAN: u8 = 60;
pub const TYPE_STRING: u8 = 61;
pub const TYPE_NUMBER: u8 = 62;
pub const TYPE_GROUP: u8 = 63;
pub const TYPE_FIELD: u8 = 64;
pub const TYPE_INDEXER: u8 = 65;
pub const TYPE_PACK: u8 = 66;
pub const TYPE_PACK_VARIADIC: u8 = 67;
pub const TYPE_PACK_GENERIC: u8 = 68;

const FLAG_CONST: u8 = 1;
const FLAG_EXPORTED: u8 = 2;
const FLAG_OPEN: u8 = 4;
const FLAG_METHOD: u8 = 8;
const FLAG_SELF: u8 = 16;
const FLAG_VARARG: u8 = 32;
const FLAG_HAS_ANNOTATION: u8 = 64;
const FLAG_NAMED: u8 = 128;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Event {
    pub tag: u8,
    pub flags: u8,
    pub start: usize,
    pub end: usize,
    pub value: u32,
}

pub fn compare_chunk(oracle: &mut Oracle, source: &[u8]) -> Result<(), String> {
    compare(oracle, ParseMode::Chunk, source, vermis_chunk(source))
}

pub fn compare_expression(oracle: &mut Oracle, source: &[u8]) -> Result<(), String> {
    compare(
        oracle,
        ParseMode::Expression,
        source,
        vermis_expression(source),
    )
}

pub fn compare_type(oracle: &mut Oracle, source: &[u8]) -> Result<(), String> {
    compare(oracle, ParseMode::Type, source, vermis_type(source))
}

fn compare(
    oracle: &mut Oracle,
    mode: ParseMode,
    source: &[u8],
    vermis: Result<Vec<Event>, Vec<(usize, usize)>>,
) -> Result<(), String> {
    let luau = oracle
        .parse(mode, source)
        .map_err(|error| format!("oracle I/O failed: {error}"))?;

    match (luau.accepted, vermis) {
        (false, Err(errors)) => {
            if luau.errors == errors {
                Ok(())
            } else {
                Err(format!(
                    "diagnostic mismatch for {source:?}\nvermis: {errors:?}\nluau: {:?}",
                    luau.errors
                ))
            }
        }
        (true, Ok(events)) => compare_events(source, &events, &luau),
        (luau_accepted, vermis_result) => Err(format!(
            "acceptance mismatch for {source:?}\nvermis: {}\nluau: {luau_accepted}\nvermis result: {vermis_result:?}",
            vermis_result.is_ok()
        )),
    }
}

fn compare_events(source: &[u8], vermis: &[Event], luau: &ParseOutput) -> Result<(), String> {
    let oracle = luau
        .events
        .iter()
        .map(|event| Event {
            tag: event.tag,
            flags: event.flags,
            start: event.start,
            end: event.end,
            value: event.value,
        })
        .collect::<Vec<_>>();

    if vermis == oracle {
        return Ok(());
    }

    let mismatch = vermis
        .iter()
        .zip(&oracle)
        .position(|(left, right)| left != right)
        .unwrap_or(vermis.len().min(oracle.len()));
    Err(format!(
        "AST mismatch at event {mismatch} for {source:?}\nvermis: {vermis:?}\nluau: {oracle:?}"
    ))
}

fn vermis_chunk(source: &[u8]) -> Result<Vec<Event>, Vec<(usize, usize)>> {
    match vermis::parse(BStr::new(source)) {
        Ok(chunk) => {
            let mut serializer = Serializer::new(source);
            serializer.chunk(
                &chunk.body,
                Span {
                    start: 0,
                    end: source.len(),
                },
            );
            Ok(serializer.events)
        }
        Err(error) => Err(vec![(error.span.start, error.span.end)]),
    }
}

fn vermis_expression(source: &[u8]) -> Result<Vec<Event>, Vec<(usize, usize)>> {
    match vermis::parse_expression(BStr::new(source)) {
        Ok(expression) => {
            let mut serializer = Serializer::new(source);
            serializer.expression(&expression);
            Ok(serializer.events)
        }
        Err(error) => Err(vec![(error.span.start, error.span.end)]),
    }
}

fn vermis_type(source: &[u8]) -> Result<Vec<Event>, Vec<(usize, usize)>> {
    match vermis::parse_type(BStr::new(source)) {
        Ok(ty) => {
            let mut serializer = Serializer::new(source);
            serializer.ty(&ty);
            Ok(serializer.events)
        }
        Err(error) => Err(vec![(error.span.start, error.span.end)]),
    }
}

struct Serializer<'source> {
    source: &'source [u8],
    events: Vec<Event>,
}

impl<'source> Serializer<'source> {
    fn new(source: &'source [u8]) -> Self {
        Self {
            source,
            events: Vec::new(),
        }
    }

    fn event(&mut self, tag: u8, span: Span, flags: u8, value: u32) {
        self.events.push(Event {
            tag,
            flags,
            start: span.start,
            end: span.end,
            value,
        });
    }

    fn chunk(&mut self, statements: &[Statement], span: Span) {
        self.event(CHUNK, span, 0, 0);
        for statement in statements {
            self.statement(statement);
        }
    }

    fn block(&mut self, block: &Block) {
        self.event(BLOCK, block.span, 0, 0);
        for statement in &block.body {
            self.statement(statement);
        }
    }

    #[allow(clippy::too_many_lines)]
    fn statement(&mut self, statement: &Statement) {
        match statement {
            Statement::Empty { .. } => {}
            Statement::Local {
                span,
                attributes,
                bindings,
                values,
                is_const,
            } => {
                self.event(LOCAL, *span, if *is_const { FLAG_CONST } else { 0 }, 0);
                self.attributes(attributes);
                for binding in bindings {
                    self.binding(binding);
                }
                for value in values {
                    self.expression(value);
                }
            }
            Statement::LocalFunction {
                span,
                attributes,
                name,
                function,
            } => {
                self.event(LOCAL_FUNCTION, *span, 0, 0);
                self.event(BINDING, *name, 0, 0);
                self.function_with_attributes(function, span.start, attributes);
            }
            Statement::Assignment {
                span,
                targets,
                values,
            } => {
                self.event(ASSIGNMENT, *span, 0, 0);
                for target in targets {
                    self.expression(target);
                }
                for value in values {
                    self.expression(value);
                }
            }
            Statement::CompoundAssignment {
                span,
                target,
                operator,
                value,
            } => {
                self.event(COMPOUND_ASSIGNMENT, *span, 0, compound_value(*operator));
                self.expression(target);
                self.expression(value);
            }
            Statement::Call { span, expression } => {
                self.event(CALL_STATEMENT, *span, 0, 0);
                self.expression(expression);
            }
            Statement::Return { span, values } => {
                self.event(RETURN, *span, 0, 0);
                for value in values {
                    self.expression(value);
                }
            }
            Statement::Break { span } => self.event(BREAK, *span, 0, 0),
            Statement::Continue { span } => self.event(CONTINUE, *span, 0, 0),
            Statement::Do { span, body } => {
                self.event(
                    BLOCK,
                    Span {
                        start: if body.body.is_empty() {
                            span.end
                        } else {
                            body.span.start
                        },
                        end: span.end,
                    },
                    0,
                    0,
                );
                for statement in &body.body {
                    self.statement(statement);
                }
            }
            Statement::If {
                span,
                branches,
                else_body,
            } => {
                let flags = branches.first().is_some_and(|branch| {
                    matches!(&branch.condition, IfCondition::Local { binding, .. } if binding.is_const)
                });
                self.event(IF, *span, if flags { FLAG_CONST } else { 0 }, 0);
                for branch in branches {
                    match &branch.condition {
                        IfCondition::Expression(expression) => self.expression(expression),
                        IfCondition::Local { binding, value } => {
                            self.binding(binding);
                            self.expression(value);
                        }
                    }
                    self.block(&branch.body);
                }
                if let Some(body) = else_body {
                    self.block(body);
                }
            }
            Statement::While {
                span,
                condition,
                body,
            } => {
                self.event(WHILE, *span, 0, 0);
                self.expression(condition);
                self.block(body);
            }
            Statement::Repeat {
                span,
                body,
                condition,
            } => {
                self.event(REPEAT, *span, 0, 0);
                self.block(body);
                self.expression(condition);
            }
            Statement::NumericFor {
                span,
                binding,
                from,
                to,
                step,
                body,
            } => {
                self.event(NUMERIC_FOR, *span, 0, 0);
                self.binding(binding);
                self.expression(from);
                self.expression(to);
                if let Some(step) = step {
                    self.expression(step);
                }
                self.block(body);
            }
            Statement::GenericFor {
                span,
                bindings,
                values,
                body,
            } => {
                self.event(GENERIC_FOR, *span, 0, 0);
                for binding in bindings {
                    self.binding(binding);
                }
                for value in values {
                    self.expression(value);
                }
                self.block(body);
            }
            Statement::Function {
                span,
                attributes,
                name,
                function,
            } => {
                self.event(FUNCTION_STATEMENT, *span, 0, 0);
                self.function_name(name);
                self.function_with_attributes(function, span.start, attributes);
            }
            Statement::TypeAlias {
                span,
                exported,
                name,
                generics,
                value,
            } => {
                self.event(
                    TYPE_ALIAS,
                    *span,
                    if *exported { FLAG_EXPORTED } else { 0 },
                    0,
                );
                self.event(NAME, *name, 0, 0);
                self.generics(generics);
                self.ty(value);
            }
            Statement::TypeFunction {
                span,
                exported,
                name,
                function,
            } => {
                self.event(
                    TYPE_FUNCTION_STATEMENT,
                    *span,
                    if *exported { FLAG_EXPORTED } else { 0 },
                    0,
                );
                self.event(NAME, *name, 0, 0);
                self.function(function, span.start);
            }
            Statement::DeclareGlobal {
                span,
                name,
                annotation,
            } => {
                self.event(DECLARE_GLOBAL, *span, 0, 0);
                self.event(NAME, *name, 0, 0);
                self.ty(annotation);
            }
            Statement::DeclareFunction {
                span,
                name,
                signature,
            } => {
                self.event(DECLARE_FUNCTION, *span, 0, 0);
                self.event(NAME, *name, 0, 0);
                self.generics(&signature.generics);
                for parameter in &signature.parameters {
                    self.type_parameter(parameter);
                }
                if let Some(variadic) = &signature.variadic {
                    self.event(TYPE_PACK_VARIADIC, variadic.span, 0, 0);
                    self.ty(variadic);
                }
                self.pack(&signature.returns);
            }
            Statement::Class {
                span,
                exported,
                open,
                name,
                superclass,
                members,
            } => {
                let mut flags = 0;
                if *exported {
                    flags |= FLAG_EXPORTED;
                }
                if *open {
                    flags |= FLAG_OPEN;
                }
                self.event(CLASS, *span, flags, 0);
                self.event(BINDING, *name, 0, 0);
                if let Some(superclass) = superclass {
                    self.class_superclass(superclass);
                }
                for member in members {
                    let flags = match member.kind {
                        ClassMemberKind::Property { .. } => 0,
                        ClassMemberKind::Method { .. } => FLAG_METHOD,
                    };
                    self.event(CLASS_MEMBER, member.name, flags, 0);
                    self.event(NAME, member.name, flags, 0);
                    match &member.kind {
                        ClassMemberKind::Property { annotation } => {
                            if let Some(annotation) = annotation {
                                self.ty(annotation);
                            }
                        }
                        ClassMemberKind::Method { function } => {
                            self.function(function, member.span.start);
                        }
                    }
                }
            }
            Statement::Export { span, statement } => {
                self.event(EXPORT, *span, 0, 0);
                self.statement(statement);
            }
        }
    }

    fn binding(&mut self, binding: &vermis::Binding) {
        let flags = if binding.is_const { FLAG_CONST } else { 0 };
        if binding.annotation.is_some() {
            self.event(BINDING, binding.name, flags | FLAG_HAS_ANNOTATION, 0);
        } else {
            self.event(BINDING, binding.name, flags, 0);
        }
        if let Some(annotation) = &binding.annotation {
            self.ty(annotation);
        }
    }

    fn function_name(&mut self, name: &FunctionName) {
        self.event(FUNCTION_NAME, name.span, 0, 0);
        for part in &name.parts {
            self.event(NAME, *part, 0, 0);
        }
        if let Some(method) = name.method {
            self.event(NAME, method, FLAG_METHOD, 0);
        }
    }

    fn attributes(&mut self, attributes: &[Attribute]) {
        for attribute in attributes {
            let value = attribute
                .name
                .and_then(|span| self.source.get(span.start..span.end))
                .map_or(4, attribute_value);
            self.event(ATTRIBUTE, attribute.span, 0, value);
            for argument in &attribute.arguments {
                self.expression(argument);
            }
        }
    }

    fn generics(&mut self, generics: &[GenericParameter]) {
        for generic in generics {
            let flags = if generic.is_pack { FLAG_VARARG } else { 0 };
            self.event(GENERIC_PARAMETER, generic.name, flags, 0);
            if let Some(default) = &generic.default {
                self.ty(default);
            }
        }
    }

    fn function(&mut self, function: &Function, start: usize) {
        self.function_with_attributes(function, start, &function.attributes);
    }

    fn function_with_attributes(
        &mut self,
        function: &Function,
        start: usize,
        attributes: &[Attribute],
    ) {
        let mut flags = 0;
        if function.variadic {
            flags |= FLAG_VARARG;
        }
        if function.variadic_type.is_some() {
            flags |= FLAG_HAS_ANNOTATION;
        }
        if function.return_types.is_some() {
            flags |= FLAG_NAMED;
        }
        self.event(
            FUNCTION,
            Span {
                start,
                end: function.span.end,
            },
            flags,
            0,
        );
        self.attributes(attributes);
        self.generics(&function.generics);
        for parameter in &function.parameters {
            self.binding(parameter);
        }
        if let Some(variadic) = &function.variadic_type {
            self.pack(variadic);
        }
        if let Some(returns) = &function.return_types {
            self.pack(returns);
        }
        self.block(&function.body);
    }

    #[allow(clippy::too_many_lines)]
    fn expression(&mut self, expression: &Expression) {
        match &expression.kind {
            ExpressionKind::Nil => self.event(NIL, expression.span, 0, 0),
            ExpressionKind::Boolean(value) => {
                self.event(BOOLEAN, expression.span, u8::from(*value), 0);
            }
            ExpressionKind::Number => self.event(NUMBER, expression.span, 0, 0),
            ExpressionKind::String => self.event(STRING, expression.span, 0, 0),
            ExpressionKind::Interpolated(expressions) => {
                self.event(INTERPOLATED, expression.span, 0, 0);
                for expression in expressions {
                    self.expression(expression);
                }
            }
            ExpressionKind::Name => self.event(NAME, expression.span, 0, 0),
            ExpressionKind::Vararg => self.event(VARARG, expression.span, 0, 0),
            ExpressionKind::Unary { operator, operand } => {
                self.event(UNARY, expression.span, 0, unary_value(*operator));
                self.expression(operand);
            }
            ExpressionKind::Binary {
                operator,
                left,
                right,
            } => {
                self.event(BINARY, expression.span, 0, binary_value(*operator));
                self.expression(left);
                self.expression(right);
            }
            ExpressionKind::Group(inner) => {
                self.event(GROUP, expression.span, 0, 0);
                self.expression(inner);
            }
            ExpressionKind::IfElse {
                condition,
                then_expression,
                else_expression,
            } => {
                self.event(IF_ELSE, expression.span, 0, 0);
                self.expression(condition);
                self.expression(then_expression);
                self.expression(else_expression);
            }
            ExpressionKind::TypeAssertion {
                expression: inner,
                annotation,
            } => {
                self.event(TYPE_ASSERTION, expression.span, 0, 0);
                self.expression(inner);
                self.ty(annotation);
            }
            ExpressionKind::Function(function) => self.function(function, expression.span.start),
            ExpressionKind::Table(fields) => {
                self.event(TABLE, expression.span, 0, 0);
                for field in fields {
                    let start = match &field.key {
                        Some(TableKey::Name(name)) => name.start,
                        Some(TableKey::Expression(key)) => key.span.start,
                        None => field.value.span.start,
                    };
                    self.event(
                        TABLE_FIELD,
                        Span {
                            start,
                            end: field.span.end,
                        },
                        0,
                        0,
                    );
                    match &field.key {
                        None => {}
                        Some(TableKey::Name(name)) => self.event(TABLE_KEY_NAME, *name, 0, 0),
                        Some(TableKey::Expression(key)) => {
                            self.event(TABLE_KEY_EXPRESSION, key.span, 0, 0);
                            self.expression(key);
                        }
                    }
                    self.expression(&field.value);
                }
            }
            ExpressionKind::Call {
                function,
                method,
                type_arguments,
                type_arguments_span,
                arguments,
            } => {
                self.event(
                    CALL,
                    expression.span,
                    if method.is_some() { FLAG_SELF } else { 0 },
                    0,
                );
                if method.is_none() && !type_arguments.is_empty() {
                    let span = type_arguments_span.expect("explicit type arguments have a span");
                    self.event(
                        INSTANTIATE,
                        Span {
                            start: function.span.start,
                            end: span.end,
                        },
                        0,
                        0,
                    );
                }
                self.expression(function);
                if let Some(method) = method {
                    self.event(NAME, *method, FLAG_METHOD, 0);
                }
                for argument in type_arguments {
                    self.type_argument(argument);
                }
                for argument in arguments {
                    self.expression(argument);
                }
            }
            ExpressionKind::Index { object, index } => {
                self.event(INDEX, expression.span, 0, 0);
                self.expression(object);
                self.expression(index);
            }
            ExpressionKind::Field { object, name } => {
                self.event(FIELD, expression.span, 0, 0);
                self.expression(object);
                self.event(NAME, *name, 0, 0);
            }
        }
    }

    fn class_superclass(&mut self, ty: &TypeExpression) {
        if let TypeExpressionKind::Name { path, arguments } = &ty.kind
            && arguments.is_empty()
            && !path.is_empty()
        {
            if path.len() == 1 {
                self.event(NAME, path[0], 0, 0);
            } else {
                self.event(
                    FIELD,
                    Span {
                        start: path[0].start,
                        end: path[path.len() - 1].end,
                    },
                    0,
                    0,
                );
                for part in path {
                    self.event(NAME, *part, 0, 0);
                }
            }
        } else {
            self.ty(ty);
        }
    }

    fn type_argument(&mut self, argument: &TypeArgument) {
        match argument {
            TypeArgument::Type(ty) => self.ty(ty),
            TypeArgument::Pack(pack) => self.pack(pack),
        }
    }

    fn type_parameter(&mut self, parameter: &TypeParameter) {
        self.event(
            TYPE_PARAMETER,
            parameter.span,
            if parameter.name.is_some() {
                FLAG_NAMED
            } else {
                0
            },
            0,
        );
        self.ty(&parameter.annotation);
    }

    fn ty(&mut self, ty: &TypeExpression) {
        match &ty.kind {
            TypeExpressionKind::Name { path, arguments } => {
                self.event(TYPE_NAME, ty.span, 0, 0);
                for part in path {
                    self.event(NAME, *part, 0, 0);
                }
                for argument in arguments {
                    self.type_argument(argument);
                }
            }
            TypeExpressionKind::Nil => self.event(TYPE_NIL, ty.span, 0, 0),
            TypeExpressionKind::Boolean(value) => {
                self.event(TYPE_BOOLEAN, ty.span, u8::from(*value), 0);
            }
            TypeExpressionKind::String => self.event(TYPE_STRING, ty.span, 0, 0),
            TypeExpressionKind::Number => self.event(TYPE_NUMBER, ty.span, 0, 0),
            TypeExpressionKind::Table { fields, indexer } => {
                self.event(TYPE_TABLE, ty.span, 0, 0);
                for field in fields {
                    let span = field.name.unwrap_or(field.span);
                    self.event(TYPE_FIELD, span, u8::from(field.optional), 0);
                    if let Some(name) = field.name {
                        self.event(NAME, name, 0, 0);
                    }
                    if let Some(key) = &field.key {
                        self.ty(key);
                    }
                    self.ty(&field.annotation);
                }
                if let Some(indexer) = indexer {
                    self.event(TYPE_INDEXER, indexer.span, 0, 0);
                    if indexer.implicit {
                        self.event(TYPE_NAME, indexer.index.span, 0, 0);
                        self.event(NAME, indexer.index.span, 0, 0);
                    } else {
                        self.ty(&indexer.index);
                    }
                    self.ty(&indexer.result);
                }
            }
            TypeExpressionKind::Function {
                generics,
                parameters,
                variadic,
                returns,
            } => {
                self.event(TYPE_FUNCTION, ty.span, 0, 0);
                self.generics(generics);
                for parameter in parameters {
                    self.type_parameter(parameter);
                }
                if let Some(variadic) = variadic {
                    self.event(TYPE_PACK_VARIADIC, variadic.span, 0, 0);
                    self.ty(variadic);
                }
                self.pack(returns);
            }
            TypeExpressionKind::Typeof(expression) => {
                self.event(TYPEOF, ty.span, 0, 0);
                self.expression(expression);
            }
            TypeExpressionKind::Optional(inner) => {
                self.event(TYPE_UNION, ty.span, 0, 0);
                self.event(TYPE_OPTIONAL, ty.span, 0, 0);
                self.ty(inner);
            }
            TypeExpressionKind::Union(types) => {
                self.event(TYPE_UNION, ty.span, 0, 0);
                for member in types {
                    self.ty(member);
                }
            }
            TypeExpressionKind::Intersection(types) => {
                self.event(TYPE_INTERSECTION, ty.span, 0, 0);
                for member in types {
                    self.ty(member);
                }
            }
            TypeExpressionKind::Group(inner) => {
                self.event(TYPE_GROUP, ty.span, 0, 0);
                self.ty(inner);
            }
        }
    }

    fn pack(&mut self, pack: &TypePack) {
        if pack.types.is_empty() {
            match &pack.tail {
                Some(TypePackTail::Variadic(ty)) => {
                    self.event(TYPE_PACK_VARIADIC, pack.span, 0, 0);
                    self.ty(ty);
                }
                Some(TypePackTail::Generic(_)) => {
                    self.event(TYPE_PACK_GENERIC, pack.span, 0, 0);
                }
                None => self.event(TYPE_PACK, pack.span, 0, 0),
            }
            return;
        }

        self.event(TYPE_PACK, pack.span, 0, 0);
        for ty in &pack.types {
            self.ty(ty);
        }
        if let Some(tail) = &pack.tail {
            match tail {
                TypePackTail::Variadic(ty) => {
                    self.event(TYPE_PACK_VARIADIC, ty.span, 0, 0);
                    self.ty(ty);
                }
                TypePackTail::Generic(name) => {
                    self.event(TYPE_PACK_GENERIC, *name, 0, 0);
                }
            }
        }
    }
}

fn attribute_value(name: &[u8]) -> u32 {
    match name {
        b"@checked" => 0,
        b"@native" => 1,
        b"@deprecated" => 2,
        _ => 4,
    }
}

fn unary_value(operator: UnaryOperator) -> u32 {
    match operator {
        UnaryOperator::Negate => 0,
        UnaryOperator::Not => 1,
        UnaryOperator::Length => 2,
        UnaryOperator::BitNot => 3,
    }
}

fn compound_value(operator: Operator) -> u32 {
    match operator {
        Operator::AddAssign => 8,
        Operator::SubtractAssign => 9,
        Operator::MultiplyAssign => 10,
        Operator::DivideAssign => 11,
        Operator::FloorDivideAssign => 12,
        Operator::ModuloAssign => 13,
        Operator::PowerAssign => 14,
        Operator::ConcatAssign => 15,
        _ => 0,
    }
}

fn binary_value(operator: BinaryOperator) -> u32 {
    match operator {
        BinaryOperator::Or
        | BinaryOperator::BitOr
        | BinaryOperator::BitXor
        | BinaryOperator::BitAnd
        | BinaryOperator::ShiftLeft
        | BinaryOperator::ShiftRight => 0,
        BinaryOperator::And => 1,
        BinaryOperator::Less => 2,
        BinaryOperator::LessEqual => 3,
        BinaryOperator::Greater => 4,
        BinaryOperator::GreaterEqual => 5,
        BinaryOperator::Equal => 6,
        BinaryOperator::NotEqual => 7,
        BinaryOperator::Add => 8,
        BinaryOperator::Subtract => 9,
        BinaryOperator::Multiply => 10,
        BinaryOperator::Divide => 11,
        BinaryOperator::FloorDivide => 12,
        BinaryOperator::Modulo => 13,
        BinaryOperator::Power => 14,
        BinaryOperator::Concat => 15,
    }
}
