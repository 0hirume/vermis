use std::{fs, path::Path};
use vermis::parse;

#[test]
fn upstream_programs() {
    let directory = Path::new("vendor/luau/tests/conformance");

    let mut files: Vec<_> = fs::read_dir(directory)
        .expect("pinned Luau corpus is required")
        .map(|entry| entry.expect("corpus entry").path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "luau" || extension == "lua")
        })
        .collect();

    files.sort();
    assert!(!files.is_empty(), "upstream corpus is empty");

    let mut failures = Vec::new();

    for path in files {
        let source = fs::read(&path).expect("read corpus program");
        let tree = parse(&source);

        if !tree.diagnostics().is_empty() {
            failures.push(format!("{}: {:?}", path.display(), tree.diagnostics()));
        }

        let restored: Vec<_> = tree
            .tokens()
            .flat_map(|token| token.text().iter().copied())
            .collect();

        assert_eq!(restored, source, "{}", path.display());
        assert_eq!(tree.source(), source.as_slice());
        assert_eq!(tree.root().text(), source.as_slice());
        assert_eq!(tree.root().parent(), None);

        for node in tree.root().descendants() {
            let span = node.span();
            assert!(span.start <= span.end && span.end <= source.len());

            if node != tree.root() {
                let parent = node.parent().unwrap();
                assert_eq!(parent.children().filter(|child| *child == node).count(), 1);
            }

            let mut end = span.start;

            for child in node.children() {
                let child_span = child.span();

                assert!(
                    end <= child_span.start && child_span.end <= span.end,
                    "{}: {node:?}, child: {child:?}",
                    path.display()
                );

                assert_eq!(child.parent(), Some(node));
                end = child_span.end;
            }
        }
    }

    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
