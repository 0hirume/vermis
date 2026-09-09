use bstr::BStr;
use vermis::{
    BinaryOperator, ExpressionKind, ParseErrorKind, Statement, TokenKind, UnaryOperator, parse,
    parse_expression, parse_type,
};

fn parse_source(source: &[u8]) -> vermis::Chunk {
    parse(BStr::new(source)).unwrap_or_else(|error| panic!("parse failed: {error:?}"))
}

#[test]
fn parses_expression_precedence_and_unary_operators() {
    let chunk = parse_source(b"local value = -a ^ 2 + #b * ~c and not d or e");
    let Statement::Local { values, .. } = &chunk.body[0] else {
        panic!("expected local statement");
    };
    let expression = &values[0];

    let ExpressionKind::Binary {
        operator: BinaryOperator::Or,
        left,
        right,
    } = &expression.kind
    else {
        panic!("expected or expression");
    };
    assert!(matches!(right.kind, ExpressionKind::Name));

    let ExpressionKind::Binary {
        operator: BinaryOperator::And,
        left: and_left,
        right: and_right,
    } = &left.kind
    else {
        panic!("expected and expression");
    };
    assert!(matches!(
        and_right.kind,
        ExpressionKind::Unary {
            operator: UnaryOperator::Not,
            ..
        }
    ));
    assert!(matches!(
        and_left.kind,
        ExpressionKind::Binary {
            operator: BinaryOperator::Add,
            ..
        }
    ));
}

#[test]
fn parses_calls_fields_indexes_and_table_constructors() {
    let source =
        b"local value = object.field[index](1, 2):method {name = 'value', [key] = value, true}";
    let chunk = parse_source(source);
    let Statement::Local { values, .. } = &chunk.body[0] else {
        panic!("expected local statement");
    };

    let ExpressionKind::Call {
        method: Some(method),
        arguments,
        ..
    } = &values[0].kind
    else {
        panic!("expected method call");
    };
    assert_eq!(method.bytes(BStr::new(source)), b"method");
    assert_eq!(arguments.len(), 1);
    let ExpressionKind::Table(fields) = &arguments[0].kind else {
        panic!("expected table argument");
    };
    assert_eq!(fields.len(), 3);
}

#[test]
fn parses_control_flow_and_loops() {
    let chunk = parse_source(
        br"
            if ready then
                while running do
                    continue
                end
            elseif retry then
                repeat
                    break
                until done
            else
                do
                    return 1, 2
                end
            end
            for i = 1, 10, 2 do print(i) end
            for key, value in pairs(items) do value = key end
        ",
    );

    assert!(matches!(chunk.body[0], Statement::If { .. }));
    assert!(matches!(chunk.body[1], Statement::NumericFor { .. }));
    assert!(matches!(chunk.body[2], Statement::GenericFor { .. }));
}

#[test]
fn parses_functions_and_attributes() {
    let source = b"@[native] function module.create:value(first, second, ...) \
        local result = first return result end local function helper() end";
    let chunk = parse_source(source);

    let Statement::Function {
        attributes,
        name,
        function,
        ..
    } = &chunk.body[0]
    else {
        panic!("expected function statement");
    };
    assert_eq!(attributes.len(), 1);
    assert_eq!(name.parts.len(), 2);
    assert_eq!(
        name.method.expect("method name").bytes(BStr::new(source)),
        b"value"
    );
    assert_eq!(function.parameters.len(), 2);
    assert!(function.variadic);

    assert!(matches!(chunk.body[1], Statement::LocalFunction { .. }));
}

#[test]
fn parses_function_expressions_and_interpolation() {
    let source = b"local callback = @native function(value) return value end\n\
        local message = `hello {name}!`";
    let chunk = parse_source(source);
    let Statement::Local { values, .. } = &chunk.body[0] else {
        panic!("expected function local");
    };
    assert!(matches!(values[0].kind, ExpressionKind::Function(_)));

    let Statement::Local { values, .. } = &chunk.body[1] else {
        panic!("expected interpolation local");
    };
    let ExpressionKind::Interpolated(expressions) = &values[0].kind else {
        panic!("expected interpolation");
    };
    assert_eq!(expressions.len(), 1);
}

#[test]
fn preserves_spans_for_byte_source() {
    let source = b"local value = '\xff'";
    let chunk = parse_source(source);
    let Statement::Local {
        bindings, values, ..
    } = &chunk.body[0]
    else {
        panic!("expected local statement");
    };
    assert_eq!(bindings[0].name.bytes(BStr::new(source)), b"value");
    assert_eq!(values[0].span.bytes(BStr::new(source)), b"'\xff'");
}

#[test]
fn reports_lexical_errors_with_their_original_span() {
    let source = b"local \xff = 1";
    let error = parse(BStr::new(source)).expect_err("invalid name should fail");
    assert!(matches!(
        error.kind,
        ParseErrorKind::Lexical(TokenKind::Error(_))
    ));
    assert_eq!(error.span.start, 6);
    assert_eq!(error.span.end, 7);
}

#[test]
fn rejects_non_assignable_statement_expressions() {
    let error = parse(BStr::new(b"1 = value")).expect_err("literal cannot be assigned");
    assert_eq!(error.kind, ParseErrorKind::InvalidAssignmentTarget);
}

#[test]
fn parses_standalone_expression_and_type_apis() {
    assert!(matches!(
        parse_expression(BStr::new(b"value + 1"))
            .expect("expression should parse")
            .kind,
        ExpressionKind::Binary { .. }
    ));
    assert!(parse_type(BStr::new(b"{value: string}")).is_ok());
}

#[test]
fn parses_types_annotations_generics_and_assertions() {
    let source = br"
        type Result<T> = {value: T, [string]: number} | nil
        type Mapper<T> = <T>(T) -> T
        function identity<T>(value: T, ...: T): T return value end
        local result: Result<string> = identity<<string>>(value) :: string
        local packed = identity<<(string, number)>>(value)
    ";
    let chunk = parse_source(source);

    assert!(matches!(chunk.body[0], Statement::TypeAlias { .. }));
    assert!(matches!(chunk.body[1], Statement::TypeAlias { .. }));
    assert!(matches!(chunk.body[2], Statement::Function { .. }));
    let Statement::Local {
        bindings, values, ..
    } = &chunk.body[3]
    else {
        panic!("expected typed local");
    };
    assert!(bindings[0].annotation.is_some());
    assert!(matches!(
        values[0].kind,
        ExpressionKind::TypeAssertion { .. }
    ));
    assert!(matches!(chunk.body[4], Statement::Local { .. }));
}

#[test]
fn parses_if_expressions_and_local_conditions() {
    let chunk = parse_source(
        b"local value = if ready then candidate elseif fallback then fallback else nil",
    );
    let Statement::Local { values, .. } = &chunk.body[0] else {
        panic!("expected local statement");
    };
    assert!(matches!(values[0].kind, ExpressionKind::IfElse { .. }));

    let conditional = parse_source(b"if const ready = value then return ready end");
    assert!(matches!(conditional.body[0], Statement::If { .. }));
}

#[test]
fn parses_declarations_classes_exports_and_attribute_arguments() {
    let source = br#"
        @deprecated({reason = "old"}) function old() end
        export const answer: number = 42
        declare global version: string
        declare function print(value: string): nil
        class Box extends Parent
            public value: string
            function get(self): string return self.value end
        end
    "#;
    let chunk = parse_source(source);

    assert!(matches!(chunk.body[0], Statement::Function { .. }));
    assert!(matches!(chunk.body[1], Statement::Export { .. }));
    assert!(matches!(chunk.body[2], Statement::DeclareGlobal { .. }));
    assert!(matches!(chunk.body[3], Statement::DeclareFunction { .. }));
    assert!(matches!(chunk.body[4], Statement::Class { .. }));
}
