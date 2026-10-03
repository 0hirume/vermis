use std::fmt::Write;

use vermis::{EditError, Element, Kind, Span, Tree, parse};

fn fingerprint(tree: &Tree) -> String {
    assert_eq!(
        tree.source_chunks().flatten().copied().collect::<Vec<_>>(),
        tree.source()
    );

    assert_eq!(tree.root().text(), tree.source());

    assert_eq!(
        tree.tokens()
            .flat_map(|token| token.text().iter().copied())
            .collect::<Vec<_>>(),
        tree.source()
    );

    let nodes: Vec<_> = tree.root().descendants().collect();
    let tokens: Vec<_> = tree.tokens().collect();

    let node_index = |node| {
        nodes
            .iter()
            .position(|candidate| *candidate == node)
            .unwrap()
    };

    let token_index = |token| {
        tokens
            .iter()
            .position(|candidate| *candidate == token)
            .unwrap()
    };

    let mut result = format!("{:?}\n{:?}\n", tree.source(), tree.diagnostics());

    for node in &nodes {
        let elements: Vec<_> = node
            .elements()
            .map(|element| match element {
                Element::Node(child) => ("node", node_index(child)),
                Element::Token(token) => ("token", token_index(token)),
            })
            .collect();

        writeln!(
            result,
            "{:?}",
            (
                node.kind(),
                node.span(),
                node.text(),
                node.parts(),
                node.recovery().collect::<Vec<_>>(),
                node.parent().map(node_index),
                node.previous_sibling().map(node_index),
                node.next_sibling().map(node_index),
                node.children().map(node_index).collect::<Vec<_>>(),
                elements,
                node.tokens().map(token_index).collect::<Vec<_>>(),
            )
        )
        .unwrap();

        writeln!(result, "{:?}", tree.covering(node.span()).map(node_index)).unwrap();
    }

    for token in &tokens {
        writeln!(
            result,
            "{:?}",
            (
                token.data(),
                token.text(),
                node_index(token.parent()),
                token.previous().map(token_index),
                token.next().map(token_index),
            )
        )
        .unwrap();
    }

    for offset in (0..=tree.source().len() + 1).chain([usize::MAX]) {
        writeln!(
            result,
            "{:?}",
            (
                tree.token_at(offset).map(token_index),
                tree.node_at(offset).map(node_index),
                tree.covering(Span {
                    start: offset,
                    end: offset
                })
                .map(node_index),
            )
        )
        .unwrap();
    }

    result
}

fn update(tree: &Tree, range: Span, replacement: &[u8]) -> Tree {
    let previous = fingerprint(tree);
    let mut source = tree.source().to_vec();

    source
        .splice(range.start..range.end, replacement.iter().copied())
        .for_each(drop);

    let updated = tree.update(range, replacement).unwrap();
    assert_eq!(updated.source(), source);

    assert_eq!(
        fingerprint(&updated),
        fingerprint(&parse(&source)),
        "source: {:?}, range: {range:?}, replacement: {replacement:?}",
        tree.source()
    );

    assert_eq!(fingerprint(tree), previous);

    updated
}

fn replace(tree: &Tree, needle: &[u8], replacement: &[u8]) -> Tree {
    let start = tree
        .source()
        .windows(needle.len())
        .position(|window| window == needle)
        .unwrap();

    update(
        tree,
        Span {
            start,
            end: start + needle.len(),
        },
        replacement,
    )
}

type Replacements<'source> = &'source [(&'source [u8], &'source [u8])];

#[test]
fn histories() {
    let histories: &[(&[u8], Replacements<'_>)] = &[
        (
            b"local function run(value: number): number\nlocal data = {entry = {value + 1}}\nreturn data.entry[1]\nend\nreturn run(2)",
            &[(b"value + 1", b"(value * 3) - 2"), (b"entry = {", b"entry = {other = 0, "), (b"number", b"number?"), (b"run(2)", b"run({value = 2})"), (b"other = 0, ", b"")],
        ),
        (
            b"type Result<T> = {value: T, transform: (T) -> T}\nlocal value: Result<number> = {value = 1}\nreturn value",
            &[(b"(T) -> T", b"(T, number) -> (T, string)"), (b"value: T", b"value: T?"), (b"Result<number>", b"Result<number | string>"), (b"{value = 1}", b"{value = function() return 1 end}")],
        ),
        (
            b"local value = [[text]]\nlocal following = 2\nreturn following",
            &[(b"]]", b""), (b"text", b"text]]"), (b"[[text]]", b"'text'"), (b"'text'", b"'text"), (b"'text", b"'text'")],
        ),
        (
            b"--[[ note ]]\nlocal following = 2\nreturn following",
            &[(b"]]", b""), (b"note ", b"note ]]"), (b"--[[ note ]]", b"-- note"), (b"-- note\n", b"-- note"), (b"-- note", b"-- note\n")],
        ),
        (
            b"local value = first - -second\nreturn value",
            &[(b"- -", b"--"), (b"--", b"- -"), (b"first - -second", b"first second"), (b"first second", b"firstsecond"), (b"firstsecond", b"first .. second"), (b"..", b"...")],
        ),
        (
            b"local value = `hello {name}`\nreturn value",
            &[(b"name", b"{first = name}.first"), (b"hello", b"hello {1 + 2}"), (b"}`", b"`"), (b".first`", b".first}`")],
        ),
        (
            b"local value = 1\nreturn value\n",
            &[(b"return value", b"return value\nlocal after = 2"), (b"local after = 2", b"after()"), (b"\nafter()", b""), (b"local value = 1", b"local value = 1\nreturn value"), (b"return value\nreturn value", b"return value")],
        ),
    ];

    for &(source, edits) in histories {
        let mut tree = parse(source);

        for &(needle, replacement) in edits {
            tree = replace(&tree, needle, replacement);
        }

        let end = tree.source().len();
        tree = update(&tree, Span { start: end, end }, b"\n-- end");
        let end = tree.source().len();

        update(
            &tree,
            Span {
                start: end - 7,
                end,
            },
            b"",
        );
    }
}

#[test]
fn boundaries() {
    for source in [
        b"".as_slice(),
        b"local x = a + b\nreturn x",
        b"-- note\nreturn {value = 1}",
        b"return `a {b}`",
        b"return '\xc3\xa9'",
    ] {
        let tree = parse(source);

        for start in 0..=source.len() {
            for end in start..=source.len().min(start + 1) {
                for replacement in [b"".as_slice(), b" ", b"x", b"-", b"'", b"\xff"] {
                    update(&tree, Span { start, end }, replacement);
                }
            }
        }
    }
}

#[test]
fn regions() {
    let left = parse(b"local a =\nlocal b = 2\nlocal c = 3\nlocal d = 4\nlocal e = 5");
    assert_ne!(left.diagnostics(), []);
    let left = replace(&left, b"c = 3", b"c = 30");
    let left = replace(&left, b"d = 4", b"d = 40");
    let left = replace(&left, b"c = 30", b"c = 3");
    assert_ne!(left.diagnostics(), []);
    let left = replace(&left, b"local a =\n", b"local a = 10\n");
    assert_eq!(left.diagnostics(), []);

    let right = parse(b"local a = 1\nlocal b =\nlocal c = 3\nlocal d = 4");
    assert_ne!(right.diagnostics(), []);
    let right = replace(&right, b"a = 1", b"a = 2");
    let right = replace(&right, b"d = 4", b"d = 40");
    assert_ne!(right.diagnostics(), []);
    let right = replace(&right, b"local b =\n", b"local b = 20\n");
    assert_eq!(right.diagnostics(), []);

    let comment = parse(b"local before = 1\nlocal value = --[[note");
    let end = comment.source().len();
    let comment = update(&comment, Span { start: end, end }, b" more");
    let comment = replace(&comment, b"note more", b"note more]]");
    let end = comment.source().len();
    let repaired = update(&comment, Span { start: end, end }, b" 1");
    assert_eq!(repaired.diagnostics(), []);

    let interpolation =
        parse(b"local before = 1\nbad `text {function() end} rest\nlocal tail = 2\nlocal last = 3");

    replace(&interpolation, b"text", b"te`xt");

    let string = parse(b"local before = 1\ncall'''\nname'\nlocal tail = 2\nlocal last = 3");
    replace(&string, b"\nname", b"end");

    let lookahead = parse(b"local before = 1\np\n<\nname\nlocal tail = 2\nlocal last = 3");
    replace(&lookahead, b"name", b"<A/>");

    let nested = parse(b"local first = {nested = {value = 1}}\nlocal function helper(value) return {nested = {value}} end\nlocal middle = (2 + 3)\nlocal later: number = 4\nlocal last = helper(middle)");
    assert_eq!(nested.diagnostics(), []);
    let nested = replace(&nested, b"2 + 3", b"20 + 30");
    let nested = replace(&nested, b"later: number", b"later: number?");
    let nested = replace(&nested, b"local middle =", b"return");
    assert_ne!(nested.diagnostics(), []);
    let nested = replace(&nested, b"20 + 30", b"20 * 30");
    assert_ne!(nested.diagnostics(), []);

    let nested = replace(&nested, b"return (20 * 30)", b"local middle = (20 * 30)");

    assert_eq!(nested.diagnostics(), []);
}

#[test]
fn recovery() {
    let original = parse(b"local value = 1\nreturn value");
    assert_eq!(original.diagnostics(), []);
    let incomplete = replace(&original, b"= 1", b"=");
    assert_ne!(incomplete.diagnostics(), []);

    assert!(
        incomplete
            .root()
            .descendants()
            .any(|node| node.kind() == Kind::Missing && node.span().is_empty())
    );

    let repaired = replace(&incomplete, b"=", b"= 2");
    assert_eq!(repaired.diagnostics(), []);

    let after_return = replace(&repaired, b"return value", b"return value\nlocal after = 3");

    assert_ne!(after_return.diagnostics(), []);
}

#[test]
fn snapshots() {
    let original = parse(b"local value = 1\nreturn value");
    let root = original.root();

    let view = root
        .descendants()
        .find(|node| node.kind() == Kind::Local)
        .unwrap();

    let token = original.token_at(14).unwrap();
    let chunks = original.source_chunks().collect::<Vec<_>>();
    let parts = format!("{:?}", view.parts());
    let previous = fingerprint(&original);
    let mut replacement = b"22".to_vec();
    let updated = update(&original, Span { start: 14, end: 15 }, &replacement);
    replacement.fill(0);
    let second = replace(&updated, b"22", b"333");
    drop(updated);
    assert_eq!(second.source(), b"local value = 333\nreturn value");
    assert_eq!(root.text(), b"local value = 1\nreturn value");
    assert_eq!(view.text(), b"local value = 1");
    assert_eq!(token.text(), b"1");
    assert_eq!(format!("{:?}", view.parts()), parts);

    assert_eq!(
        chunks.into_iter().flatten().copied().collect::<Vec<_>>(),
        original.source()
    );

    assert_eq!(fingerprint(&original), previous);

    for range in [
        Span { start: 0, end: 0 },
        view.span(),
        Span {
            start: original.source().len(),
            end: original.source().len(),
        },
    ] {
        update(&original, range, range.bytes(original.source()));
    }

    let deleted = update(
        &second,
        Span {
            start: 0,
            end: second.source().len(),
        },
        b"",
    );

    update(&deleted, Span { start: 0, end: 0 }, original.source());
}

#[test]
fn ranges() {
    for source in [b"".as_slice(), b"return 1"] {
        let tree = parse(source);
        let previous = fingerprint(&tree);

        for range in [
            Span { start: 1, end: 0 },
            Span {
                start: 0,
                end: source.len() + 1,
            },
            Span {
                start: source.len() + 1,
                end: source.len() + 1,
            },
            Span {
                start: usize::MAX,
                end: usize::MAX,
            },
            Span {
                start: 0,
                end: usize::MAX,
            },
        ] {
            assert_eq!(
                tree.update(range, b"replacement").unwrap_err(),
                EditError::InvalidRange,
                "{range:?}"
            );

            assert_eq!(fingerprint(&tree), previous);
        }
    }
}

#[test]
fn parsed_type_parameters_invalidate_parent_classification() {
    let tree = parse(b"declare f: (value: num</A>)//cal tail = 3");
    update(&tree, Span { start: 16, end: 21 }, b"x");
}

#[test]
fn lexical_entry_cannot_split_a_changed_preceding_token() {
    let tree = parse(b"if</Atypeof");
    update(&tree, Span { start: 2, end: 3 }, b"x");
}

#[test]
fn new_condition_errors_keep_their_original_chronological_slot() {
    let tree = parse(b"if const value = f//nd");
    update(&tree, Span { start: 16, end: 17 }, b"</A>");
}
