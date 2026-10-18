//! Parser recovery tests.

use vermis::tree::NodeKind;

use crate::support::{check, first};

#[test]
fn incomplete_structure_stays_typed_and_lossless() {
    let tree = check(include_bytes!("../fixtures/recovery/initializer.luau"));

    assert!(
        !tree.diagnostics.is_empty(),
        "{:?}: {:?}",
        tree.source,
        tree.diagnostics
    );

    let local = first(&tree, |kind| matches!(kind, NodeKind::Local { .. }));

    let NodeKind::Local {
        bindings, values, ..
    } = &tree.node(local).kind
    else {
        panic!("expected local node");
    };

    assert_eq!(tree.list(bindings).len(), 1);

    let NodeKind::Binding {
        name, annotation, ..
    } = tree.node(tree.list(bindings)[0].node).kind
    else {
        panic!("expected binding node");
    };

    assert_eq!(tree.text(name), b"value");
    assert!(annotation.is_none());
    assert_eq!(tree.list(values).len(), 1);

    let initializer = tree.node(tree.list(values)[0].node);

    let NodeKind::Missing { .. } = initializer.kind else {
        panic!("expected missing node");
    };

    assert_eq!(initializer.span.start, 13);
}

#[test]
fn declarations() {
    macro_rules! rejected {
        ($source:expr, $($variant:ident),+) => {{
            let tree = check($source);
            assert!(!tree.diagnostics.is_empty(), "{:?}: {:?}", tree.source, tree.diagnostics);
            $(assert!(
                tree.nodes.iter().any(|node| matches!(node.kind, NodeKind::$variant { .. })),
                "{:?}: missing {}", $source, stringify!($variant)
            );)+
        }};
    }

    rejected!(
        include_bytes!("../fixtures/recovery/declarations/bindings.luau").as_slice(),
        Local,
        Binding,
        Missing
    );

    rejected!(
        include_bytes!("../fixtures/recovery/declarations/function.luau").as_slice(),
        Function,
        Parameters,
        Missing
    );

    rejected!(
        include_bytes!("../fixtures/recovery/declarations/qualification.luau").as_slice(),
        Function,
        FunctionName,
        Missing
    );

    rejected!(
        include_bytes!("../fixtures/recovery/declarations/parameter.luau").as_slice(),
        Function,
        Binding,
        Missing
    );

    rejected!(
        include_bytes!("../fixtures/recovery/declarations/signature.luau").as_slice(),
        Function,
        Parameters,
        Missing
    );

    rejected!(
        include_bytes!("../fixtures/recovery/declarations/annotation.luau").as_slice(),
        Declaration,
        Missing
    );

    rejected!(
        include_bytes!("../fixtures/recovery/declarations/class.luau").as_slice(),
        Declaration,
        Class,
        TypeField,
        Missing
    );

    rejected!(
        include_bytes!("../fixtures/recovery/declarations/alias.luau").as_slice(),
        TypeAlias,
        Missing
    );

    rejected!(
        include_bytes!("../fixtures/recovery/declarations/generics.luau").as_slice(),
        TypeAlias,
        Generics,
        Generic,
        Missing
    );

    rejected!(
        include_bytes!("../fixtures/recovery/declarations/table.luau").as_slice(),
        TypeAlias,
        TypeTable,
        TypeField,
        Missing
    );

    rejected!(
        include_bytes!("../fixtures/recovery/declarations/result.luau").as_slice(),
        TypeFunction,
        Missing
    );

    rejected!(
        include_bytes!("../fixtures/recovery/declarations/namespace.luau").as_slice(),
        TypeName,
        Missing
    );

    rejected!(
        include_bytes!("../fixtures/recovery/declarations/arguments.luau").as_slice(),
        Call,
        Arguments,
        Missing
    );

    rejected!(
        include_bytes!("../fixtures/recovery/declarations/method.luau").as_slice(),
        MethodCall,
        Missing
    );

    rejected!(
        include_bytes!("../fixtures/recovery/declarations/binary.luau").as_slice(),
        Binary,
        Missing
    );

    rejected!(
        include_bytes!("../fixtures/recovery/declarations/interpolation.luau").as_slice(),
        Interpolation,
        Binary,
        Missing
    );

    rejected!(
        include_bytes!("../fixtures/recovery/declarations/attributes.luau").as_slice(),
        Attributes,
        Attribute,
        Arguments,
        Function,
        Missing
    );

    rejected!(
        include_bytes!("../fixtures/recovery/declarations/callback.luau").as_slice(),
        Declaration,
        Attributes,
        TypeFunction,
        Missing
    );
}

#[test]
fn expressions() {
    let tree = check(include_bytes!("../fixtures/recovery/expressions.luau"));

    assert!(
        !tree.diagnostics.is_empty(),
        "{:?}: {:?}",
        tree.source,
        tree.diagnostics
    );

    let call = first(&tree, |kind| matches!(kind, NodeKind::Call { .. }));

    let NodeKind::Call { arguments, .. } = tree.node(call).kind else {
        panic!("expected call node");
    };

    let NodeKind::Arguments { values, .. } = &tree.node(arguments).kind else {
        panic!("expected arguments node");
    };

    let values = tree.list(values);
    assert_eq!(values.len(), 2);
    assert_eq!(tree.text(values[0].node), b"first");

    let NodeKind::Binary { left, right, .. } = tree.node(values[1].node).kind else {
        panic!("expected binary node");
    };

    assert_eq!(tree.text(left), b"second");

    let NodeKind::Missing { .. } = tree.node(right).kind else {
        panic!("expected missing node");
    };
}

#[test]
fn boundaries() {
    for source in [
        include_bytes!("../fixtures/recovery/boundaries/local.luau").as_slice(),
        include_bytes!("../fixtures/recovery/boundaries/function.luau").as_slice(),
        include_bytes!("../fixtures/recovery/boundaries/alias.luau").as_slice(),
        include_bytes!("../fixtures/recovery/boundaries/table.luau").as_slice(),
    ] {
        let tree = check(source);

        assert!(
            tree.diagnostics.is_empty(),
            "{:?}: {:?}",
            tree.source,
            tree.diagnostics
        );

        for end in 0..source.len() {
            check(&source[..end]);
        }
    }

    for byte in u8::MIN..=u8::MAX {
        check(&[b'l', byte]);
        check(&[b'<', byte]);
        check(&[b'{', b'F', b'=', b'{', byte]);
    }

    let deep = format!("return {}value{}", "(".repeat(1000), ")".repeat(1000));
    let tree = check(deep.as_bytes());

    assert!(
        !tree.diagnostics.is_empty(),
        "{:?}: {:?}",
        tree.source,
        tree.diagnostics
    );
}

#[test]
fn recovery_loops_make_progress() {
    for source in [
        include_bytes!("../fixtures/recovery/progress/decimal.luau").as_slice(),
        include_bytes!("../fixtures/recovery/progress/hexadecimal.luau").as_slice(),
    ] {
        let tree = check(source);

        assert!(
            !tree.diagnostics.is_empty(),
            "{:?}: {:?}",
            tree.source,
            tree.diagnostics
        );

        let call = first(&tree, |kind| matches!(kind, NodeKind::Call { .. }));

        let NodeKind::Call { arguments, .. } = tree.node(call).kind else {
            panic!("expected call node");
        };

        let NodeKind::Arguments { values, .. } = &tree.node(arguments).kind else {
            panic!("expected arguments node");
        };

        assert_eq!(tree.list(values).len(), 1);

        let value = tree.list(values)[0].node;

        let NodeKind::String { .. } = tree.node(value).kind else {
            panic!("expected string node");
        };

        assert_eq!(tree.text(value), &source[1..]);
    }

    let tree = check(include_bytes!("../fixtures/recovery/progress/number.luau"));

    assert!(
        !tree.diagnostics.is_empty(),
        "{:?}: {:?}",
        tree.source,
        tree.diagnostics
    );

    let local = first(&tree, |kind| matches!(kind, NodeKind::Local { .. }));

    let NodeKind::Local { values, .. } = &tree.node(local).kind else {
        panic!("expected local node");
    };

    let NodeKind::Number { .. } = tree.node(tree.list(values)[0].node).kind else {
        panic!("expected number node");
    };

    let NodeKind::Root { block, .. } = tree.node(tree.root).kind else {
        panic!("expected root node");
    };

    let NodeKind::Block { statements } = &tree.node(block).kind else {
        panic!("expected block node");
    };

    assert_eq!(tree.list(statements).len(), 2);

    let tree = check(include_bytes!("../fixtures/recovery/progress/alias.luau"));

    assert!(
        !tree.diagnostics.is_empty(),
        "{:?}: {:?}",
        tree.source,
        tree.diagnostics
    );

    let alias = first(&tree, |kind| matches!(kind, NodeKind::TypeAlias { .. }));

    let NodeKind::TypeAlias { annotation, .. } = tree.node(alias).kind else {
        panic!("expected type alias node");
    };

    let NodeKind::String { .. } = tree.node(annotation).kind else {
        panic!("expected string node");
    };

    first(&tree, |kind| matches!(kind, NodeKind::Local { .. }));

    let tree = check(include_bytes!("../fixtures/recovery/progress/fields.luau"));

    assert!(
        !tree.diagnostics.is_empty(),
        "{:?}: {:?}",
        tree.source,
        tree.diagnostics
    );

    assert_eq!(
        tree.nodes
            .iter()
            .filter(|node| matches!(node.kind, NodeKind::TypeField { .. }))
            .count(),
        2
    );

    for source in [
        include_bytes!("../fixtures/recovery/progress/declaration.luau").as_slice(),
        include_bytes!("../fixtures/recovery/progress/class.luau").as_slice(),
    ] {
        let tree = check(source);

        assert!(
            !tree.diagnostics.is_empty(),
            "{:?}: {:?}",
            tree.source,
            tree.diagnostics
        );

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
    let tree = check(include_bytes!(
        "../fixtures/recovery/punctuation/group.luau"
    ));

    let missing = first(&tree, |kind| matches!(kind, NodeKind::Missing { .. }));

    let NodeKind::Missing {
        expected: "expression",
    } = tree.node(missing).kind
    else {
        panic!("expected missing node");
    };

    let group = first(&tree, |kind| matches!(kind, NodeKind::Group { .. }));

    let NodeKind::Group { closing: None, .. } = tree.node(group).kind else {
        panic!("expected group node");
    };

    assert!(
        tree.diagnostics
            .iter()
            .any(|diagnostic| diagnostic.span.is_empty()
                && diagnostic.span.start == tree.source.len())
    );

    let tree = check(include_bytes!(
        "../fixtures/recovery/punctuation/table.luau"
    ));

    let table = first(&tree, |kind| matches!(kind, NodeKind::Table { .. }));

    let NodeKind::Table { closing: None, .. } = tree.node(table).kind else {
        panic!("expected table node");
    };

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
    let unit = include_bytes!("../fixtures/recovery/repetition.luau").as_slice();
    let diagnostics = check(unit).diagnostics.len();

    for depth in [2, 4, 8, 16, 64] {
        let source = unit.repeat(depth);
        let tree = check(&source);

        assert!(
            tree.diagnostics.len() <= diagnostics * depth,
            "{} diagnostics at depth {depth}",
            tree.diagnostics.len()
        );
    }
}
