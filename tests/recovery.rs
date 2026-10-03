use vermis::{Expected, Kind, Parts, Span, TokenKind, Tree, View, parse};

fn check(source: &[u8]) -> Tree {
    let tree = parse(source);

    let mut restored = Vec::new();
    let mut end = 0;

    for token in tree.tokens() {
        assert_eq!(token.span().start, end);
        end = token.span().end;
        restored.extend_from_slice(token.text());
    }

    assert_eq!(restored, source);
    assert_eq!(end, source.len());

    for node in tree.root().descendants() {
        let span = node.span();
        assert!(span.start <= span.end && span.end <= source.len());
        assert!(node.parts().is_some(), "{source:?}: {node:?}");

        if node.kind() == Kind::Missing {
            assert_eq!(span.start, span.end);
        }

        let mut end = span.start;

        for child in node.children() {
            assert!(child.span().start >= end && child.span().end <= span.end);
            end = child.span().end;
        }
    }

    tree
}

fn first(tree: &Tree, kind: Kind) -> View<'_> {
    tree.root()
        .descendants()
        .find(|node| node.kind() == kind)
        .unwrap()
}

#[test]
fn incomplete_structure_stays_typed_and_lossless() {
    let tree = check(b"local value =");
    assert_ne!(tree.diagnostics(), []);

    let Parts::Local {
        mut bindings,
        mut values,
    } = first(&tree, Kind::Local).parts().unwrap()
    else {
        panic!("missing local view");
    };

    let Parts::Binding { name, annotation } = bindings.next().unwrap().parts().unwrap() else {
        panic!("missing binding view");
    };

    assert_eq!(name.text(), b"value");
    assert!(annotation.is_none());
    assert!(bindings.next().is_none());
    let initializer = values.next().unwrap();
    assert_eq!(initializer.kind(), Kind::Missing);
    assert_eq!(initializer.span().start, b"local value =".len());
    assert!(values.next().is_none());
}

#[test]
fn declarations() {
    for (source, kinds) in [
        (
            "local first, second =",
            &[Kind::Local, Kind::Binding, Kind::Missing][..],
        ),
        (
            "local function",
            &[Kind::LocalFunction, Kind::Parameters, Kind::Missing][..],
        ),
        (
            "function module.",
            &[Kind::Function, Kind::FunctionName, Kind::Missing][..],
        ),
        (
            "function f(first, second:",
            &[Kind::Function, Kind::Binding, Kind::Missing][..],
        ),
        (
            "declare function f(first: number,",
            &[Kind::Declaration, Kind::Parameters, Kind::Missing][..],
        ),
        ("declare value:", &[Kind::Declaration, Kind::Missing][..]),
        (
            "declare extern type Box with value:",
            &[
                Kind::Declaration,
                Kind::Class,
                Kind::TypeField,
                Kind::Missing,
            ][..],
        ),
        ("type Value =", &[Kind::TypeAlias, Kind::Missing][..]),
        (
            "type Value<T,",
            &[
                Kind::TypeAlias,
                Kind::Generics,
                Kind::Generic,
                Kind::Missing,
            ][..],
        ),
        (
            "type Value = {first: number, second:",
            &[
                Kind::TypeAlias,
                Kind::TypeTable,
                Kind::TypeField,
                Kind::Missing,
            ][..],
        ),
        (
            "type Value = (value: number) ->",
            &[Kind::TypeFunctionExpression, Kind::Missing][..],
        ),
        (
            "type Value = namespace.",
            &[Kind::TypeName, Kind::Missing][..],
        ),
        (
            "return f(first,",
            &[Kind::Call, Kind::Arguments, Kind::Missing][..],
        ),
        (
            "return receiver:method",
            &[Kind::MethodCall, Kind::Missing][..],
        ),
        ("return first +", &[Kind::Binary, Kind::Missing][..]),
        (
            "return `before {first +",
            &[Kind::Interpolation, Kind::Binary, Kind::Missing][..],
        ),
        (
            "@[deprecated(",
            &[
                Kind::Attributes,
                Kind::Attribute,
                Kind::Arguments,
                Kind::Function,
                Kind::Missing,
            ][..],
        ),
        (
            "declare callback: @checked",
            &[
                Kind::Declaration,
                Kind::Attributes,
                Kind::TypeFunctionExpression,
                Kind::Missing,
            ][..],
        ),
    ] {
        let tree = check(source.as_bytes());
        assert!(!tree.diagnostics().is_empty(), "{source}");

        for kind in kinds {
            first(&tree, *kind);
        }
    }
}

#[test]
fn expressions() {
    let tree = check(b"return f(first, second +");
    assert_ne!(tree.diagnostics(), []);

    let Parts::Call { arguments, .. } = first(&tree, Kind::Call).parts().unwrap() else {
        panic!()
    };

    let Parts::Arguments { mut values } = arguments.parts().unwrap() else {
        panic!()
    };

    assert_eq!(values.next().unwrap().text(), b"first");

    let Parts::Binary { left, right, .. } = values.next().unwrap().parts().unwrap() else {
        panic!()
    };

    assert_eq!(left.text(), b"second");
    assert_eq!(right.kind(), Kind::Missing);
    assert!(values.next().is_none());
}

#[test]
fn boundaries() {
    for source in [
        "local value = f(first, second + 1)",
        "function f(first: number): number return first end",
        "type Value<T> = {first: T, callback: (T) -> T}",
        "return {Name = 'name', value, {child}}",
    ] {
        assert!(
            check(source.as_bytes()).diagnostics().is_empty(),
            "{source}"
        );

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
    assert_ne!(check(deep.as_bytes()).diagnostics(), []);
}

#[test]
fn updated_block_end_context_matches_full_parse() {
    for ending in ["return 1", "break", "continue"] {
        for statement in ["local value = 2", "do local value = 2 end"] {
            let source = format!("{ending}\n{statement}");
            let start = source.find('2').unwrap();
            let tree = parse(source.as_bytes());

            let updated = tree
                .update(
                    Span {
                        start,
                        end: start + 1,
                    },
                    b"",
                )
                .unwrap();

            let edited = source.replacen('2', "", 1);
            let reparsed = check(edited.as_bytes());
            assert_eq!(updated.diagnostics(), reparsed.diagnostics(), "{source}");

            assert_eq!(
                updated
                    .root()
                    .descendants()
                    .map(View::kind)
                    .collect::<Vec<_>>(),
                reparsed
                    .root()
                    .descendants()
                    .map(View::kind)
                    .collect::<Vec<_>>(),
                "{source}"
            );
        }
    }

    let source = b"local first =\nlocal second = 2\nreturn second";
    let start = source.iter().position(|byte| *byte == b'2').unwrap();

    let updated = parse(source)
        .update(
            Span {
                start,
                end: start + 1,
            },
            b"3",
        )
        .unwrap();

    let reparsed = check(b"local first =\nlocal second = 3\nreturn second");
    assert_eq!(updated.diagnostics(), reparsed.diagnostics());
}

#[test]
fn recovery_loops_make_progress() {
    for source in [b"f'\\256'".as_slice(), b"f'\\xgg'".as_slice()] {
        let tree = check(source);
        assert_ne!(tree.diagnostics(), []);

        let Parts::Call { arguments, .. } = first(&tree, Kind::Call).parts().unwrap() else {
            panic!("missing call view");
        };

        let Parts::Arguments { mut values } = arguments.parts().unwrap() else {
            panic!("missing arguments view");
        };

        let value = values.next().unwrap();
        assert_eq!(value.kind(), Kind::String);
        assert_eq!(value.text(), &source[1..]);
        assert!(values.next().is_none());
    }

    let tree = check(b"local value = 0x\nlocal next = 1");
    assert_ne!(tree.diagnostics(), []);

    let Parts::Local { mut values, .. } = first(&tree, Kind::Local).parts().unwrap() else {
        panic!("missing local view");
    };

    assert_eq!(values.next().unwrap().kind(), Kind::Number);
    assert_eq!(tree.root().children().next().unwrap().children().count(), 2);

    let tree = check(b"type Value = '\\256'\nlocal next = 1");
    assert_ne!(tree.diagnostics(), []);

    let Parts::TypeAlias { annotation, .. } = first(&tree, Kind::TypeAlias).parts().unwrap() else {
        panic!("missing alias view");
    };

    assert_eq!(annotation.kind(), Kind::String);
    first(&tree, Kind::Local);

    let tree = check(b"declare extern type Box with field: '\\256' next: number end");

    assert_ne!(tree.diagnostics(), []);

    assert_eq!(
        tree.root()
            .descendants()
            .filter(|node| node.kind() == Kind::TypeField)
            .count(),
        2
    );

    for source in [
        b"declare extern type Box with ) end".as_slice(),
        b"class Box public ) end".as_slice(),
    ] {
        let tree = check(source);
        assert_ne!(tree.diagnostics(), []);
        first(&tree, Kind::Class);
    }

    let source = format!("return {}f{{}}{}", "(".repeat(254), ")".repeat(254));
    let tree = check(source.as_bytes());

    assert!(
        tree.diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.message == "syntax nesting limit exceeded")
    );

    first(&tree, Kind::Call);
}

#[test]
fn missing_roles_and_punctuation_remain_owned_and_zero_width() {
    let tree = check(b"local value =\nreturn (item");
    let missing = first(&tree, Kind::Missing);

    assert!(
        missing
            .recovery()
            .any(|expectation| expectation.expected == Expected::Role("expression"))
    );

    let group = first(&tree, Kind::Group);

    assert!(
        group
            .recovery()
            .any(|expectation| expectation.expected == Expected::Token(TokenKind::Byte(b')')))
    );

    for node in tree.root().descendants() {
        for expectation in node.recovery() {
            assert!(expectation.span.is_empty());

            assert!(
                expectation.span.start >= node.span().start
                    && expectation.span.start <= tree.source().len()
            );
        }
    }

    let table = check(b"return {Value = item");

    assert!(
        table
            .root()
            .descendants()
            .flat_map(View::recovery)
            .any(|expectation| expectation.expected == Expected::Token(TokenKind::Byte(b'}')))
    );
}

#[test]
fn partially_consumed_depth_recovery_preserves_lossless_missing_nodes() {
    let source = format!("return {}if elseif{}", "(".repeat(254), ")".repeat(254));
    let tree = check(source.as_bytes());

    assert!(
        tree.diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.message == "syntax nesting limit exceeded")
    );

    assert!(
        tree.root()
            .descendants()
            .any(|node| node.kind() == Kind::Missing)
    );
}
