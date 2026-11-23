//! Parser grammar tests.

use vermis::tree::NodeKind;

use crate::support::{accepted, check, first};

#[test]
fn precedence_and_associativity() {
    for (source, expected_operator, expected_left, expected_right) in [
        (
            include_bytes!("../fixtures/parser/precedence/addition.luau").as_slice(),
            "+",
            "a",
            "b * c",
        ),
        (
            include_bytes!("../fixtures/parser/precedence/subtraction.luau").as_slice(),
            "-",
            "a - b",
            "c",
        ),
        (
            include_bytes!("../fixtures/parser/precedence/exponentiation.luau").as_slice(),
            "^",
            "a",
            "b ^ c",
        ),
        (
            include_bytes!("../fixtures/parser/precedence/concatenation.luau").as_slice(),
            "..",
            "a",
            "b .. c",
        ),
        (
            include_bytes!("../fixtures/parser/precedence/disjunction.luau").as_slice(),
            "or",
            "a",
            "b and c",
        ),
    ] {
        let tree = accepted(source);
        let binary = first(&tree, |kind| matches!(kind, NodeKind::Binary { .. }));

        let NodeKind::Binary {
            left,
            operator,
            right,
        } = tree.node(binary).kind
        else {
            panic!("expected binary node");
        };

        assert_eq!(tree.text(left), expected_left.as_bytes());

        assert_eq!(
            tree.token(operator).bytes(tree.source),
            expected_operator.as_bytes()
        );

        assert_eq!(tree.text(right), expected_right.as_bytes());
    }

    let tree = accepted(include_bytes!("../fixtures/parser/precedence/negation.luau").as_slice());
    let unary = first(&tree, |kind| matches!(kind, NodeKind::Unary { .. }));

    let NodeKind::Unary { operand, .. } = tree.node(unary).kind else {
        panic!("expected unary node");
    };

    let NodeKind::Binary { .. } = tree.node(operand).kind else {
        panic!("expected binary node");
    };
}

#[test]
fn condition_bindings() {
    for source in [
        include_bytes!("../fixtures/parser/conditions/bindings/statement.luau").as_slice(),
        include_bytes!("../fixtures/parser/conditions/bindings/expression.luau").as_slice(),
    ] {
        let tree = accepted(source);

        let bindings: Vec<_> = tree
            .nodes
            .iter()
            .filter_map(|node| {
                let (constant, bindings, values) = match &node.kind {
                    NodeKind::Local {
                        bindings, values, ..
                    } => (false, bindings, values),

                    NodeKind::Constant {
                        bindings, values, ..
                    } => (true, bindings, values),

                    _ => return None,
                };

                assert_eq!(tree.list(bindings).len(), 1);
                assert_eq!(tree.list(values).len(), 1);

                Some((
                    constant,
                    tree.text(tree.list(bindings)[0].node),
                    tree.text(tree.list(values)[0].node),
                ))
            })
            .collect();

        assert_eq!(
            bindings,
            [
                (false, b"first: number".as_slice(), b"value".as_slice()),
                (true, b"second".as_slice(), b"other".as_slice())
            ]
        );
    }

    for source in [
        include_bytes!("../fixtures/parser/conditions/context/statement.luau").as_slice(),
        include_bytes!("../fixtures/parser/conditions/context/expression.luau").as_slice(),
    ] {
        let tree = accepted(source);

        assert!(
            tree.nodes
                .iter()
                .all(|node| !matches!(node.kind, NodeKind::Constant { .. }))
        );
    }
}

#[test]
fn excess_constant_initializers() {
    for source in [
        include_bytes!("../fixtures/parser/initializers/excess/constant.luau").as_slice(),
        include_bytes!("../fixtures/parser/initializers/excess/export.luau").as_slice(),
    ] {
        let tree = check(source);
        assert_eq!(tree.diagnostics.len(), 1, "{source:?}");

        assert_eq!(
            tree.diagnostics[0].message,
            "too many constant initializers"
        );
    }

    for source in [
        include_bytes!("../fixtures/parser/initializers/local.luau").as_slice(),
        include_bytes!("../fixtures/parser/initializers/expansion.luau").as_slice(),
    ] {
        accepted(source);
    }
}

#[test]
fn grammar() {
    for source in [
        include_bytes!("../fixtures/parser/grammar/conditional.luau").as_slice(),
        include_bytes!("../fixtures/parser/grammar/while.luau").as_slice(),
        include_bytes!("../fixtures/parser/grammar/repeat.luau").as_slice(),
        include_bytes!("../fixtures/parser/grammar/numeric.luau").as_slice(),
        include_bytes!("../fixtures/parser/grammar/iteration.luau").as_slice(),
        include_bytes!("../fixtures/parser/grammar/block.luau").as_slice(),
        include_bytes!("../fixtures/parser/grammar/variadic.luau").as_slice(),
        include_bytes!("../fixtures/parser/grammar/chaining.luau").as_slice(),
        include_bytes!("../fixtures/parser/grammar/interpolation.luau").as_slice(),
        include_bytes!("../fixtures/parser/grammar/expression.luau").as_slice(),
        include_bytes!("../fixtures/parser/grammar/anonymous.luau").as_slice(),
        include_bytes!("../fixtures/parser/grammar/deprecated.luau").as_slice(),
        include_bytes!("../fixtures/parser/grammar/constant.luau").as_slice(),
        include_bytes!("../fixtures/parser/grammar/alias.luau").as_slice(),
        include_bytes!("../fixtures/parser/grammar/metaprogram.luau").as_slice(),
        include_bytes!("../fixtures/parser/grammar/defaults.luau").as_slice(),
        include_bytes!("../fixtures/parser/grammar/arguments.luau").as_slice(),
        include_bytes!("../fixtures/parser/grammar/access.luau").as_slice(),
        include_bytes!("../fixtures/parser/grammar/indexer.luau").as_slice(),
        include_bytes!("../fixtures/parser/grammar/generic.luau").as_slice(),
        include_bytes!("../fixtures/parser/grammar/combination.luau").as_slice(),
        include_bytes!("../fixtures/parser/grammar/array.luau").as_slice(),
        include_bytes!("../fixtures/parser/grammar/instantiation.luau").as_slice(),
        include_bytes!("../fixtures/parser/grammar/declaration.luau").as_slice(),
        include_bytes!("../fixtures/parser/grammar/collection.luau").as_slice(),
        include_bytes!("../fixtures/parser/grammar/checked.luau").as_slice(),
        include_bytes!("../fixtures/parser/grammar/callback.luau").as_slice(),
        include_bytes!("../fixtures/parser/grammar/attributed.luau").as_slice(),
        include_bytes!("../fixtures/parser/grammar/method.luau").as_slice(),
        include_bytes!("../fixtures/parser/grammar/union.luau").as_slice(),
        include_bytes!("../fixtures/parser/grammar/intersection.luau").as_slice(),
        include_bytes!("../fixtures/parser/grammar/external.luau").as_slice(),
        include_bytes!("../fixtures/parser/grammar/class.luau").as_slice(),
        include_bytes!("../fixtures/parser/grammar/names.luau").as_slice(),
        include_bytes!("../fixtures/parser/grammar/calls.luau").as_slice(),
    ] {
        accepted(source);
    }
}

#[test]
fn attributed_declarations() {
    for (attributes, sources) in [
        (
            "@native",
            [
                include_bytes!("../fixtures/parser/attributes/export/native.luau").as_slice(),
                include_bytes!("../fixtures/parser/attributes/constant/native.luau").as_slice(),
            ],
        ),
        (
            "@[native, deprecated({reason = 'old'})]",
            [
                include_bytes!("../fixtures/parser/attributes/export/group.luau").as_slice(),
                include_bytes!("../fixtures/parser/attributes/constant/group.luau").as_slice(),
            ],
        ),
        (
            "@[deprecated {reason = 'old'}]",
            [
                include_bytes!("../fixtures/parser/attributes/export/table.luau").as_slice(),
                include_bytes!("../fixtures/parser/attributes/constant/table.luau").as_slice(),
            ],
        ),
        (
            "@[native 'message']",
            [
                include_bytes!("../fixtures/parser/attributes/export/string.luau").as_slice(),
                include_bytes!("../fixtures/parser/attributes/constant/string.luau").as_slice(),
            ],
        ),
        (
            "@[native [[message]]]",
            [
                include_bytes!("../fixtures/parser/attributes/export/long.luau").as_slice(),
                include_bytes!("../fixtures/parser/attributes/constant/long.luau").as_slice(),
            ],
        ),
    ] {
        for (keyword, source) in ["export", "const"].into_iter().zip(sources) {
            let tree = accepted(source);
            let function = first(&tree, |kind| matches!(kind, NodeKind::Function { .. }));

            let NodeKind::Function {
                attributes: function_attributes,
                prefix,
                generics,
                parameters,
                returns,
                body,
                ..
            } = tree.node(function).kind
            else {
                panic!("expected function node");
            };

            let attributes_node = if keyword == "export" {
                let export = first(&tree, |kind| matches!(kind, NodeKind::Export { .. }));

                let NodeKind::Export {
                    attributes,
                    declaration,
                    ..
                } = tree.node(export).kind
                else {
                    panic!("expected export node");
                };

                assert_eq!(tree.text(export), source);
                assert_eq!(declaration, function);

                attributes.expect("expected export attributes")
            } else {
                assert_eq!(tree.text(function), source);

                assert_eq!(
                    tree.token(prefix.expect("expected constant prefix"))
                        .bytes(tree.source),
                    b"const"
                );

                function_attributes.expect("expected function attributes")
            };

            let NodeKind::Attributes { .. } = tree.node(attributes_node).kind else {
                panic!("expected attributes node");
            };

            assert_eq!(tree.text(attributes_node), attributes.as_bytes());

            let NodeKind::Generics { .. } = tree
                .node(generics.expect("expected function generics"))
                .kind
            else {
                panic!("expected generics node");
            };

            let NodeKind::Parameters { .. } = tree.node(parameters).kind else {
                panic!("expected parameters node");
            };

            let NodeKind::Returns { .. } = tree
                .node(returns.expect("expected function return annotation"))
                .kind
            else {
                panic!("expected returns node");
            };

            let NodeKind::Block { .. } = tree.node(body.expect("expected function body")).kind
            else {
                panic!("expected block node");
            };
        }
    }

    accepted(include_bytes!("../fixtures/parser/attributes/declarations/constant.luau").as_slice());

    accepted(include_bytes!("../fixtures/parser/attributes/declarations/local.luau").as_slice());

    accepted(
        include_bytes!("../fixtures/parser/attributes/declarations/namespace.luau").as_slice(),
    );

    accepted(include_bytes!("../fixtures/parser/attributes/declarations/external.luau").as_slice());

    for source in [
        include_bytes!("../fixtures/parser/attributes/invalid/local.luau").as_slice(),
        include_bytes!("../fixtures/parser/attributes/invalid/constant.luau").as_slice(),
        include_bytes!("../fixtures/parser/attributes/invalid/exported.luau").as_slice(),
        include_bytes!("../fixtures/parser/attributes/invalid/attributed.luau").as_slice(),
        include_bytes!("../fixtures/parser/attributes/invalid/namespace.luau").as_slice(),
        include_bytes!("../fixtures/parser/attributes/invalid/value.luau").as_slice(),
        include_bytes!("../fixtures/parser/attributes/invalid/method.luau").as_slice(),
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

#[test]
fn classes_are_structured() {
    for (reference, sources) in [
        (
            "Parent",
            [
                include_bytes!("../fixtures/parser/classes/name/plain.luau").as_slice(),
                include_bytes!("../fixtures/parser/classes/name/open.luau").as_slice(),
                include_bytes!("../fixtures/parser/classes/name/export.luau").as_slice(),
                include_bytes!("../fixtures/parser/classes/name/combined.luau").as_slice(),
            ],
        ),
        (
            "classes.Parent",
            [
                include_bytes!("../fixtures/parser/classes/field/plain.luau").as_slice(),
                include_bytes!("../fixtures/parser/classes/field/open.luau").as_slice(),
                include_bytes!("../fixtures/parser/classes/field/export.luau").as_slice(),
                include_bytes!("../fixtures/parser/classes/field/combined.luau").as_slice(),
            ],
        ),
        (
            "classes['Parent']",
            [
                include_bytes!("../fixtures/parser/classes/string/plain.luau").as_slice(),
                include_bytes!("../fixtures/parser/classes/string/open.luau").as_slice(),
                include_bytes!("../fixtures/parser/classes/string/export.luau").as_slice(),
                include_bytes!("../fixtures/parser/classes/string/combined.luau").as_slice(),
            ],
        ),
        (
            "classes[select(name)]",
            [
                include_bytes!("../fixtures/parser/classes/call/plain.luau").as_slice(),
                include_bytes!("../fixtures/parser/classes/call/open.luau").as_slice(),
                include_bytes!("../fixtures/parser/classes/call/export.luau").as_slice(),
                include_bytes!("../fixtures/parser/classes/call/combined.luau").as_slice(),
            ],
        ),
    ] {
        for source in sources {
            let tree = accepted(source);
            let extends = first(&tree, |kind| matches!(kind, NodeKind::Extends { .. }));

            let NodeKind::Extends { superclass, .. } = tree.node(extends).kind else {
                panic!("expected extends node");
            };

            assert!(match reference {
                "Parent" => matches!(tree.node(superclass).kind, NodeKind::Name { .. }),
                "classes.Parent" => matches!(tree.node(superclass).kind, NodeKind::Field { .. }),
                _ => matches!(tree.node(superclass).kind, NodeKind::Index { .. }),
            });

            assert_eq!(tree.text(superclass), reference.as_bytes());
        }
    }

    for (attributes, source) in [
        (
            "@checked",
            include_bytes!("../fixtures/parser/classes/attributes/single.luau").as_slice(),
        ),
        (
            "@[checked, deprecated({reason = 'old'})]",
            include_bytes!("../fixtures/parser/classes/attributes/group.luau").as_slice(),
        ),
    ] {
        let tree = accepted(source);
        let method = first(&tree, |kind| matches!(kind, NodeKind::Function { .. }));

        let NodeKind::Function {
            attributes: Some(attributes_node),
            name: Some(name),
            ..
        } = tree.node(method).kind
        else {
            panic!("expected function node");
        };

        let NodeKind::Attributes { .. } = tree.node(attributes_node).kind else {
            panic!("expected attributes node");
        };

        assert_eq!(tree.text(attributes_node), attributes.as_bytes());
        assert_eq!(tree.text(name), b"get");
    }

    accepted(include_bytes!("../fixtures/parser/classes/context.luau").as_slice());

    for source in [
        include_bytes!("../fixtures/parser/classes/invalid/bracket.luau").as_slice(),
        include_bytes!("../fixtures/parser/classes/invalid/index.luau").as_slice(),
        include_bytes!("../fixtures/parser/classes/invalid/attribute.luau").as_slice(),
        include_bytes!("../fixtures/parser/classes/invalid/orphan.luau").as_slice(),
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

#[test]
fn access_types_are_structured() {
    for (access, source, fields) in [
        (
            "read",
            include_bytes!("../fixtures/parser/access/read/array.luau").as_slice(),
            [
                (
                    "value: number",
                    [
                        include_bytes!("../fixtures/parser/access/read/name.luau").as_slice(),
                        include_bytes!("../fixtures/parser/access/read/external.luau").as_slice(),
                    ]
                    .as_slice(),
                ),
                (
                    "['value']: number",
                    [include_bytes!("../fixtures/parser/access/read/string.luau").as_slice()]
                        .as_slice(),
                ),
                (
                    "[string]: number",
                    [include_bytes!("../fixtures/parser/access/read/indexer.luau").as_slice()]
                        .as_slice(),
                ),
            ],
        ),
        (
            "write",
            include_bytes!("../fixtures/parser/access/write/array.luau").as_slice(),
            [
                (
                    "value: number",
                    [
                        include_bytes!("../fixtures/parser/access/write/name.luau").as_slice(),
                        include_bytes!("../fixtures/parser/access/write/external.luau").as_slice(),
                    ]
                    .as_slice(),
                ),
                (
                    "['value']: number",
                    [include_bytes!("../fixtures/parser/access/write/string.luau").as_slice()]
                        .as_slice(),
                ),
                (
                    "[string]: number",
                    [include_bytes!("../fixtures/parser/access/write/indexer.luau").as_slice()]
                        .as_slice(),
                ),
            ],
        ),
    ] {
        let tree = accepted(source);
        let table = first(&tree, |kind| matches!(kind, NodeKind::TypeTable { .. }));

        let NodeKind::TypeTable {
            access: Some(access_token),
            element: Some(element),
            ..
        } = tree.node(table).kind
        else {
            panic!("expected type table node");
        };

        assert_eq!(
            tree.token(access_token).bytes(tree.source),
            access.as_bytes()
        );

        assert_eq!(tree.text(element), b"number");

        for (field, sources) in fields {
            for source in sources {
                let tree = accepted(source);

                let field_node = first(&tree, |kind| {
                    if field == "[string]: number" {
                        matches!(kind, NodeKind::TypeIndexer { .. })
                    } else {
                        matches!(kind, NodeKind::TypeField { .. })
                    }
                });

                let (NodeKind::TypeField {
                    access: Some(access_token),
                    ..
                }
                | NodeKind::TypeIndexer {
                    access: Some(access_token),
                    ..
                }) = tree.node(field_node).kind
                else {
                    panic!("expected access-qualified type field or indexer node");
                };

                assert_eq!(
                    tree.token(access_token).bytes(tree.source),
                    access.as_bytes()
                );
            }
        }
    }

    for source in [
        include_bytes!("../fixtures/parser/access/group.luau").as_slice(),
        include_bytes!("../fixtures/parser/access/array.luau").as_slice(),
        include_bytes!("../fixtures/parser/access/names.luau").as_slice(),
        include_bytes!("../fixtures/parser/access/fields.luau").as_slice(),
        include_bytes!("../fixtures/parser/access/key.luau").as_slice(),
        include_bytes!("../fixtures/parser/access/external.luau").as_slice(),
    ] {
        accepted(source);
    }
}

#[test]
fn enabled_syntax() {
    for source in [
        include_bytes!("../fixtures/parser/enabled/inlining.luau").as_slice(),
        include_bytes!("../fixtures/parser/enabled/anonymous.luau").as_slice(),
        include_bytes!("../fixtures/parser/enabled/declaration.luau").as_slice(),
        include_bytes!("../fixtures/parser/enabled/external.luau").as_slice(),
        include_bytes!("../fixtures/parser/enabled/local.luau").as_slice(),
        include_bytes!("../fixtures/parser/enabled/constant.luau").as_slice(),
        include_bytes!("../fixtures/parser/enabled/export.luau").as_slice(),
        include_bytes!("../fixtures/parser/enabled/metaprogram.luau").as_slice(),
        include_bytes!("../fixtures/parser/enabled/class.luau").as_slice(),
        include_bytes!("../fixtures/parser/enabled/namespace.luau").as_slice(),
        include_bytes!("../fixtures/parser/enabled/condition.luau").as_slice(),
        include_bytes!("../fixtures/parser/enabled/optional.luau").as_slice(),
        include_bytes!("../fixtures/parser/enabled/union.luau").as_slice(),
        include_bytes!("../fixtures/parser/enabled/variadic.luau").as_slice(),
        include_bytes!("../fixtures/parser/enabled/generic.luau").as_slice(),
        include_bytes!("../fixtures/parser/enabled/callback.luau").as_slice(),
        include_bytes!("../fixtures/parser/enabled/instantiation.luau").as_slice(),
        include_bytes!("../fixtures/parser/enabled/method.luau").as_slice(),
    ] {
        accepted(source);
    }

    for source in [
        include_bytes!("../fixtures/parser/numbers/prefix/binary.luau").as_slice(),
        include_bytes!("../fixtures/parser/numbers/prefix/integer.luau").as_slice(),
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

#[test]
fn empty_type_arguments() {
    for source in [
        include_bytes!("../fixtures/parser/arguments/empty/alias.luau").as_slice(),
        include_bytes!("../fixtures/parser/arguments/empty/namespace.luau").as_slice(),
        include_bytes!("../fixtures/parser/arguments/empty/annotation.luau").as_slice(),
        include_bytes!("../fixtures/parser/arguments/empty/call.luau").as_slice(),
        include_bytes!("../fixtures/parser/arguments/empty/reference.luau").as_slice(),
        include_bytes!("../fixtures/parser/arguments/empty/method.luau").as_slice(),
    ] {
        let tree = accepted(source);
        let arguments_node = first(&tree, |kind| matches!(kind, NodeKind::TypeArguments { .. }));

        let NodeKind::TypeArguments { arguments, .. } = &tree.node(arguments_node).kind else {
            panic!("expected type arguments node");
        };

        let arguments = tree.list(arguments);
        assert!(arguments.is_empty(), "{:?}: {arguments:?}", tree.source);
        assert_eq!(tree.text(arguments_node), b"<>");
    }
}

#[test]
fn exported_functions_are_structured() {
    for source in [
        include_bytes!("../fixtures/parser/exports/function.luau").as_slice(),
        include_bytes!("../fixtures/parser/exports/attributed.luau").as_slice(),
    ] {
        let tree = accepted(source);
        let export = first(&tree, |kind| matches!(kind, NodeKind::Export { .. }));

        let NodeKind::Export { declaration, .. } = tree.node(export).kind else {
            panic!("expected export node");
        };

        let NodeKind::Function {
            name: Some(name), ..
        } = tree.node(declaration).kind
        else {
            panic!("expected function node");
        };

        let NodeKind::Name { .. } = tree.node(name).kind else {
            panic!("expected name node");
        };

        assert_eq!(tree.text(name), b"identity");
    }
}

#[test]
fn declaration_type_context() {
    for source in [
        include_bytes!("../fixtures/parser/declarations/accepted/function.luau").as_slice(),
        include_bytes!("../fixtures/parser/declarations/accepted/nested.luau").as_slice(),
        include_bytes!("../fixtures/parser/declarations/accepted/access.luau").as_slice(),
        include_bytes!("../fixtures/parser/declarations/accepted/optional.luau").as_slice(),
    ] {
        accepted(source);
    }

    for source in [
        include_bytes!("../fixtures/parser/declarations/invalid/alias.luau").as_slice(),
        include_bytes!("../fixtures/parser/declarations/invalid/local.luau").as_slice(),
        include_bytes!("../fixtures/parser/declarations/invalid/union.luau").as_slice(),
        include_bytes!("../fixtures/parser/declarations/invalid/prefix.luau").as_slice(),
        include_bytes!("../fixtures/parser/declarations/invalid/group.luau").as_slice(),
        include_bytes!("../fixtures/parser/declarations/invalid/indexer.luau").as_slice(),
        include_bytes!("../fixtures/parser/declarations/invalid/field.luau").as_slice(),
        include_bytes!("../fixtures/parser/declarations/invalid/array.luau").as_slice(),
        include_bytes!("../fixtures/parser/declarations/invalid/argument.luau").as_slice(),
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

#[test]
fn types_are_structured() {
    let tree = accepted(include_bytes!("../fixtures/parser/types/structure.luau").as_slice());
    first(&tree, |kind| matches!(kind, NodeKind::TypeAlias { .. }));
    first(&tree, |kind| matches!(kind, NodeKind::Generics { .. }));
    first(&tree, |kind| matches!(kind, NodeKind::TypeTable { .. }));
    first(&tree, |kind| matches!(kind, NodeKind::TypeField { .. }));
    first(&tree, |kind| matches!(kind, NodeKind::TypeOptional { .. }));
    first(&tree, |kind| matches!(kind, NodeKind::TypeFunction { .. }));
    first(&tree, |kind| matches!(kind, NodeKind::TypePack { .. }));

    let tree = accepted(include_bytes!("../fixtures/parser/types/instantiation.luau").as_slice());
    let call = first(&tree, |kind| matches!(kind, NodeKind::Call { .. }));

    let NodeKind::Call { callee, .. } = tree.node(call).kind else {
        panic!("expected call node");
    };

    let NodeKind::Instantiate { .. } = tree.node(callee).kind else {
        panic!("expected instantiate node");
    };
}

#[test]
fn numeric_literals() {
    for source in [
        include_bytes!("../fixtures/parser/numbers/accepted/decimal.luau").as_slice(),
        include_bytes!("../fixtures/parser/numbers/accepted/separated.luau").as_slice(),
        include_bytes!("../fixtures/parser/numbers/accepted/fraction.luau").as_slice(),
        include_bytes!("../fixtures/parser/numbers/accepted/precision.luau").as_slice(),
        include_bytes!("../fixtures/parser/numbers/accepted/exponent.luau").as_slice(),
        include_bytes!("../fixtures/parser/numbers/accepted/exponentiation.luau").as_slice(),
        include_bytes!("../fixtures/parser/numbers/accepted/hexadecimal.luau").as_slice(),
        include_bytes!("../fixtures/parser/numbers/accepted/hexadecimals.luau").as_slice(),
        include_bytes!("../fixtures/parser/numbers/accepted/binary.luau").as_slice(),
        include_bytes!("../fixtures/parser/numbers/accepted/binaries.luau").as_slice(),
    ] {
        accepted(source);
    }

    for source in [
        include_bytes!("../fixtures/parser/numbers/invalid/exponent.luau").as_slice(),
        include_bytes!("../fixtures/parser/numbers/invalid/separated.luau").as_slice(),
        include_bytes!("../fixtures/parser/numbers/invalid/hexadecimal.luau").as_slice(),
        include_bytes!("../fixtures/parser/numbers/invalid/hexadecimals.luau").as_slice(),
        include_bytes!("../fixtures/parser/numbers/invalid/binary.luau").as_slice(),
        include_bytes!("../fixtures/parser/numbers/invalid/binaries.luau").as_slice(),
        include_bytes!("../fixtures/parser/numbers/invalid/fraction.luau").as_slice(),
        include_bytes!("../fixtures/parser/numbers/invalid/precision.luau").as_slice(),
        include_bytes!("../fixtures/parser/numbers/invalid/decimal.luau").as_slice(),
        include_bytes!("../fixtures/parser/numbers/invalid/decimals.luau").as_slice(),
        include_bytes!("../fixtures/parser/numbers/invalid/integer.luau").as_slice(),
        include_bytes!("../fixtures/parser/numbers/invalid/integers.luau").as_slice(),
        include_bytes!("../fixtures/parser/numbers/invalid/suffix.luau").as_slice(),
        include_bytes!("../fixtures/parser/numbers/invalid/suffix/decimal.luau").as_slice(),
        include_bytes!("../fixtures/parser/numbers/invalid/suffix/separated.luau").as_slice(),
        include_bytes!("../fixtures/parser/numbers/invalid/suffix/maximum.luau").as_slice(),
        include_bytes!("../fixtures/parser/numbers/invalid/suffix/grouped.luau").as_slice(),
        include_bytes!("../fixtures/parser/numbers/invalid/suffix/bits.luau").as_slice(),
        include_bytes!("../fixtures/parser/numbers/invalid/suffix/groups.luau").as_slice(),
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

#[test]
fn malformed_syntax() {
    for source in [
        include_bytes!("../fixtures/parser/malformed/semicolon.luau").as_slice(),
        include_bytes!("../fixtures/parser/malformed/repeated.luau").as_slice(),
        include_bytes!("../fixtures/parser/malformed/namespace.luau").as_slice(),
        include_bytes!("../fixtures/parser/malformed/method.luau").as_slice(),
        include_bytes!("../fixtures/parser/malformed/generic.luau").as_slice(),
        include_bytes!("../fixtures/parser/malformed/attributed.luau").as_slice(),
        include_bytes!("../fixtures/parser/malformed/access.luau").as_slice(),
        include_bytes!("../fixtures/parser/malformed/duplicate.luau").as_slice(),
        include_bytes!("../fixtures/parser/malformed/argument.luau").as_slice(),
        include_bytes!("../fixtures/parser/malformed/instantiation.luau").as_slice(),
        include_bytes!("../fixtures/parser/malformed/alias.luau").as_slice(),
        include_bytes!("../fixtures/parser/malformed/function.luau").as_slice(),
        include_bytes!("../fixtures/parser/malformed/binding.luau").as_slice(),
        include_bytes!("../fixtures/parser/malformed/initializer.luau").as_slice(),
        include_bytes!("../fixtures/parser/malformed/call.luau").as_slice(),
        include_bytes!("../fixtures/parser/malformed/parameter.luau").as_slice(),
        include_bytes!("../fixtures/parser/malformed/field.luau").as_slice(),
        include_bytes!("../fixtures/parser/malformed/pack.luau").as_slice(),
        include_bytes!("../fixtures/parser/malformed/combination.luau").as_slice(),
        include_bytes!("../fixtures/parser/malformed/intersection.luau").as_slice(),
        include_bytes!("../fixtures/parser/malformed/optional.luau").as_slice(),
        include_bytes!("../fixtures/parser/malformed/order.luau").as_slice(),
        include_bytes!("../fixtures/parser/malformed/default.luau").as_slice(),
        include_bytes!("../fixtures/parser/malformed/group.luau").as_slice(),
        include_bytes!("../fixtures/parser/malformed/key.luau").as_slice(),
        include_bytes!("../fixtures/parser/malformed/hexadecimal.luau").as_slice(),
        include_bytes!("../fixtures/parser/malformed/binary.luau").as_slice(),
        include_bytes!("../fixtures/parser/malformed/exponent.luau").as_slice(),
        include_bytes!("../fixtures/parser/malformed/integer.luau").as_slice(),
        include_bytes!("../fixtures/parser/malformed/escape.luau").as_slice(),
        include_bytes!("../fixtures/parser/malformed/decimal.luau").as_slice(),
        include_bytes!("../fixtures/parser/malformed/unicode.luau").as_slice(),
        include_bytes!("../fixtures/parser/malformed/scalar.luau").as_slice(),
        include_bytes!("../fixtures/parser/malformed/parenthesis.luau").as_slice(),
        include_bytes!("../fixtures/parser/malformed/conditional.luau").as_slice(),
        include_bytes!("../fixtures/parser/malformed/assignment.luau").as_slice(),
        include_bytes!("../fixtures/parser/malformed/trailing.luau").as_slice(),
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

#[test]
fn nesting_boundary() {
    for depth in [255, 256] {
        accepted(format!("{}{}", "do ".repeat(depth), "end ".repeat(depth)).as_bytes());
    }

    let source = format!("{}{}", "do ".repeat(257), "end ".repeat(257));
    let tree = check(source.as_bytes());

    assert!(
        tree.diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message == "syntax nesting limit exceeded")
    );
}

#[test]
fn recovery_and_bytes() {
    let tree = check(include_bytes!("../fixtures/parser/recovery/bytes.luau"));

    assert!(
        !tree.diagnostics.is_empty(),
        "{:?}: {:?}",
        tree.source,
        tree.diagnostics
    );

    assert!(
        tree.nodes
            .iter()
            .any(|node| matches!(node.kind, NodeKind::Local { .. })
                && node.span.bytes(tree.source) == b"local valid = '\xff'")
    );

    first(&tree, |kind| matches!(kind, NodeKind::Return { .. }));

    for first in u8::MIN..=u8::MAX {
        for second in u8::MIN..=u8::MAX {
            check(&[first, second]);
        }
    }

    for source in [
        include_bytes!("../fixtures/parser/recovery/prefixes/conditional.luau").as_slice(),
        include_bytes!("../fixtures/parser/recovery/prefixes/generic.luau").as_slice(),
        include_bytes!("../fixtures/parser/recovery/prefixes/interpolation.luau").as_slice(),
    ] {
        for end in 0..=source.len() {
            check(&source[..end]);
        }
    }

    let deep = format!("return {}value{}", "(".repeat(1000), ")".repeat(1000));
    let tree = check(deep.as_bytes());

    assert!(
        !tree.diagnostics.is_empty(),
        "{:?}: {:?}",
        tree.source,
        tree.diagnostics
    );

    for source in [
        format!("{}{}", "do ".repeat(1001), "end ".repeat(1001)),
        format!(
            "type Value = {}number{}",
            "{field: ".repeat(1000),
            "}".repeat(1000)
        ),
        format!("return {}value", "if ready then value else ".repeat(1000)),
        format!(
            "return {}1{}",
            "function() return ".repeat(1000),
            " end".repeat(1000)
        ),
    ] {
        let tree = check(source.as_bytes());

        assert!(
            !tree.diagnostics.is_empty(),
            "{:?}: {:?}",
            tree.source,
            tree.diagnostics
        );
    }

    accepted(format!("return value{}", ".field".repeat(1000)).as_bytes());
}
