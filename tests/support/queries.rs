use vermis::tree::{NodeIndex, NodeKind, Tree};

use super::validation::{check, relations};

/// Parses source that must be valid.
///
/// # Panics
/// Panics on diagnostics or invalid storage.
pub(crate) fn accepted(source: &[u8]) -> Tree<'_> {
    let tree = check(source);

    assert!(
        tree.diagnostics.is_empty(),
        "{:?}: {:?}",
        tree.source,
        tree.diagnostics
    );

    tree
}

/// Finds the first matching node in preorder.
///
/// # Panics
/// Panics if no node matches.
pub(crate) fn first(tree: &Tree<'_>, predicate: impl Fn(&NodeKind) -> bool) -> NodeIndex {
    let mut pending = vec![tree.root];

    while let Some(index) = pending.pop() {
        if predicate(&tree.node(index).kind) {
            return index;
        }

        let (children, _) = relations(tree, index);
        pending.extend(children.into_iter().rev());
    }

    panic!("expected syntax node");
}
