use vermis::{Diagnostic, DiagnosticCode, Edit, Severity, Span, Tree, parse, parse_luaux};

fn messages(tree: &Tree) -> Vec<&'static str> {
    tree.diagnostics()
        .iter()
        .map(|diagnostic| diagnostic.message)
        .collect()
}

fn replace(tree: &Tree, needle: &[u8], replacement: &[u8], parser: fn(&[u8]) -> Tree) -> Tree {
    let start = tree
        .source()
        .windows(needle.len())
        .position(|bytes| bytes == needle)
        .unwrap();

    let range = Span {
        start,
        end: start + needle.len(),
    };

    let old = tree.diagnostics().to_vec();
    let updated = tree.update(range, replacement).unwrap();
    let mut source = tree.source().to_vec();

    source
        .splice(range.start..range.end, replacement.iter().copied())
        .for_each(drop);

    assert_eq!(updated.source(), source);
    assert_eq!(updated.diagnostics(), parser(&source).diagnostics());
    assert_eq!(tree.diagnostics(), old);

    updated
}

#[test]
fn diagnostics_preserve_emission_order_instead_of_span_order() {
    let source = b"local node = <First></Other";
    let tree = parse_luaux(source);

    assert_eq!(
        messages(&tree),
        [
            "expected end of closing tag",
            "closing tag does not match opening tag",
        ]
    );

    let diagnostics = tree.diagnostics();

    assert_eq!(
        diagnostics[0].span,
        Span {
            start: source.len(),
            end: source.len()
        }
    );

    let closing = source.windows(2).position(|bytes| bytes == b"</").unwrap();

    assert_eq!(
        diagnostics[1].span,
        Span {
            start: closing,
            end: source.len()
        }
    );

    assert!(diagnostics[0].span.start > diagnostics[1].span.start);
    assert_eq!(diagnostics[0].code(), DiagnosticCode::ExpectedSyntax);
    assert_eq!(diagnostics[1].code(), DiagnosticCode::InvalidSyntax);

    assert!(
        diagnostics
            .iter()
            .all(|diagnostic| diagnostic.severity() == Severity::Error)
    );

    replace(&tree, b"node", b"longer_node", parse_luaux);
}

#[test]
fn empty_child_slots_gain_diagnostics_at_their_original_chronological_position() {
    let original = parse(b"return 1\nlocal later = 0x\n");

    assert_eq!(
        messages(&original),
        [
            "statement follows a block-ending statement",
            "malformed number",
        ]
    );

    let updated = replace(&original, b"return 1", b"return 0x", parse);

    assert_eq!(
        messages(&updated),
        [
            "malformed number",
            "statement follows a block-ending statement",
            "malformed number",
        ]
    );

    let repaired = replace(&updated, b"return 0x", b"return 2", parse);
    assert_eq!(messages(&repaired), messages(&original));
}

#[test]
fn shifted_suffix_diagnostics_keep_their_owner_identity_and_old_snapshot() {
    let original = parse(b"local first = 1\nlocal broken = 0x\nlocal escaped = '\\256'\n");

    assert_eq!(
        messages(&original),
        ["malformed number", "malformed string escape"]
    );

    let suffix = original
        .root()
        .children()
        .next()
        .unwrap()
        .children()
        .nth(1)
        .unwrap()
        .identity();

    let old = original.diagnostics().to_vec();
    let updated = replace(&original, b"first = 1", b"first = 123456", parse);

    let shifted = updated
        .root()
        .children()
        .next()
        .unwrap()
        .children()
        .nth(1)
        .unwrap()
        .identity();

    assert_eq!(suffix, shifted);
    assert_eq!(updated.diagnostics().len(), old.len());

    for (before, after) in old.iter().zip(updated.diagnostics()) {
        assert_eq!(before.message, after.message);
        assert_eq!(after.span.start, before.span.start + 5);
        assert_eq!(after.span.end, before.span.end + 5);
    }

    assert_eq!(original.diagnostics(), old);
}

#[test]
fn child_count_changes_reparse_owners_without_reordering_direct_records() {
    let original = parse(b"return 1\nlocal later = 0x\n");

    let updated = replace(
        &original,
        b"return 1",
        b"return 1\nlocal middle = 0x",
        parse,
    );

    assert_eq!(
        messages(&updated),
        [
            "statement follows a block-ending statement",
            "malformed number",
            "malformed number",
        ]
    );

    replace(&updated, b"\nlocal middle = 0x", b"", parse);
}

#[test]
fn batches_preserve_chronology_and_repair_only_edited_diagnostics() {
    let original = parse_luaux(
        b"local first = 1\nlocal broken = 0x\nlocal escaped = '\\256'\nlocal node = <First></Other",
    );

    let old = original.diagnostics().to_vec();

    assert_eq!(
        messages(&original),
        [
            "malformed number",
            "malformed string escape",
            "expected end of closing tag",
            "closing tag does not match opening tag",
        ]
    );

    let first = original
        .source()
        .windows(1)
        .position(|bytes| bytes == b"1")
        .unwrap();

    let closing = original
        .source()
        .windows(5)
        .position(|bytes| bytes == b"Other")
        .unwrap();

    let edits = [
        Edit {
            range: Span {
                start: closing,
                end: closing + 5,
            },
            replacement: b"First>".to_vec(),
        },
        Edit {
            range: Span {
                start: first,
                end: first + 1,
            },
            replacement: b"123456".to_vec(),
        },
    ];

    let updated = original.update_many(&edits).unwrap();

    assert_eq!(
        updated.source(),
        b"local first = 123456\nlocal broken = 0x\nlocal escaped = '\\256'\nlocal node = <First></First>"
    );

    assert_eq!(
        updated.diagnostics(),
        [
            Diagnostic {
                span: Span { start: 36, end: 38 },
                message: "malformed number",
            },
            Diagnostic {
                span: Span { start: 55, end: 61 },
                message: "malformed string escape",
            },
        ]
    );

    assert_eq!(
        updated.diagnostics(),
        parse_luaux(updated.source()).diagnostics()
    );

    assert_eq!(original.diagnostics(), old);

    assert_eq!(
        original.source(),
        b"local first = 1\nlocal broken = 0x\nlocal escaped = '\\256'\nlocal node = <First></Other"
    );
}

#[test]
fn zero_width_records_shift_and_disappear_when_their_owner_is_repaired() {
    let original = parse(b"local value =\nreturn (item");
    assert_ne!(original.diagnostics(), []);

    assert!(
        original
            .diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.span.is_empty())
    );

    let shifted = replace(&original, b"value", b"longer_value", parse);
    let completed = replace(&shifted, b"=\n", b"= 1\n", parse);
    let repaired = replace(&completed, b"(item", b"(item)", parse);
    assert_eq!(repaired.diagnostics(), []);
}

#[test]
fn identical_messages_at_distinct_anchors_remain_independent() {
    let original = parse_luaux(b"local node = <A><B></C></D>");

    assert_eq!(
        messages(&original),
        [
            "closing tag does not match opening tag",
            "closing tag does not match opening tag",
        ]
    );

    let records = original.diagnostics();
    assert_eq!(records[0].span.len(), records[1].span.len());
    assert_ne!(records[0].span.start, records[1].span.start);
    let repaired = replace(&original, b"</C>", b"</B>", parse_luaux);

    assert_eq!(
        messages(&repaired),
        ["closing tag does not match opening tag"]
    );

    assert_eq!(repaired.diagnostics()[0].span, records[1].span);
    let completed = replace(&repaired, b"</D>", b"</A>", parse_luaux);
    assert_eq!(completed.diagnostics(), []);
}
