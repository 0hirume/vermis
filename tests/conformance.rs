use bstr::BStr;
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
        let tree = parse(BStr::new(&source));

        if !tree.diagnostics.is_empty() {
            failures.push(format!("{}: {:?}", path.display(), tree.diagnostics));
        }

        let restored: Vec<_> = tree
            .tokens
            .iter()
            .flat_map(|token| token.bytes(tree.source).iter().copied())
            .collect();

        assert_eq!(restored, source, "{}", path.display());
        assert_eq!(tree.text(tree.root), source.as_slice());

        for node in &tree.nodes {
            for child in &tree.children[node.children.clone()] {
                let span = tree.nodes[*child].span;

                assert!(
                    node.span.start <= span.start && span.end <= node.span.end,
                    "{}: {node:?}, child: {:?}",
                    path.display(),
                    tree.nodes[*child]
                );
            }
        }
    }

    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
