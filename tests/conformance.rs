//! Upstream Luau conformance tests.

use std::{fs, path::Path};
pub mod support;

use support::check;

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
    }

    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
