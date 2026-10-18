//! Named syntax field tests.

use std::fs;

use vermis::tree::NodeKind;

use crate::support::{check, first};

const SOURCES: &[&[u8]] = &[
    include_bytes!("../fixtures/syntax/fields/assignments.luau").as_slice(),
    include_bytes!("../fixtures/syntax/fields/method.luau").as_slice(),
    include_bytes!("../fixtures/syntax/fields/functions.luau").as_slice(),
    include_bytes!("../fixtures/syntax/fields/condition.luau").as_slice(),
    include_bytes!("../fixtures/syntax/fields/loops.luau").as_slice(),
    include_bytes!("../fixtures/syntax/fields/iteration.luau").as_slice(),
    include_bytes!("../fixtures/syntax/fields/exports.luau").as_slice(),
    include_bytes!("../fixtures/syntax/fields/declaration.luau").as_slice(),
    include_bytes!("../fixtures/syntax/fields/external.luau").as_slice(),
    include_bytes!("../fixtures/syntax/fields/class.luau").as_slice(),
    include_bytes!("../fixtures/syntax/fields/generics.luau").as_slice(),
    include_bytes!("../fixtures/syntax/fields/combination.luau").as_slice(),
    include_bytes!("../fixtures/syntax/fields/types.luau").as_slice(),
    include_bytes!("../fixtures/syntax/fields/expressions.luau").as_slice(),
    include_bytes!("../fixtures/syntax/fields/conditional.luau").as_slice(),
    include_bytes!("../fixtures/syntax/fields/constructors.luau").as_slice(),
    include_bytes!("../fixtures/syntax/fields/attributes.luau").as_slice(),
];

#[test]
fn named_statements_and_expressions() {
    let tree = check(include_bytes!("../fixtures/syntax/statements.luau"));

    assert!(
        tree.diagnostics.is_empty(),
        "{:?}: {:?}",
        tree.source,
        tree.diagnostics
    );

    let NodeKind::Root { block, .. } = tree.node(tree.root).kind else {
        panic!("expected root node");
    };

    let NodeKind::Block { statements } = &tree.node(block).kind else {
        panic!("expected block node");
    };

    assert_eq!(tree.list(statements).len(), 1);

    let NodeKind::Export {
        attributes,
        declaration,
        ..
    } = tree.node(tree.list(statements)[0].node).kind
    else {
        panic!("expected export node");
    };

    assert_eq!(
        tree.text(attributes.expect("expected export attributes")),
        b"@native"
    );

    let NodeKind::Function {
        name,
        generics,
        parameters,
        returns,
        body,
        ..
    } = tree.node(declaration).kind
    else {
        panic!("expected function node");
    };

    assert_eq!(
        tree.text(name.expect("expected function name")),
        b"identity"
    );

    assert_eq!(tree.text(generics.expect("expected generics")), b"<T>");
    assert_eq!(tree.text(parameters), b"(value: T)");

    let NodeKind::Returns { annotation, .. } =
        tree.node(returns.expect("expected return annotation")).kind
    else {
        panic!("expected returns node");
    };

    assert_eq!(tree.text(annotation), b"T");

    let NodeKind::Block { statements } = &tree.node(body.expect("expected function body")).kind
    else {
        panic!("expected block node");
    };

    assert_eq!(tree.list(statements).len(), 3);
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

    assert_eq!(tree.text(name), b"copy");
    assert!(annotation.is_none());
    assert_eq!(tree.list(values).len(), 1);

    let NodeKind::Binary {
        left,
        operator,
        right,
    } = tree.node(tree.list(values)[0].node).kind
    else {
        panic!("expected binary node");
    };

    assert_eq!(tree.text(left), b"value");
    assert_eq!(tree.token(operator).bytes(tree.source), b"+");
    assert_eq!(tree.text(right), b"1");

    let assignment = first(&tree, |kind| {
        matches!(kind, NodeKind::CompoundAssignment { .. })
    });

    let NodeKind::CompoundAssignment {
        target,
        operator,
        value,
    } = tree.node(assignment).kind
    else {
        panic!("expected compound assignment node");
    };

    assert_eq!(tree.text(target), b"copy");
    assert_eq!(tree.token(operator).bytes(tree.source), b"+=");
    assert_eq!(tree.text(value), b"2");
}

#[test]
fn named_types_and_calls() {
    let tree = check(include_bytes!("../fixtures/syntax/types.luau"));

    assert!(
        tree.diagnostics.is_empty(),
        "{:?}: {:?}",
        tree.source,
        tree.diagnostics
    );

    let alias = first(&tree, |kind| matches!(kind, NodeKind::TypeAlias { .. }));

    let NodeKind::TypeAlias {
        name,
        generics,
        annotation,
        ..
    } = tree.node(alias).kind
    else {
        panic!("expected type alias node");
    };

    assert_eq!(tree.text(name), b"Result");

    let NodeKind::Generics { parameters, .. } =
        &tree.node(generics.expect("expected generics")).kind
    else {
        panic!("expected generics node");
    };

    assert_eq!(tree.list(parameters).len(), 2);

    let NodeKind::TypeTable {
        access,
        element,
        fields,
        ..
    } = &tree.node(annotation).kind
    else {
        panic!("expected type table node");
    };

    assert!(access.is_none() && element.is_none());
    let fields = tree.list(fields);
    assert_eq!(fields.len(), 2);

    let NodeKind::TypeField {
        access,
        key,
        annotation,
        ..
    } = tree.node(fields[0].node).kind
    else {
        panic!("expected type field node");
    };

    assert_eq!(
        tree.token(access.expect("expected type access modifier"))
            .bytes(tree.source),
        b"read"
    );

    assert_eq!(tree.text(key), b"value");

    let NodeKind::TypeOptional { annotation, .. } = tree.node(annotation).kind else {
        panic!("expected type optional node");
    };

    assert_eq!(tree.text(annotation), b"T");

    let NodeKind::TypeField { annotation, .. } = tree.node(fields[1].node).kind else {
        panic!("expected type field node");
    };

    let NodeKind::TypeFunction {
        parameters,
        returns,
        ..
    } = tree.node(annotation).kind
    else {
        panic!("expected type function node");
    };

    assert_eq!(tree.text(parameters), b"(T)");
    assert_eq!(tree.text(returns), b"(T, Values...)");
    let call = first(&tree, |kind| matches!(kind, NodeKind::MethodCall { .. }));

    let NodeKind::MethodCall {
        receiver,
        method,
        instantiation,
        arguments,
        ..
    } = tree.node(call).kind
    else {
        panic!("expected method call node");
    };

    assert_eq!(tree.text(receiver), b"object");
    assert_eq!(tree.text(method), b"method");
    assert_eq!(tree.text(arguments), b"(1)");

    let NodeKind::InstantiationArguments { arguments, .. } = tree
        .node(instantiation.expect("expected method instantiation"))
        .kind
    else {
        panic!("expected instantiation arguments node");
    };

    let NodeKind::TypeArguments { arguments, .. } = &tree.node(arguments).kind else {
        panic!("expected type arguments node");
    };

    let NodeKind::TypeName {
        namespace,
        name,
        arguments,
        ..
    } = tree.node(tree.list(arguments)[0].node).kind
    else {
        panic!("expected type name node");
    };

    assert_eq!(
        tree.text(namespace.expect("expected type namespace")),
        b"namespace"
    );

    assert_eq!(tree.text(name), b"Result");

    let NodeKind::TypeArguments { arguments, .. } =
        &tree.node(arguments.expect("expected type arguments")).kind
    else {
        panic!("expected type arguments node");
    };

    let arguments = tree.list(arguments);
    assert!(arguments.is_empty(), "{:?}: {arguments:?}", tree.source);
}

#[test]
fn every_kind_has_named_fields() {
    let mut seen = Vec::new();

    for source in SOURCES {
        let tree = check(source);

        assert!(
            tree.diagnostics.is_empty(),
            "{:?}: {:?}",
            tree.source,
            tree.diagnostics
        );

        seen.extend(tree.nodes.into_iter().map(|node| node.kind));
    }

    for source in [
        include_bytes!("../fixtures/syntax/recovery/missing.luau").as_slice(),
        include_bytes!("../fixtures/syntax/recovery/error.luau").as_slice(),
    ] {
        seen.extend(check(source).nodes.into_iter().map(|node| node.kind));
    }

    macro_rules! covered {
        ($($variant:ident),+) => {
            $(assert!(seen.iter().any(|kind| matches!(kind, NodeKind::$variant { .. })), "missing {} coverage", stringify!($variant));)+
        };
    }

    covered!(
        Root,
        Block,
        Missing,
        Name,
        Number,
        String,
        Boolean,
        Nil,
        Variadic,
        Local,
        Constant,
        Assignment,
        CompoundAssignment,
        CallStatement,
        Function,
        FunctionName,
        Parameters,
        Binding,
        Returns,
        If,
        Branch,
        Else,
        While,
        Repeat,
        NumericFor,
        GenericFor,
        Do,
        Return,
        Break,
        Continue,
        Export,
        TypeAlias,
        Declaration,
        Class,
        Property,
        Extends,
        Attributes,
        AttributeGroup,
        Attribute,
        Arguments,
        Generics,
        Generic,
        Unary,
        Binary,
        Group,
        Call,
        MethodCall,
        Field,
        Index,
        Instantiate,
        InstantiationArguments,
        Assertion,
        Conditional,
        Interpolation,
        Table,
        TableField,
        TypeName,
        TypeTable,
        TypeField,
        TypeIndexer,
        TypeFunction,
        TypeGroup,
        TypePack,
        GenericPack,
        VariadicType,
        TypeParameter,
        TypeArguments,
        TypeUnion,
        TypeIntersection,
        TypeOptional,
        TypeOf,
        Error
    );
}

#[test]
fn corpus_and_recovery() {
    for entry in fs::read_dir("vendor/luau/tests/conformance").expect("expected pinned Luau corpus")
    {
        let path = entry.expect("expected corpus entry").path();

        if path
            .extension()
            .is_some_and(|extension| extension == "lua" || extension == "luau")
        {
            let source = fs::read(path).expect("expected readable corpus program");
            let tree = check(&source);

            assert!(
                tree.diagnostics.is_empty(),
                "{:?}: {:?}",
                tree.source,
                tree.diagnostics
            );
        }
    }

    for source in [
        include_bytes!("../fixtures/syntax/recovery/corpus/binding.luau").as_slice(),
        include_bytes!("../fixtures/syntax/recovery/corpus/string.luau").as_slice(),
        include_bytes!("../fixtures/syntax/recovery/corpus/function.luau").as_slice(),
        include_bytes!("../fixtures/syntax/recovery/corpus/bytes.luau").as_slice(),
    ] {
        check(source);
    }
}

#[test]
fn borrowed_lists_and_recovery() {
    let tree = check(include_bytes!("../fixtures/syntax/lists/return.luau"));
    let statement = first(&tree, |kind| matches!(kind, NodeKind::Return { .. }));

    let NodeKind::Return { values, .. } = &tree.node(statement).kind else {
        panic!("expected return node");
    };

    let values = tree.list(values);
    assert_eq!(values.len(), 3);

    let actual: Vec<_> = values.iter().map(|entry| tree.text(entry.node)).collect();

    assert_eq!(
        actual,
        [
            b"first".as_slice(),
            b"second".as_slice(),
            b"third".as_slice()
        ]
    );

    assert!(values[..2].iter().all(|entry| {
        tree.token(entry.separator.expect("expected list separator"))
            .bytes(tree.source)
            == b","
    }));

    assert!(values[2].separator.is_none());

    for source in [
        include_bytes!("../fixtures/syntax/recovery/lists/expression.luau").as_slice(),
        include_bytes!("../fixtures/syntax/recovery/lists/statements.luau").as_slice(),
    ] {
        let tree = check(source);

        assert!(
            !tree.diagnostics.is_empty(),
            "{:?}: {:?}",
            tree.source,
            tree.diagnostics
        );
    }
}
