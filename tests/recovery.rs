//! Parser recovery tests.

pub mod support;

use support::{check, first};
use vermis::tree::NodeKind;

#[test]
fn incomplete_structure_stays_typed_and_lossless() {
    let tree = check(b"local value =");
    assert_ne!(tree.diagnostics, []);
    let local = first(&tree, |kind| matches!(kind, NodeKind::Local { .. }));

    let NodeKind::Local {
        bindings, values, ..
    } = &tree.node(local).kind
    else {
        panic!()
    };

    assert_eq!(tree.list(bindings).len(), 1);

    let NodeKind::Binding {
        name, annotation, ..
    } = tree.node(tree.list(bindings)[0].node).kind
    else {
        panic!()
    };

    assert_eq!(tree.text(name), b"value");
    assert!(annotation.is_none());
    assert_eq!(tree.list(values).len(), 1);
    let initializer = tree.node(tree.list(values)[0].node);
    assert!(matches!(initializer.kind, NodeKind::Missing { .. }));
    assert_eq!(initializer.span.start, b"local value =".len());
}

#[test]
fn declarations() {
    macro_rules! rejected {
        ($source:literal, $($variant:ident),+) => {{
            let tree = check($source.as_bytes());
            assert!(!tree.diagnostics.is_empty(), "{}", $source);
            $(assert!(
                tree.nodes.iter().any(|node| matches!(node.kind, NodeKind::$variant { .. })),
                "{}: missing {}", $source, stringify!($variant)
            );)+
        }};
    }

    rejected!("local first, second =", Local, Binding, Missing);
    rejected!("local function", Function, Parameters, Missing);
    rejected!("function module.", Function, FunctionName, Missing);
    rejected!("function f(first, second:", Function, Binding, Missing);

    rejected!(
        "declare function f(first: number,",
        Function,
        Parameters,
        Missing
    );

    rejected!("declare value:", Declaration, Missing);

    rejected!(
        "declare extern type Box with value:",
        Declaration,
        Class,
        TypeField,
        Missing
    );

    rejected!("type Value =", TypeAlias, Missing);
    rejected!("type Value<T,", TypeAlias, Generics, Generic, Missing);

    rejected!(
        "type Value = {first: number, second:",
        TypeAlias,
        TypeTable,
        TypeField,
        Missing
    );

    rejected!("type Value = (value: number) ->", TypeFunction, Missing);
    rejected!("type Value = namespace.", TypeName, Missing);
    rejected!("return f(first,", Call, Arguments, Missing);
    rejected!("return receiver:method", MethodCall, Missing);
    rejected!("return first +", Binary, Missing);
    rejected!("return `before {first +", Interpolation, Binary, Missing);

    rejected!(
        "@[deprecated(",
        Attributes,
        Attribute,
        Arguments,
        Function,
        Missing
    );

    rejected!(
        "declare callback: @checked",
        Declaration,
        Attributes,
        TypeFunction,
        Missing
    );
}

#[test]
fn expressions() {
    let tree = check(b"return f(first, second +");
    assert_ne!(tree.diagnostics, []);
    let call = first(&tree, |kind| matches!(kind, NodeKind::Call { .. }));

    let NodeKind::Call { arguments, .. } = tree.node(call).kind else {
        panic!()
    };

    let NodeKind::Arguments { values, .. } = &tree.node(arguments).kind else {
        panic!()
    };

    let values = tree.list(values);
    assert_eq!(values.len(), 2);
    assert_eq!(tree.text(values[0].node), b"first");

    let NodeKind::Binary { left, right, .. } = tree.node(values[1].node).kind else {
        panic!()
    };

    assert_eq!(tree.text(left), b"second");
    assert!(matches!(tree.node(right).kind, NodeKind::Missing { .. }));
}

#[test]
fn boundaries() {
    for source in [
        "local value = f(first, second + 1)",
        "function f(first: number): number return first end",
        "type Value<T> = {first: T, callback: (T) -> T}",
        "return {Name = 'name', value, {child}}",
    ] {
        assert!(check(source.as_bytes()).diagnostics.is_empty(), "{source}");

        for end in 0..source.len() {
            check(&source.as_bytes()[..end]);
        }
    }

    for byte in u8::MIN..=u8::MAX {
        check(&[b'l', byte]);
        check(&[b'<', byte]);
        check(&[b'{', b'F', b'=', b'{', byte]);
    }

    let deep = format!("return {}value{}", "(".repeat(1000), ")".repeat(1000));
    assert_ne!(check(deep.as_bytes()).diagnostics, []);
}

#[test]
fn recovery_loops_make_progress() {
    for source in [b"f'\\256'".as_slice(), b"f'\\xgg'".as_slice()] {
        let tree = check(source);
        assert_ne!(tree.diagnostics, []);
        let call = first(&tree, |kind| matches!(kind, NodeKind::Call { .. }));

        let NodeKind::Call { arguments, .. } = tree.node(call).kind else {
            panic!()
        };

        let NodeKind::Arguments { values, .. } = &tree.node(arguments).kind else {
            panic!()
        };

        assert_eq!(tree.list(values).len(), 1);
        let value = tree.list(values)[0].node;
        assert!(matches!(tree.node(value).kind, NodeKind::String { .. }));
        assert_eq!(tree.text(value), &source[1..]);
    }

    let tree = check(b"local value = 0x\nlocal next = 1");
    assert_ne!(tree.diagnostics, []);
    let local = first(&tree, |kind| matches!(kind, NodeKind::Local { .. }));

    let NodeKind::Local { values, .. } = &tree.node(local).kind else {
        panic!()
    };

    assert!(matches!(
        tree.node(tree.list(values)[0].node).kind,
        NodeKind::Number { .. }
    ));

    let NodeKind::Root { block, .. } = tree.node(tree.root).kind else {
        panic!()
    };

    let NodeKind::Block { statements } = &tree.node(block).kind else {
        panic!()
    };

    assert_eq!(tree.list(statements).len(), 2);
    let tree = check(b"type Value = '\\256'\nlocal next = 1");
    assert_ne!(tree.diagnostics, []);
    let alias = first(&tree, |kind| matches!(kind, NodeKind::TypeAlias { .. }));

    let NodeKind::TypeAlias { annotation, .. } = tree.node(alias).kind else {
        panic!()
    };

    assert!(matches!(
        tree.node(annotation).kind,
        NodeKind::String { .. }
    ));

    first(&tree, |kind| matches!(kind, NodeKind::Local { .. }));
    let tree = check(b"declare extern type Box with field: '\\256' next: number end");
    assert_ne!(tree.diagnostics, []);

    assert_eq!(
        tree.nodes
            .iter()
            .filter(|node| matches!(node.kind, NodeKind::TypeField { .. }))
            .count(),
        2
    );

    for source in [
        b"declare extern type Box with ) end".as_slice(),
        b"class Box public ) end".as_slice(),
    ] {
        let tree = check(source);
        assert_ne!(tree.diagnostics, []);
        first(&tree, |kind| matches!(kind, NodeKind::Class { .. }));
    }

    let source = format!("return {}f{{}}{}", "(".repeat(254), ")".repeat(254));
    let tree = check(source.as_bytes());

    assert!(
        tree.diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message == "syntax nesting limit exceeded")
    );

    first(&tree, |kind| matches!(kind, NodeKind::Call { .. }));
}

#[test]
fn missing_roles_and_punctuation_remain_explicit() {
    let tree = check(b"local value =\nreturn (item");
    let missing = first(&tree, |kind| matches!(kind, NodeKind::Missing { .. }));

    assert!(matches!(
        tree.node(missing).kind,
        NodeKind::Missing {
            expected: "expression"
        }
    ));

    let group = first(&tree, |kind| matches!(kind, NodeKind::Group { .. }));

    assert!(matches!(
        tree.node(group).kind,
        NodeKind::Group { closing: None, .. }
    ));

    assert!(
        tree.diagnostics
            .iter()
            .any(|diagnostic| diagnostic.span.is_empty()
                && diagnostic.span.start == tree.source.len())
    );

    let tree = check(b"return {Value = item");
    let table = first(&tree, |kind| matches!(kind, NodeKind::Table { .. }));

    assert!(matches!(
        tree.node(table).kind,
        NodeKind::Table { closing: None, .. }
    ));

    assert!(
        tree.diagnostics
            .iter()
            .any(|diagnostic| diagnostic.span.is_empty()
                && diagnostic.span.start == tree.source.len())
    );
}

#[test]
fn partially_consumed_depth_recovery_preserves_lossless_missing_nodes() {
    let source = format!("return {}if elseif{}", "(".repeat(254), ")".repeat(254));
    let tree = check(source.as_bytes());

    assert!(
        tree.diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message == "syntax nesting limit exceeded")
    );

    first(&tree, |kind| matches!(kind, NodeKind::Missing { .. }));
}

#[test]
fn nested_syntax_stays_within_stack_budget() {
    for depth in [96, 160, 256, 512] {
        for source in [
            format!("return {}value{}", "(".repeat(depth), ")".repeat(depth)),
            format!(
                "type Value = {}number{}",
                "(".repeat(depth),
                ")".repeat(depth)
            ),
        ] {
            let tree = check(source.as_bytes());

            assert_eq!(
                tree.diagnostics
                    .iter()
                    .any(|diagnostic| diagnostic.message == "syntax nesting limit exceeded"),
                depth > 96
            );
        }
    }
}

#[test]
fn failed_nested_functions_do_not_leave_unreachable_nodes() {
    for (depth, declaration) in [
        (255, "function f"),
        (255, "local function f"),
        (255, "type function f"),
        (255, "export function f"),
        (255, "@native function f"),
        (255, "@native"),
        (254, "class C function f"),
    ] {
        let source = format!("{}{}", "do ".repeat(depth), declaration);
        let tree = check(source.as_bytes());

        assert!(
            tree.diagnostics
                .iter()
                .any(|diagnostic| diagnostic.message == "syntax nesting limit exceeded"),
            "{declaration}"
        );
    }
}

#[test]
fn failed_statements_are_not_reparsed() {
    let unit = "(@;";
    let diagnostics = check(unit.as_bytes()).diagnostics.len();

    for depth in [2, 4, 8, 16, 64] {
        let source = unit.repeat(depth);
        let tree = check(source.as_bytes());

        assert!(
            tree.diagnostics.len() <= diagnostics * depth,
            "{} diagnostics at depth {depth}",
            tree.diagnostics.len()
        );
    }
}
