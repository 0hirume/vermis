//! Upstream Luau conformance tests.

use std::fs;
use std::path::Path;

use vermis::emitter::{Emitter, LineEnding};

use crate::support::check;

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
        let tree = check(&source);

        if !tree.diagnostics.is_empty() {
            failures.push(format!("{}: {:?}", path.display(), tree.diagnostics));
        }

        let mut emitter = Emitter::from_tree(&tree, LineEnding::LineFeed);
        emitter.node(&tree, tree.root);
        let output = emitter.finish();
        assert_eq!(output.as_ref(), source, "{}", path.display());

        let reparsed = check(&output);
        assert_eq!(reparsed.tokens, tree.tokens, "{}", path.display());
        assert_eq!(reparsed.nodes, tree.nodes, "{}", path.display());
        assert_eq!(reparsed.lists, tree.lists, "{}", path.display());
        assert_eq!(reparsed.diagnostics, tree.diagnostics, "{}", path.display());
    }

    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
