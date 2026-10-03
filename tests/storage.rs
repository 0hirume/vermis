//! Indexed storage tests.

pub mod support;

use std::fmt::Write;
use support::{check, first};
use vermis::token::TokenKind;
use vermis::tree::NodeKind;

#[test]
fn wide_siblings_keep_source_order() {
    let mut source = String::new();

    for number in 0..4096 {
        writeln!(source, "local value{number} = {number}").unwrap();
    }

    let tree = check(source.as_bytes());
    assert_eq!(tree.diagnostics, []);

    let NodeKind::Root { block, .. } = tree.node(tree.root).kind else {
        panic!()
    };

    let NodeKind::Block { statements } = &tree.node(block).kind else {
        panic!()
    };

    let statements = tree.list(statements);
    assert_eq!(statements.len(), 4096);

    for (number, statement) in statements.iter().enumerate() {
        assert!(matches!(
            tree.node(statement.node).kind,
            NodeKind::Local { .. }
        ));

        assert_eq!(
            tree.text(statement.node),
            format!("local value{number} = {number}").as_bytes()
        );
    }
}

#[test]
fn deep_postfix_chains_parse_and_drop() {
    let calls = 4096;
    let mut source = b"return value".to_vec();

    for _ in 0..calls {
        source.extend_from_slice(b"()");
    }

    let tree = check(&source);
    assert_eq!(tree.diagnostics, []);
    assert_eq!(tree.nodes.len(), 4 + calls * 2);
    assert_eq!(tree.tokens.len(), 4 + calls * 2);
    let name = first(&tree, |kind| matches!(kind, NodeKind::Name { .. }));
    assert_eq!(tree.text(name), b"value");
    assert_eq!(tree.text(tree.root), source);
    let last = &tree.tokens[tree.tokens.len() - 2];
    assert_eq!(last.bytes(tree.source), b")");
    assert_eq!(tree.tokens.last().unwrap().kind, TokenKind::EndOfFile);
    let call = first(&tree, |kind| matches!(kind, NodeKind::Call { .. }));

    let NodeKind::Call { arguments, .. } = tree.node(call).kind else {
        panic!()
    };

    let NodeKind::Arguments {
        closing: Some(closing),
        ..
    } = tree.node(arguments).kind
    else {
        panic!()
    };

    assert_eq!(tree.token(closing), last);
    drop(tree);
}
