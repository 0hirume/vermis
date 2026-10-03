//! Diagnostic ordering and source range tests.

pub mod support;

use support::check;
use vermis::token::Span;
use vermis::tree::{Diagnostic, Tree};

fn messages(tree: &Tree<'_>) -> Vec<&'static str> {
    tree.diagnostics
        .iter()
        .map(|diagnostic| diagnostic.message)
        .collect()
}

#[test]
fn diagnostics_preserve_emission_order() {
    let tree = check(b"(node =");

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
            "return 1\nlocal later = 0x\n",
            &[
                "statement follows a block-ending statement",
                "malformed number",
            ][..],
        ),
        (
            "return 0x\nlocal later = 0x\n",
            &[
                "malformed number",
                "statement follows a block-ending statement",
                "malformed number",
            ][..],
        ),
        (
            "return 1\nlocal middle = 0x\nlocal later = 0x\n",
            &[
                "statement follows a block-ending statement",
                "malformed number",
                "malformed number",
            ][..],
        ),
    ] {
        assert_eq!(messages(&check(source.as_bytes())), expected);
    }
}

#[test]
fn diagnostic_spans_track_source_offsets() {
    let original = check(b"local first = 1\nlocal broken = 0x\nlocal escaped = '\\256'\n");
    let shifted = check(b"local first = 123456\nlocal broken = 0x\nlocal escaped = '\\256'\n");

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
    let tree = check(b"local value =\nreturn (item");
    assert_ne!(tree.diagnostics, []);

    assert!(
        tree.diagnostics
            .iter()
            .any(|diagnostic| diagnostic.span.is_empty())
    );

    assert_eq!(
        check(b"local longer_value = 1\nreturn (item)").diagnostics,
        []
    );
}

#[test]
fn identical_messages_at_distinct_anchors_remain_independent() {
    let original = check(b"local first = 0x\nlocal second = 0x");

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
    let repaired = check(b"local first = 10\nlocal second = 0x");
    assert_eq!(messages(&repaired), ["malformed number"]);
    assert_eq!(repaired.diagnostics[0].span, records[1].span);

    assert_eq!(
        check(b"local first = 10\nlocal second = 20").diagnostics,
        []
    );
}
