//! Statement recovery tests.

use vermis::parse;
use vermis::tree::NodeKind;

#[test]
fn unfinished_function_contains_empty_body() {
    let tree = parse(b"function f(first: number): number ");

    let function = tree
        .nodes
        .iter()
        .find(|node| matches!(node.kind, NodeKind::Function { .. }))
        .unwrap();

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
    assert_ne!(tree.diagnostics, []);
}

#[test]
fn unfinished_attributes_retain_function_structure() {
    let tree = parse(b"@[deprecated(");

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
        unreachable!();
    };

    assert!(matches!(
        tree.node(attributes).kind,
        NodeKind::Attributes { .. }
    ));

    assert_ne!(tree.diagnostics, []);
}
