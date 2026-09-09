#[allow(dead_code)]
#[path = "support/oracle.rs"]
mod oracle;
#[allow(dead_code)]
#[path = "support/parser.rs"]
mod parser;

use std::fs;
use std::path::{Path, PathBuf};

fn oracle() -> oracle::Oracle {
    let path = std::env::var_os("VERMIS_ORACLE")
        .map(PathBuf::from)
        .expect("VERMIS_ORACLE must point to the Luau oracle executable");

    oracle::Oracle::spawn(&path).expect("failed to start Luau oracle")
}

fn walk(path: &Path, files: &mut Vec<PathBuf>) {
    let entries = fs::read_dir(path).unwrap_or_else(|error| panic!("{}: {error}", path.display()));

    for entry in entries {
        let entry = entry.unwrap_or_else(|error| panic!("{}: {error}", path.display()));
        let path = entry.path();

        if path.is_dir() {
            walk(&path, files);
        } else if path
            .extension()
            .is_some_and(|extension| extension == "lua" || extension == "luau")
        {
            files.push(path);
        }
    }
}

#[test]
#[ignore = "requires the built Luau oracle"]
fn conformance_ast_parity() {
    let mut files = Vec::new();
    walk(Path::new("vendor/luau/tests/conformance"), &mut files);
    files.sort();

    let mut oracle = oracle();
    let mut mismatches = Vec::new();

    for path in files {
        let source = fs::read(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
        if let Err(error) = parser::compare_chunk(&mut oracle, &source) {
            mismatches.push(format!("{}: {error}", path.display()));
        }
    }

    assert!(
        mismatches.is_empty(),
        "{} mismatches:\n{}",
        mismatches.len(),
        mismatches.join("\n")
    );
}
