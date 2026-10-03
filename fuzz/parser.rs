#![no_main]

use libfuzzer_sys::fuzz_target;
use vermis::{Element, Span, TokenKind, Tree, parse, parse_luaux};

fn validate(tree: &Tree, source: &[u8]) {
    let root = tree.root();
    let tokens: Vec<_> = tree.tokens().collect();
    let mut end = 0;

    for token in &tokens {
        let span = token.span();
        assert_eq!(span.start, end);
        assert!(span.start <= span.end && span.end <= source.len());
        assert_eq!(token.text(), span.bytes(tree.source()));
        let parent = token.parent();
        assert!(parent.span().start <= span.start && span.end <= parent.span().end);

        if token.kind() == TokenKind::Eof {
            assert_eq!(token.parent(), root);
            assert!(token.next().is_none());
            assert_eq!(span.start, source.len());
        } else {
            assert!(span.start < span.end);
        }

        if let Some(previous) = token.previous() {
            assert_eq!(previous.next(), Some(*token));
        }

        if let Some(next) = token.next() {
            assert_eq!(next.previous(), Some(*token));
        }

        end = span.end;
    }

    assert_eq!(end, source.len());
    assert_eq!(tokens.last().unwrap().kind(), TokenKind::Eof);
    assert_eq!(tree.source(), source);
    assert_eq!(root.text(), source);
    assert_eq!(root.parent(), None);

    assert_eq!(
        tree.source_chunks().flatten().copied().collect::<Vec<_>>(),
        source
    );

    let mut start = 0;

    for node in root.descendants() {
        let span = node.span();
        assert!(start <= span.start && span.start <= span.end && span.end <= source.len());
        assert!(node.parts().is_some(), "{source:?}: {node:?}");
        start = span.start;

        if node != root {
            let parent = node.parent().unwrap();
            assert_eq!(parent.children().filter(|child| *child == node).count(), 1);
        }

        let mut end = span.start;

        for child in node.children() {
            let child_span = child.span();
            assert!(child_span.start >= end && child_span.end <= span.end);
            assert_eq!(child.parent(), Some(node));
            end = child_span.end;
        }
    }

    let mut pending = vec![Element::Node(root)];
    let mut observed_tokens = Vec::new();
    let mut observed_nodes = Vec::new();

    while let Some(element) = pending.pop() {
        match element {
            Element::Node(node) => {
                observed_nodes.push(node);
                let elements: Vec<_> = node.elements().collect();
                let mut end = node.span().start;

                for element in &elements {
                    let span = match element {
                        Element::Node(child) => {
                            assert_eq!(child.parent(), Some(node));

                            child.span()
                        }

                        Element::Token(token) => {
                            assert_eq!(token.parent(), node);

                            token.span()
                        }
                    };

                    assert!(end <= span.start && span.end <= node.span().end);
                    end = span.end;
                }

                pending.extend(elements.into_iter().rev());
            }

            Element::Token(token) => observed_tokens.push(token),
        }
    }

    assert_eq!(observed_nodes, root.descendants().collect::<Vec<_>>());
    assert_eq!(observed_tokens, tokens);

    assert_eq!(
        observed_tokens
            .iter()
            .flat_map(|token| token.text().iter().copied())
            .collect::<Vec<_>>(),
        source
    );

    for diagnostic in tree.diagnostics() {
        assert!(
            diagnostic.span.start <= diagnostic.span.end && diagnostic.span.end <= source.len()
        );
    }
}

fn fingerprint(tree: &Tree) -> String {
    let nodes: Vec<_> = tree
        .root()
        .descendants()
        .map(|node| {
            (
                node.kind(),
                node.span(),
                node.text(),
                node.parts(),
                node.recovery().collect::<Vec<_>>(),
                node.parent(),
                node.previous_sibling(),
                node.next_sibling(),
                node.children().collect::<Vec<_>>(),
                node.elements().collect::<Vec<_>>(),
                node.tokens().collect::<Vec<_>>(),
            )
        })
        .collect();

    let tokens: Vec<_> = tree
        .tokens()
        .map(|token| {
            (
                token.data(),
                token.text(),
                token.parent(),
                token.previous(),
                token.next(),
            )
        })
        .collect();

    let lookups: Vec<_> = [0, tree.source().len() / 2, tree.source().len()]
        .into_iter()
        .map(|offset| {
            (
                tree.token_at(offset),
                tree.node_at(offset),
                tree.covering(Span {
                    start: offset,
                    end: offset,
                }),
            )
        })
        .collect();

    format!(
        "{nodes:?}\n{tokens:?}\n{lookups:?}\n{:?}",
        tree.diagnostics()
    )
}

fuzz_target!(|source: &[u8]| {
    for parser in [parse, parse_luaux] {
        let mut tree = parser(source);
        validate(&tree, source);
        if source.len() > 256 {
            continue;
        }

        let replacements: &[&[u8]] = &[b"", b" ", b"'", b"--[[", b"x", b"<A/>", b"}", b"end"];
        let mut edited = source.to_vec();
        for step in 0..4 {
            let choice = usize::from(source.get(step * 3).copied().unwrap_or(0));
            let start =
                usize::from(source.get(step * 3 + 1).copied().unwrap_or(0)) % (edited.len() + 1);
            let width = usize::from(source.get(step * 3 + 2).copied().unwrap_or(0)) % 5;
            let end = edited.len().min(start + width);
            let replacement = if choice % 2 == 0 {
                replacements[(choice / 2) % replacements.len()]
            } else {
                &source[..source.len().min(4)]
            };
            let previous = fingerprint(&tree);
            let updated = tree.update(Span { start, end }, replacement).unwrap();
            assert_eq!(fingerprint(&tree), previous);
            edited
                .splice(start..end, replacement.iter().copied())
                .for_each(drop);
            validate(&updated, &edited);
            let fresh = parser(&edited);
            validate(&fresh, &edited);
            assert_eq!(fingerprint(&updated), fingerprint(&fresh));
            tree = updated;
        }
    }
});
