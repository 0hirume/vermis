#![no_main]

use libfuzzer_sys::fuzz_target;
use vermis::{
    Diagnostic, Element, Expectation, Kind, Span, Token, TokenKind, TokenView, Tree, View, parse,
};

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

#[derive(Debug, PartialEq, Eq)]
struct Snapshot {
    nodes: Vec<NodeSnapshot>,
    tokens: Vec<TokenSnapshot>,
    lookups: [LookupSnapshot; 3],
    diagnostics: Vec<Diagnostic>,
}

#[derive(Debug, PartialEq, Eq)]
struct NodeSnapshot {
    kind: Kind,
    span: Span,
    text: Vec<u8>,
    parts: String,
    recovery: Vec<Expectation>,
    parent: Option<(Kind, Span)>,
    previous_sibling: Option<(Kind, Span)>,
    next_sibling: Option<(Kind, Span)>,
    children: Vec<(Kind, Span)>,
    elements: Vec<ElementSnapshot>,
    tokens: Vec<Token>,
}

#[derive(Debug, PartialEq, Eq)]
struct TokenSnapshot {
    data: Token,
    text: Vec<u8>,
    parent: (Kind, Span),
    previous: Option<Token>,
    next: Option<Token>,
}

#[derive(Debug, PartialEq, Eq)]
enum ElementSnapshot {
    Node((Kind, Span)),
    Token(Token),
}

#[derive(Debug, PartialEq, Eq)]
struct LookupSnapshot {
    token: Option<Token>,
    node: Option<(Kind, Span)>,
    covering: Option<(Kind, Span)>,
}

fn node_value(node: View<'_>) -> (Kind, Span) {
    (node.kind(), node.span())
}

fn token_value(token: TokenView<'_>) -> Token {
    Token {
        kind: token.kind(),
        span: token.span(),
    }
}

fn snapshot(tree: &Tree) -> Snapshot {
    let nodes = tree
        .root()
        .descendants()
        .map(|node| NodeSnapshot {
            kind: node.kind(),
            span: node.span(),
            text: node.text().to_vec(),
            parts: format!("{:?}", node.parts()),
            recovery: node.recovery().collect(),
            parent: node.parent().map(node_value),
            previous_sibling: node.previous_sibling().map(node_value),
            next_sibling: node.next_sibling().map(node_value),
            children: node.children().map(node_value).collect(),
            elements: node
                .elements()
                .map(|element| match element {
                    Element::Node(child) => ElementSnapshot::Node(node_value(child)),
                    Element::Token(token) => ElementSnapshot::Token(token_value(token)),
                })
                .collect(),
            tokens: node.tokens().map(token_value).collect(),
        })
        .collect();

    let tokens = tree
        .tokens()
        .map(|token| TokenSnapshot {
            data: token.data(),
            text: token.text().to_vec(),
            parent: node_value(token.parent()),
            previous: token.previous().map(token_value),
            next: token.next().map(token_value),
        })
        .collect();

    let lookups = [0, tree.source().len() / 2, tree.source().len()].map(|offset| LookupSnapshot {
        token: tree.token_at(offset).map(token_value),
        node: tree.node_at(offset).map(node_value),
        covering: tree
            .covering(Span {
                start: offset,
                end: offset,
            })
            .map(node_value),
    });

    Snapshot {
        nodes,
        tokens,
        lookups,
        diagnostics: tree.diagnostics().to_vec(),
    }
}

fuzz_target!(|source: &[u8]| {
    let mut tree = parse(source);
    validate(&tree, source);

    if source.len() > 256 {
        return;
    }

    let replacements: &[&[u8]] = &[b"", b" ", b"'", b"--[[", b"x", b"<", b"}", b"end"];
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

        let previous = snapshot(&tree);
        let updated = tree.update(Span { start, end }, replacement).unwrap();
        assert_eq!(snapshot(&tree), previous);

        edited
            .splice(start..end, replacement.iter().copied())
            .for_each(drop);

        validate(&updated, &edited);
        let fresh = parse(&edited);
        validate(&fresh, &edited);
        assert_eq!(snapshot(&updated), snapshot(&fresh));
        tree = updated;
    }
});
