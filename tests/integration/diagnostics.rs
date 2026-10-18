//! Diagnostic ordering and source range tests.

use vermis::token::Span;
use vermis::tree::{Diagnostic, Tree};

use crate::support::check;

#[test]
fn diagnostics_preserve_emission_order() {
    let tree = check(include_bytes!("../fixtures/diagnostics/emission.luau"));

    assert_eq!(
        tree.diagnostics,
        [
            Diagnostic {
                span: Span { start: 6, end: 7 },
                message: "expected closing expression"
            },
            Diagnostic {
                span: Span { start: 0, end: 5 },
                message: "invalid assignment target"
            },
            Diagnostic {
                span: Span { start: 7, end: 7 },
                message: "expected expression"
            },
        ]
    );
}

#[test]
fn nested_diagnostics_keep_source_order() {
    for (source, expected) in [
        (
            include_bytes!("../fixtures/diagnostics/order/later.luau").as_slice(),
            [
                "statement follows a block-ending statement",
                "malformed number",
            ]
            .as_slice(),
        ),
        (
            include_bytes!("../fixtures/diagnostics/order/return.luau").as_slice(),
            [
                "malformed number",
                "statement follows a block-ending statement",
                "malformed number",
            ]
            .as_slice(),
        ),
        (
            include_bytes!("../fixtures/diagnostics/order/middle.luau").as_slice(),
            [
                "statement follows a block-ending statement",
                "malformed number",
                "malformed number",
            ]
            .as_slice(),
        ),
    ] {
        assert_eq!(messages(&check(source)), expected);
    }
}

#[test]
fn diagnostic_spans_track_source_offsets() {
    let original = check(include_bytes!(
        "../fixtures/diagnostics/offsets/original.luau"
    ));

    let shifted = check(include_bytes!(
        "../fixtures/diagnostics/offsets/shifted.luau"
    ));

    assert_eq!(
        messages(&original),
        ["malformed number", "malformed string escape"]
    );

    assert_eq!(shifted.diagnostics.len(), original.diagnostics.len());

    for (before, after) in original.diagnostics.iter().zip(&shifted.diagnostics) {
        assert_eq!(before.message, after.message);
        assert_eq!(after.span.start, before.span.start + 5);
        assert_eq!(after.span.end, before.span.end + 5);
    }

    assert_eq!(
        shifted.diagnostics,
        [
            Diagnostic {
                span: Span { start: 36, end: 38 },
                message: "malformed number"
            },
            Diagnostic {
                span: Span { start: 55, end: 61 },
                message: "malformed string escape"
            },
        ]
    );
}

#[test]
fn incomplete_syntax_has_zero_width_diagnostics() {
    let tree = check(include_bytes!("../fixtures/diagnostics/incomplete.luau"));

    assert!(
        !tree.diagnostics.is_empty(),
        "{:?}: {:?}",
        tree.source,
        tree.diagnostics
    );

    assert!(
        tree.diagnostics
            .iter()
            .any(|diagnostic| diagnostic.span.is_empty())
    );

    let tree = check(include_bytes!("../fixtures/diagnostics/completed.luau"));

    assert!(
        tree.diagnostics.is_empty(),
        "{:?}: {:?}",
        tree.source,
        tree.diagnostics
    );
}

#[test]
fn identical_messages_at_distinct_anchors_remain_independent() {
    let original = check(include_bytes!(
        "../fixtures/diagnostics/anchors/original.luau"
    ));

    assert_eq!(
        messages(&original),
        ["malformed number", "malformed number"]
    );

    let records = &original.diagnostics;

    assert_eq!(
        records[0].span.bytes(original.source).len(),
        records[1].span.bytes(original.source).len()
    );

    assert_ne!(records[0].span.start, records[1].span.start);

    let repaired = check(include_bytes!(
        "../fixtures/diagnostics/anchors/repaired.luau"
    ));

    assert_eq!(messages(&repaired), ["malformed number"]);
    assert_eq!(repaired.diagnostics[0].span, records[1].span);

    let tree = check(include_bytes!(
        "../fixtures/diagnostics/anchors/completed.luau"
    ));

    assert!(
        tree.diagnostics.is_empty(),
        "{:?}: {:?}",
        tree.source,
        tree.diagnostics
    );
}

fn messages(tree: &Tree<'_>) -> Vec<&'static str> {
    tree.diagnostics
        .iter()
        .map(|diagnostic| diagnostic.message)
        .collect()
}
