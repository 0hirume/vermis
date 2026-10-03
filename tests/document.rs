use std::{
    sync::{
        Arc, Barrier,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
};

use vermis::{
    Control, Document, DocumentError, Edit, EditError, Limits, ParseError, Resource, Snapshot,
    Span, parse,
};

fn consistent(snapshot: &Snapshot) {
    let tree = snapshot.tree();
    assert_eq!(tree.root().text(), tree.source());

    assert_eq!(
        tree.tokens()
            .flat_map(|token| token.text().iter().copied())
            .collect::<Vec<_>>(),
        tree.source()
    );

    assert_eq!(
        tree.source_chunks().flatten().copied().collect::<Vec<_>>(),
        tree.source()
    );
}

fn unchanged(document: &Document, original: &Snapshot) {
    let current = document.snapshot();
    assert_eq!(current.revision(), original.revision());
    assert!(document.is_current(&original.revision()));
    assert!(Arc::ptr_eq(current.tree(), original.tree()));
    consistent(original);
    consistent(&current);
}

#[test]
fn publication_preserves_old_snapshots_and_invalidates_old_revisions() {
    let document = Document::new(parse(b"return 1\n"));
    let original = document.snapshot();
    let again = document.snapshot();
    let revision = original.revision();
    assert_eq!(revision, again.revision());
    assert!(Arc::ptr_eq(original.tree(), again.tree()));
    assert!(document.is_current(&revision));

    let updated = document
        .update(&original, Span { start: 7, end: 8 }, b"200")
        .unwrap();

    assert!(!document.is_current(&revision));
    assert!(document.is_current(&updated.revision()));
    assert_ne!(revision, updated.revision());
    assert_eq!(document.snapshot().revision(), updated.revision());
    assert!(Arc::ptr_eq(document.snapshot().tree(), updated.tree()));
    assert_eq!(original.tree().source(), b"return 1\n");
    assert_eq!(updated.tree().source(), b"return 200\n");
    consistent(&original);
    consistent(&updated);
}

#[test]
fn stale_and_foreign_snapshots_cannot_overwrite_publication() {
    let document = Document::new(parse(b"return 1\n"));
    let original = document.snapshot();

    let updated = document
        .update(&original, Span { start: 7, end: 8 }, b"2")
        .unwrap();

    let stale = document.update(&original, Span { start: 7, end: 8 }, b"3");
    assert!(matches!(stale, Err(DocumentError::Conflict)));

    let stale_batch = document.update_many(
        &original,
        &[Edit {
            range: Span { start: 7, end: 8 },
            replacement: b"4".to_vec(),
        }],
    );

    assert!(matches!(stale_batch, Err(DocumentError::Conflict)));
    let foreign = Document::new(parse(b"return 2\n")).snapshot();

    assert!(matches!(
        document.update(&foreign, Span { start: 7, end: 8 }, b"5"),
        Err(DocumentError::Conflict)
    ));

    assert_eq!(document.snapshot().revision(), updated.revision());
    assert!(Arc::ptr_eq(document.snapshot().tree(), updated.tree()));
    assert_eq!(document.snapshot().tree().source(), b"return 2\n");
}

#[test]
fn batches_use_the_same_original_coordinates() {
    let document = Document::new(parse(
        b"local first = 1\nlocal second = 2\nreturn first + second\n",
    ));

    let original = document.snapshot();

    let updated = document
        .update_many(
            &original,
            &[
                Edit {
                    range: Span { start: 31, end: 32 },
                    replacement: b"2000".to_vec(),
                },
                Edit {
                    range: Span { start: 14, end: 15 },
                    replacement: b"100".to_vec(),
                },
            ],
        )
        .unwrap();

    assert_eq!(
        original.tree().source(),
        b"local first = 1\nlocal second = 2\nreturn first + second\n"
    );

    assert_eq!(
        updated.tree().source(),
        b"local first = 100\nlocal second = 2000\nreturn first + second\n"
    );

    assert_eq!(document.snapshot().revision(), updated.revision());
    consistent(&original);
    consistent(&updated);
}

#[test]
fn invalid_batches_leave_tree_and_revision_unchanged() {
    let document = Document::new(parse(b"return 1\n"));
    let original = document.snapshot();

    for invalid in [
        Span { start: 8, end: 7 },
        Span { start: 0, end: 100 },
        Span {
            start: 100,
            end: 100,
        },
    ] {
        let edits = [
            Edit {
                range: Span { start: 7, end: 8 },
                replacement: b"200".to_vec(),
            },
            Edit {
                range: invalid,
                replacement: Vec::new(),
            },
        ];

        assert!(matches!(
            document.update_many(&original, &edits),
            Err(DocumentError::Edit(EditError::InvalidRange))
        ));

        let current = document.snapshot();
        assert_eq!(current.revision(), original.revision());
        assert!(Arc::ptr_eq(current.tree(), original.tree()));
    }

    let overlap = [
        Edit {
            range: Span { start: 0, end: 8 },
            replacement: b"return 2".to_vec(),
        },
        Edit {
            range: Span { start: 7, end: 8 },
            replacement: b"3".to_vec(),
        },
    ];

    assert!(matches!(
        document.update_many(&original, &overlap),
        Err(DocumentError::Edit(EditError::OverlappingEdits))
    ));

    assert!(matches!(
        document.update(&original, Span { start: 1, end: 0 }, b""),
        Err(DocumentError::Edit(EditError::InvalidRange))
    ));

    assert_eq!(document.snapshot().revision(), original.revision());
    assert!(Arc::ptr_eq(document.snapshot().tree(), original.tree()));
    consistent(&document.snapshot());
}

#[test]
fn racing_edits_publish_exactly_one_snapshot() {
    let document = Arc::new(Document::new(parse(b"return 0\n")));
    let original = document.snapshot();
    let barrier = Arc::new(Barrier::new(8));

    let jobs: Vec<_> = (1_u8..=8)
        .map(|number| {
            let document = Arc::clone(&document);
            let expected = original.clone();
            let barrier = Arc::clone(&barrier);

            thread::spawn(move || {
                barrier.wait();

                document.update(&expected, Span { start: 7, end: 8 }, &[b'0' + number])
            })
        })
        .collect();

    let mut winners = Vec::new();

    for job in jobs {
        match job.join().unwrap() {
            Ok(snapshot) => winners.push(snapshot),
            Err(DocumentError::Conflict) => {}
            Err(error) => panic!("unexpected edit error: {error}"),
        }
    }

    assert_eq!(winners.len(), 1);
    let winner = &winners[0];
    let current = document.snapshot();
    assert_eq!(current.revision(), winner.revision());
    assert!(Arc::ptr_eq(current.tree(), winner.tree()));
    assert_ne!(current.revision(), original.revision());
    assert_eq!(original.tree().source(), b"return 0\n");
    consistent(&original);
    consistent(&current);
}

#[test]
fn coordinated_readers_observe_successive_complete_batches() {
    let document = Arc::new(Document::new(parse(b"return 0, 0\n")));
    let original = document.snapshot();
    let (published, publications) = mpsc::sync_channel(0);
    let (observed, observations) = mpsc::sync_channel(0);

    let writer = {
        let document = Arc::clone(&document);

        thread::spawn(move || {
            for number in 1_u8..=32 {
                let expected = document.snapshot();
                let replacement = vec![b'0' + number % 2];

                let updated = document
                    .update_many(
                        &expected,
                        &[
                            Edit {
                                range: Span { start: 7, end: 8 },
                                replacement: replacement.clone(),
                            },
                            Edit {
                                range: Span { start: 10, end: 11 },
                                replacement,
                            },
                        ],
                    )
                    .unwrap();

                published.send(updated.revision()).unwrap();
                observations.recv().unwrap();
            }
        })
    };

    let mut previous = original.clone();

    for number in 1_u8..=32 {
        let revision = publications.recv().unwrap();
        let snapshot = document.snapshot();
        let tree = snapshot.tree();
        let source = format!("return {0}, {0}\n", number % 2);
        assert_eq!(snapshot.revision(), revision);
        assert_ne!(snapshot.revision(), previous.revision());
        assert!(!document.is_current(&previous.revision()));
        assert_eq!(tree.source(), source.as_bytes());

        assert_eq!(
            previous.tree().source(),
            format!("return {0}, {0}\n", (number - 1) % 2).as_bytes()
        );

        let fresh = parse(source.as_bytes());
        assert_eq!(tree.diagnostics(), fresh.diagnostics());

        assert_eq!(
            tree.root()
                .descendants()
                .map(|node| (node.kind(), node.span(), node.children().count()))
                .collect::<Vec<_>>(),
            fresh
                .root()
                .descendants()
                .map(|node| (node.kind(), node.span(), node.children().count()))
                .collect::<Vec<_>>()
        );

        assert_eq!(
            tree.tokens().map(|token| token.data()).collect::<Vec<_>>(),
            fresh.tokens().map(|token| token.data()).collect::<Vec<_>>()
        );

        consistent(&snapshot);
        previous = snapshot;
        observed.send(()).unwrap();
    }

    writer.join().unwrap();
    assert_eq!(original.tree().source(), b"return 0, 0\n");
    assert_eq!(document.snapshot().tree().source(), b"return 0, 0\n");
    assert!(!document.is_current(&original.revision()));
}

#[test]
fn cancelled_updates_and_empty_batches_leave_publication_unchanged() {
    let document = Document::new(parse(b"return 1\n"));
    let original = document.snapshot();
    let cancellation = Arc::new(AtomicBool::new(true));

    let control = Control {
        cancellation: Some(Arc::clone(&cancellation)),
        ..Control::default()
    };

    for (range, replacement) in [
        (Span { start: 7, end: 8 }, b"200".as_slice()),
        (Span { start: 7, end: 8 }, b"1".as_slice()),
        (Span { start: 0, end: 0 }, b"".as_slice()),
    ] {
        assert!(matches!(
            document.update_with(&original, range, replacement, &control),
            Err(DocumentError::Edit(EditError::Parse(ParseError::Cancelled)))
        ));

        unchanged(&document, &original);

        assert!(matches!(
            document.update_many_with(
                &original,
                &[Edit {
                    range,
                    replacement: replacement.to_vec()
                }],
                &control
            ),
            Err(DocumentError::Edit(EditError::Parse(ParseError::Cancelled)))
        ));

        unchanged(&document, &original);
    }

    assert!(matches!(
        document.update_many_with(&original, &[], &control),
        Err(DocumentError::Edit(EditError::Parse(ParseError::Cancelled)))
    ));

    unchanged(&document, &original);
    cancellation.store(false, Ordering::Relaxed);

    let updated = document
        .update_with(&original, Span { start: 7, end: 8 }, b"2", &control)
        .unwrap();

    assert_eq!(updated.tree().source(), b"return 2\n");
    assert_eq!(original.tree().source(), b"return 1\n");
    unchanged(&document, &updated);
    consistent(&original);
}

#[test]
fn resource_failures_discard_staged_edits_and_empty_batches() {
    let document = Document::new(parse(b"return 1, 2\n"));
    let original = document.snapshot();

    let control = Control {
        limits: Limits {
            source_bytes: Some(original.tree().source().len() + 1),
            ..Limits::default()
        },
        ..Control::default()
    };

    let edits = [
        Edit {
            range: Span { start: 7, end: 8 },
            replacement: b"200".to_vec(),
        },
        Edit {
            range: Span { start: 10, end: 11 },
            replacement: b"3".to_vec(),
        },
    ];

    assert!(matches!(
        document.update_with(&original, edits[0].range, &edits[0].replacement, &control),
        Err(DocumentError::Edit(EditError::Parse(ParseError::Limit(
            Resource::SourceBytes
        ))))
    ));

    unchanged(&document, &original);

    assert!(matches!(
        document.update_many_with(&original, &edits, &control),
        Err(DocumentError::Edit(EditError::Parse(ParseError::Limit(
            Resource::SourceBytes
        ))))
    ));

    unchanged(&document, &original);

    let control = Control {
        limits: Limits {
            source_bytes: Some(0),
            ..Limits::default()
        },
        ..Control::default()
    };

    assert!(matches!(
        document.update_with(&original, Span { start: 7, end: 8 }, b"1", &control),
        Err(DocumentError::Edit(EditError::Parse(ParseError::Limit(
            Resource::SourceBytes
        ))))
    ));

    unchanged(&document, &original);

    assert!(matches!(
        document.update_many_with(&original, &[], &control),
        Err(DocumentError::Edit(EditError::Parse(ParseError::Limit(
            Resource::SourceBytes
        ))))
    ));

    unchanged(&document, &original);
}
