use vermis::{Element, Kind, Span, Token, TokenKind, Tree, View, parse};

fn coverage(tree: &Tree) {
    let root = tree.root();
    let tokens: Vec<_> = tree.tokens().collect();
    let mut pending = vec![Element::Node(root)];
    let mut observed = Vec::new();
    let mut nodes = Vec::new();

    while let Some(element) = pending.pop() {
        match element {
            Element::Node(view) => {
                nodes.push(view);
                let elements: Vec<_> = view.elements().collect();

                let children: Vec<_> = elements
                    .iter()
                    .filter_map(|element| match element {
                        Element::Node(child) => Some(*child),
                        Element::Token(_) => None,
                    })
                    .collect();

                assert_eq!(view.children().collect::<Vec<_>>(), children);

                for element in &elements {
                    match element {
                        Element::Node(child) => assert_eq!(child.parent(), Some(view)),
                        Element::Token(token) => assert_eq!(token.parent(), view),
                    }
                }

                pending.extend(elements.into_iter().rev());
            }

            Element::Token(token) => observed.push(token),
        }
    }

    assert_eq!(observed, tokens);
    assert_eq!(root.descendants().collect::<Vec<_>>(), nodes);

    let restored: Vec<_> = observed
        .iter()
        .flat_map(|token| token.text().iter().copied())
        .collect();

    assert_eq!(restored, tree.source());
    assert_eq!(root.text(), tree.source());

    assert_eq!(
        tokens
            .iter()
            .filter(|token| token.kind() == TokenKind::Eof)
            .count(),
        1
    );

    let mut end = 0;

    for (position, token) in tokens.iter().enumerate() {
        assert_eq!(token.span().start, end);
        end = token.span().end;
        assert_eq!(token.text(), token.span().bytes(tree.source()));

        assert_eq!(
            token.data(),
            Token {
                kind: token.kind(),
                span: token.span()
            }
        );

        assert_eq!(
            token.previous(),
            position.checked_sub(1).map(|previous| tokens[previous])
        );

        assert_eq!(token.next(), tokens.get(position + 1).copied());
    }

    assert_eq!(end, tree.source().len());
    let eof = *tokens.last().unwrap();
    assert_eq!(eof.kind(), TokenKind::Eof);
    assert_eq!(eof.parent(), root);
    assert_eq!(eof.span(), Span { start: end, end });
    assert_eq!(eof.text(), []);

    for view in root.descendants() {
        let span = view.span();

        let contained: Vec<_> = tokens
            .iter()
            .copied()
            .filter(|token| {
                if token.kind() == TokenKind::Eof {
                    view == root
                } else {
                    span.start <= token.span().start && token.span().end <= span.end
                }
            })
            .collect();

        assert_eq!(view.tokens().collect::<Vec<_>>(), contained);
    }

    let mut iterator = tree.tokens();
    let saved = iterator.clone();
    assert_eq!(iterator.len(), tokens.len());
    assert_eq!(iterator.next_back(), Some(eof));
    assert_eq!(iterator.len(), tokens.len() - 1);
    assert_eq!(saved.collect::<Vec<_>>(), tokens);
    iterator.by_ref().for_each(drop);
    assert_eq!(iterator.len(), 0);
    assert!(iterator.next().is_none());
    assert!(iterator.next_back().is_none());
    assert!(iterator.next().is_none());
}

#[test]
fn snapshots() {
    let original = b"local value = 1\nreturn value";
    let mut source = original.to_vec();
    let tree = parse(&source);
    let independent = parse(&source);
    source.fill(0);
    drop(source);

    assert_eq!(tree.source(), original);
    assert_eq!(tree.root().text(), original);
    assert_ne!(tree.root(), independent.root());

    let node = tree
        .root()
        .descendants()
        .find(|view| view.kind() == Kind::Local)
        .unwrap();

    let other = independent
        .root()
        .descendants()
        .find(|view| view.kind() == Kind::Local)
        .unwrap();

    assert_eq!(node.kind(), other.kind());
    assert_eq!(node.span(), other.span());
    assert_ne!(node, other);
    assert_ne!(tree.tokens().next(), independent.tokens().next());

    let transferred = std::thread::spawn(move || tree).join().unwrap();
    assert_eq!(transferred.source(), original);
    coverage(&transferred);
}

#[test]
fn relationships() {
    let tree = parse(b"return first, second + offset, third");
    let root: View<'_> = tree.root();
    let block = root.children().next().unwrap();
    let statement = block.children().next().unwrap();
    assert_eq!(statement.kind(), Kind::Return);
    assert_eq!(root.parent(), None);
    assert_eq!(root.previous_sibling(), None);
    assert_eq!(root.next_sibling(), None);
    assert_eq!(block.parent(), Some(root));
    assert_eq!(statement.parent(), Some(block));

    let mut children = statement.children();
    let saved = children.clone();
    assert_eq!(children.len(), 3);
    let first = children.next().unwrap();
    let third = children.next_back().unwrap();
    assert_eq!(children.len(), 1);
    let second = children.next().unwrap();
    assert!(children.next().is_none() && children.next_back().is_none());

    assert_eq!(
        saved.map(View::text).collect::<Vec<_>>(),
        [b"first".as_slice(), b"second + offset", b"third"]
    );

    for (view, previous, next) in [
        (first, None, Some(second)),
        (second, Some(first), Some(third)),
        (third, Some(second), None),
    ] {
        assert_eq!(view.parent(), Some(statement));
        assert_eq!(view.previous_sibling(), previous);
        assert_eq!(view.next_sibling(), next);

        assert_eq!(
            view.ancestors().collect::<Vec<_>>(),
            [view, statement, block, root]
        );
    }

    assert_eq!(root.ancestors().collect::<Vec<_>>(), [root]);
    assert_eq!(second.kind(), Kind::Binary);
    let nested: Vec<_> = second.children().collect();

    assert_eq!(
        nested.iter().map(|view| view.text()).collect::<Vec<_>>(),
        [b"second".as_slice(), b"+", b"offset"]
    );

    let expected = [root, block, statement, first, second]
        .into_iter()
        .chain(nested)
        .chain([third])
        .collect::<Vec<_>>();

    let mut descendants = root.descendants();
    let saved = descendants.clone();
    assert_eq!(descendants.by_ref().collect::<Vec<_>>(), expected);
    assert!(descendants.next().is_none());
    assert!(descendants.next().is_none());
    assert_eq!(saved.count(), expected.len());
    assert_eq!(statement.descendants().collect::<Vec<_>>(), expected[2..]);
    coverage(&tree);
}

#[test]
fn positions() {
    let tree = parse(b"-- note\nlocal value = (12 + 3); --[[ tail ]]\n");
    assert!(tree.diagnostics().is_empty(), "{:?}", tree.diagnostics());
    coverage(&tree);
    let tokens: Vec<_> = tree.tokens().collect();

    for token in &tokens {
        assert_eq!(tree.token_at(token.span().start), Some(*token));

        if token.kind() != TokenKind::Eof {
            assert_eq!(tree.token_at(token.span().end - 1), Some(*token));
            assert_eq!(tree.token_at(token.span().end), token.next());
        }
    }

    assert!(
        tokens
            .iter()
            .any(|token| token.kind() == TokenKind::Comment && token.text() == b"-- note")
    );

    assert!(tokens.iter().any(|token| token.kind() == TokenKind::BlockComment && token.text() == b"--[[ tail ]]"));

    assert!(
        tokens
            .iter()
            .any(|token| token.kind() == TokenKind::Whitespace && token.text() == b"\n")
    );

    assert!(
        tokens
            .iter()
            .any(|token| token.kind() == TokenKind::Byte(b';') && token.text() == b";")
    );

    assert_eq!(tree.token_at(tree.source().len()), tokens.last().copied());
    assert_eq!(tree.token_at(tree.source().len() + 1), None);
    assert_eq!(tree.token_at(usize::MAX), None);
}

#[test]
fn ranges() {
    let tree = parse(b"  local value = (first + second)\nreturn value\n ");
    let root = tree.root();
    let block = root.children().next().unwrap();

    let binary = root
        .descendants()
        .find(|view| view.kind() == Kind::Binary)
        .unwrap();

    let first = binary.children().next().unwrap();
    let second = binary.children().next_back().unwrap();
    assert_eq!(tree.node_at(first.span().start), Some(first));
    assert_eq!(tree.node_at(first.span().end - 1), Some(first));
    assert_eq!(tree.node_at(first.span().end), Some(binary));
    assert_eq!(tree.node_at(second.span().start), Some(second));
    assert_eq!(tree.node_at(0), Some(root));
    assert_eq!(tree.node_at(tree.source().len()), Some(root));
    assert_eq!(tree.node_at(tree.source().len() + 1), None);
    assert_eq!(tree.node_at(usize::MAX), None);
    assert_eq!(tree.covering(first.span()), Some(first));
    assert_eq!(tree.covering(binary.span()), Some(binary));

    assert_eq!(
        tree.covering(Span {
            start: first.span().start + 1,
            end: second.span().end - 1
        }),
        Some(binary)
    );

    assert_eq!(tree.covering(block.span()), Some(block));
    assert_eq!(tree.covering(root.span()), Some(root));

    for position in [0, first.span().start, first.span().end, tree.source().len()] {
        assert_eq!(
            tree.covering(Span {
                start: position,
                end: position
            }),
            tree.node_at(position)
        );
    }

    for span in [
        Span { start: 2, end: 1 },
        Span {
            start: 0,
            end: tree.source().len() + 1,
        },
        Span {
            start: tree.source().len() + 1,
            end: tree.source().len() + 1,
        },
        Span {
            start: usize::MAX,
            end: usize::MAX,
        },
    ] {
        assert_eq!(tree.covering(span), None);
    }

    for source in [b"".as_slice(), b" \t-- note\n"] {
        let tree = parse(source);
        coverage(&tree);
        assert_eq!(tree.node_at(source.len()), Some(tree.root()));

        assert_eq!(
            tree.covering(Span {
                start: source.len(),
                end: source.len()
            }),
            Some(tree.root())
        );

        assert_eq!(tree.token_at(source.len()).unwrap().kind(), TokenKind::Eof);
    }
}

#[test]
fn recovery() {
    for tree in [
        parse(b"local broken = '\xff\nlocal valid = 1\nreturn valid"),
        parse(b"\0\xff\xfe\nlocal valid = 1\nreturn valid"),
        parse(b"local broken = ?\nlocal valid = 1\nreturn valid"),
    ] {
        assert_ne!(tree.diagnostics(), []);
        coverage(&tree);

        assert!(
            tree.root()
                .descendants()
                .any(|view| view.kind() == Kind::Local && view.text() == b"local valid = 1")
        );

        assert!(
            tree.root()
                .descendants()
                .any(|view| view.kind() == Kind::Return && view.text() == b"return valid")
        );
    }

    for tree in [
        parse(b"function unfinished()"),
        parse(b"return {function()"),
        parse(b"\xf0\x9f"),
    ] {
        assert_ne!(tree.diagnostics(), []);
        coverage(&tree);
    }
}
