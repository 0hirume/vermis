use std::sync::{Arc, atomic::AtomicBool};

use vermis::{Control, Edit, EditError, Limits, ParseError, Resource, Span, parse};

#[test]
fn controlled_updates_preserve_losslessness_and_report_cancellation() {
    let source = b"local value = f(1 + 2)\n";
    let original = parse(b"");
    let tree = original
        .update_with(Span { start: 0, end: 0 }, source, &Control::default())
        .unwrap();
    assert_eq!(tree.root().text(), source);

    assert_eq!(
        tree.tokens()
            .flat_map(|token| token.text().iter().copied())
            .collect::<Vec<_>>(),
        source
    );

    let control = Control {
        cancellation: Some(Arc::new(AtomicBool::new(true))),
        ..Control::default()
    };

    assert_eq!(
        original
            .update_with(Span { start: 0, end: 0 }, source, &control)
            .unwrap_err(),
        EditError::Parse(ParseError::Cancelled)
    );

    assert_eq!(
        tree.update_with(Span { start: 0, end: 0 }, b"", &control)
            .unwrap_err(),
        EditError::Parse(ParseError::Cancelled)
    );

    assert_eq!(
        tree.update_many_with(&[], &control).unwrap_err(),
        EditError::Parse(ParseError::Cancelled)
    );
}

#[test]
fn updates_enforce_snapshot_limits() {
    let source = b"local broken =\nlocal function run()\nreturn f(1 + 2)\nend\nlocal tail = 3";
    let tree = parse(source);
    assert_ne!(tree.diagnostics(), []);
    let position = source.iter().rposition(|byte| *byte == b'3').unwrap();

    let range = Span {
        start: position,
        end: position + 1,
    };

    for (limits, resource) in [
        (
            Limits {
                source_bytes: Some(source.len() - 1),
                ..Limits::default()
            },
            Resource::SourceBytes,
        ),
        (
            Limits {
                tokens: Some(1),
                ..Limits::default()
            },
            Resource::Tokens,
        ),
        (
            Limits {
                nodes: Some(1),
                ..Limits::default()
            },
            Resource::Nodes,
        ),
        (
            Limits {
                diagnostics: Some(0),
                ..Limits::default()
            },
            Resource::Diagnostics,
        ),
        (
            Limits {
                depth: Some(1),
                ..Limits::default()
            },
            Resource::Depth,
        ),
    ] {
        let control = Control {
            limits,
            ..Control::default()
        };

        assert_eq!(
            tree.update_with(range, b"4", &control).unwrap_err(),
            EditError::Parse(ParseError::Limit(resource))
        );

        assert_eq!(tree.source(), source);
    }
}

#[test]
fn batches_share_one_request_ledger() {
    let source = b"local one = 1\nlocal two = 2";
    let tree = parse(source);
    let final_tree = parse(b"return one = 1\nreturn two = 2");

    let control = Control {
        limits: Limits {
            tokens: Some(final_tree.tokens().count()),
            ..Limits::default()
        },
        ..Control::default()
    };

    let edits = [
        Edit {
            range: Span { start: 0, end: 5 },
            replacement: b"return".to_vec(),
        },
        Edit {
            range: Span { start: 14, end: 19 },
            replacement: b"return".to_vec(),
        },
    ];

    for edit in &edits {
        tree.update_with(edit.range, &edit.replacement, &control)
            .unwrap();
    }

    assert_eq!(
        tree.update_many_with(&edits, &control).unwrap_err(),
        EditError::Parse(ParseError::Limit(Resource::Tokens))
    );

    assert_eq!(tree.source(), source);
}
