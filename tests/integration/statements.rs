//! Statement recovery tests.

use vermis::tree::NodeKind;

use crate::support::check;

#[test]
fn unfinished_function_contains_empty_body() {
    let tree = check(include_bytes!("../fixtures/statements/function.luau"));

    let function = tree
        .nodes
        .iter()
        .find(|node| matches!(node.kind, NodeKind::Function { .. }))
        .expect("expected unfinished function node");

    let NodeKind::Function {
        body: Some(body), ..
    } = function.kind
    else {
        panic!("function must retain its body");
    };

    let body = tree.node(body);
    assert!(body.span.start >= function.span.start);
    assert!(body.span.end <= function.span.end);
    assert!(body.tokens.end.get() <= function.tokens.end.get());

    assert!(
        !tree.diagnostics.is_empty(),
        "{:?}: {:?}",
        tree.source,
        tree.diagnostics
    );
}

#[test]
fn unfinished_attributes_retain_function_structure() {
    let tree = check(include_bytes!("../fixtures/statements/attributes.luau"));

    let function = tree
        .nodes
        .iter()
        .find(|node| {
            matches!(
                node.kind,
                NodeKind::Function {
                    attributes: Some(_),
                    ..
                }
            )
        })
        .expect("attributed function must retain its structure");

    let NodeKind::Function {
        attributes: Some(attributes),
        ..
    } = function.kind
    else {
        panic!("expected attributed function node");
    };

    let NodeKind::Attributes { .. } = tree.node(attributes).kind else {
        panic!("expected attributes node");
    };

    assert!(
        !tree.diagnostics.is_empty(),
        "{:?}: {:?}",
        tree.source,
        tree.diagnostics
    );
}
