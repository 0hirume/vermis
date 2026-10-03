use std::fmt::Write;

use vermis::{Element, Kind, Parts, Span, TokenKind, View, parse};

#[test]
fn shifted_snapshot_navigation() {
    let original = parse(b"local first = 1\nlocal second = 2\nreturn first, second\n");

    let edited = original
        .update(Span { start: 14, end: 15 }, b"123456")
        .unwrap();

    let Parts::Root { block } = edited.root().parts().unwrap() else {
        panic!()
    };

    let Parts::Block { statements } = block.parts().unwrap() else {
        panic!()
    };

    let statements: Vec<_> = statements.collect();
    assert_eq!(statements.len(), 3);
    assert_eq!(statements[1].text(), b"local second = 2");
    assert_eq!(statements[1].previous_sibling(), Some(statements[0]));
    assert_eq!(statements[1].next_sibling(), Some(statements[2]));
    assert_eq!(statements[1].parent(), Some(block));

    let original_second = original
        .root()
        .children()
        .next()
        .unwrap()
        .children()
        .nth(1)
        .unwrap();

    assert_eq!(original_second.identity(), statements[1].identity());
    assert_ne!(original.root(), edited.root());

    assert_eq!(
        edited.node_at(statements[1].span().start),
        Some(statements[1])
    );

    assert_eq!(
        original.root().text(),
        b"local first = 1\nlocal second = 2\nreturn first, second\n"
    );

    assert_eq!(
        edited.root().text(),
        b"local first = 123456\nlocal second = 2\nreturn first, second\n"
    );

    let bytes: Vec<_> = edited
        .tokens()
        .flat_map(|token| token.text().iter().copied())
        .collect();

    assert_eq!(
        bytes,
        edited
            .source_chunks()
            .flatten()
            .copied()
            .collect::<Vec<_>>()
    );

    let eof = edited.tokens().next_back().unwrap();
    assert_eq!(eof.kind(), TokenKind::Eof);
    assert_eq!(eof.parent(), edited.root());
    assert_eq!(eof.next(), None);
}

#[test]
fn wide_siblings_keep_occurrence_order() {
    let mut source = String::new();

    for number in 0..4096 {
        writeln!(source, "local value{number} = {number}").unwrap();
    }

    let original = parse(source.as_bytes());
    let block = original.root().children().next().unwrap();
    let statements: Vec<_> = block.children().collect();
    let middle = statements[2048];

    let edited = original
        .update(middle.span(), b"local changed = 17")
        .unwrap();

    let block = edited.root().children().next().unwrap();
    assert_eq!(block.children().len(), statements.len());
    let replacement = block.children().nth(2048).unwrap();
    assert_eq!(replacement.kind(), Kind::Local);
    assert_eq!(replacement.text(), b"local changed = 17");

    assert_eq!(
        replacement.previous_sibling().unwrap().text(),
        statements[2047].text()
    );

    assert_eq!(
        replacement.next_sibling().unwrap().text(),
        statements[2049].text()
    );

    let children: Vec<_> = block
        .elements()
        .filter_map(|element| match element {
            Element::Node(node) => Some(node),
            Element::Token(_) => None,
        })
        .collect();

    assert_eq!(children, block.children().collect::<Vec<_>>());

    assert_eq!(
        block.children().rev().map(View::text).collect::<Vec<_>>(),
        children
            .iter()
            .rev()
            .map(|node| node.text())
            .collect::<Vec<_>>()
    );
}

#[test]
fn recovery_positions_are_snapshot_relative() {
    let original = parse(b"local value =");

    let missing = original
        .root()
        .descendants()
        .find(|view| view.kind() == Kind::Missing)
        .unwrap();

    let expectations: Vec<_> = missing.recovery().collect();
    assert_ne!(expectations, []);

    assert!(
        expectations
            .iter()
            .all(|expectation| expectation.span == missing.span())
    );

    let edited = original
        .update(Span { start: 0, end: 0 }, b"-- note\n")
        .unwrap();

    let shifted = edited
        .root()
        .descendants()
        .find(|view| view.kind() == Kind::Missing)
        .unwrap();

    let shifted_expectations: Vec<_> = shifted.recovery().collect();
    assert_eq!(expectations.len(), shifted_expectations.len());

    for (original, shifted) in expectations.iter().zip(&shifted_expectations) {
        assert_eq!(original.expected, shifted.expected);
        assert_eq!(shifted.span.start, original.span.start + 8);
        assert_eq!(shifted.span.end, original.span.end + 8);
    }

    assert_eq!(missing.recovery().collect::<Vec<_>>(), expectations);
}

#[test]
fn deep_postfix_chains_freeze_navigate_and_drop() {
    let calls = 4096;
    let mut source = b"return value".to_vec();

    for _ in 0..calls {
        source.extend_from_slice(b"()");
    }

    let tree = parse(&source);
    assert_eq!(tree.diagnostics(), []);
    assert_eq!(tree.root().descendants().count(), 4 + calls * 2);
    assert_eq!(tree.tokens().count(), 4 + calls * 2);
    let name = tree.node_at(7).unwrap();
    assert_eq!(name.kind(), Kind::Name);
    assert_eq!(name.text(), b"value");
    assert_eq!(tree.root().text(), source);
    let last = tree.token_at(source.len() - 1).unwrap();
    assert_eq!(last.text(), b")");
    assert_eq!(last.next().unwrap().kind(), TokenKind::Eof);
    assert_eq!(last.parent().kind(), Kind::Arguments);
    drop(tree);
}
