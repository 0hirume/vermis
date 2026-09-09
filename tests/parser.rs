use bstr::BStr;
use vermis::{
    BinaryOperator, ExpressionKind, ParseErrorKind, Statement, TokenKind, UnaryOperator, parse,
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
        br#"
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
        "#,
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
    let source = b"local callback = function(value) return value end\n\
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
