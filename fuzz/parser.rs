#![no_main]

use bstr::BStr;
use libfuzzer_sys::fuzz_target;
use vermis::parse;

fuzz_target!(|source: &[u8]| {
    let tree = parse(BStr::new(source));
    let mut end = 0;

    for token in &tree.tokens {
        assert_eq!(token.span.start, end);
        assert!(token.span.start <= token.span.end && token.span.end <= source.len());
        end = token.span.end;
    }

    assert_eq!(end, source.len());
    assert_eq!(tree.text(tree.root), source);

    let mut parents = vec![0; tree.nodes.len()];

    for (index, node) in tree.nodes.iter().enumerate() {
        assert!(node.span.start <= node.span.end && node.span.end <= source.len());
        let mut end = node.span.start;

        for child in &node.children {
            assert!(*child < index);
            let span = tree.nodes[*child].span;
            assert!(span.start >= end && span.end <= node.span.end);
            end = span.end;
            parents[*child] += 1;
        }
    }

    assert!(parents.iter().enumerate().all(|(index, count)| *count == usize::from(index != tree.root)));

    for diagnostic in &tree.diagnostics {
        assert!(diagnostic.span.start <= diagnostic.span.end && diagnostic.span.end <= source.len());
    }
});
