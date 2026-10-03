use std::{
    error::Error,
    fmt,
    hash::{Hash, Hasher},
    sync::{Arc, RwLock, atomic::Ordering},
};

use crate::{Control, Edit, EditError, ParseError, Span, Tree};

#[derive(Clone)]
pub struct Revision(Arc<()>);

impl fmt::Debug for Revision {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_tuple("Revision")
            .field(&Arc::as_ptr(&self.0))
            .finish()
    }
}

impl PartialEq for Revision {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for Revision {}

impl Hash for Revision {
    fn hash<HasherType: Hasher>(&self, state: &mut HasherType) {
        Arc::as_ptr(&self.0).hash(state);
    }
}

#[derive(Clone, Debug)]
pub struct Snapshot {
    tree: Arc<Tree>,
    revision: Revision,
}

impl Snapshot {
    #[must_use]
    pub fn tree(&self) -> &Arc<Tree> {
        &self.tree
    }

    #[must_use]
    pub fn revision(&self) -> Revision {
        self.revision.clone()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DocumentError {
    Conflict,
    Edit(EditError),
}

impl fmt::Display for DocumentError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Conflict => formatter.write_str("document snapshot is no longer current"),
            Self::Edit(error) => fmt::Display::fmt(error, formatter),
        }
    }
}

impl Error for DocumentError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Conflict => None,
            Self::Edit(error) => Some(error),
        }
    }
}

pub struct Document {
    current: RwLock<Snapshot>,
}

impl Document {
    #[must_use]
    pub fn new(tree: Tree) -> Self {
        Self {
            current: RwLock::new(Self::snapshot_of(tree)),
        }
    }

    #[must_use]
    pub fn snapshot(&self) -> Snapshot {
        self.current
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    #[must_use]
    pub fn is_current(&self, revision: &Revision) -> bool {
        self.current
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .revision
            == *revision
    }

    /// # Errors
    ///
    /// Returns a conflict if the expected snapshot is stale, or an edit error for
    /// a range outside the expected source.
    pub fn update(
        &self,
        expected: &Snapshot,
        range: Span,
        replacement: &[u8],
    ) -> Result<Snapshot, DocumentError> {
        self.update_with(expected, range, replacement, &Control::default())
    }

    /// # Errors
    ///
    /// Returns a conflict if the expected snapshot is stale, or an edit error for
    /// an invalid range, cancellation or exceeded resource limits.
    pub fn update_with(
        &self,
        expected: &Snapshot,
        range: Span,
        replacement: &[u8],
        control: &Control,
    ) -> Result<Snapshot, DocumentError> {
        self.check(expected)?;

        let tree = expected
            .tree
            .update_with(range, replacement, control)
            .map_err(DocumentError::Edit)?;

        self.publish(expected, tree, control)
    }

    /// # Errors
    ///
    /// Returns a conflict if the expected snapshot is stale, or an edit error for
    /// invalid or overlapping original-coordinate edits. No error publishes a tree.
    pub fn update_many(
        &self,
        expected: &Snapshot,
        edits: &[Edit],
    ) -> Result<Snapshot, DocumentError> {
        self.update_many_with(expected, edits, &Control::default())
    }

    /// # Errors
    ///
    /// Returns a conflict if the expected snapshot is stale, or an edit error for
    /// invalid or overlapping edits, cancellation or exceeded resource limits.
    pub fn update_many_with(
        &self,
        expected: &Snapshot,
        edits: &[Edit],
        control: &Control,
    ) -> Result<Snapshot, DocumentError> {
        self.check(expected)?;

        let tree = expected
            .tree
            .update_many_with(edits, control)
            .map_err(DocumentError::Edit)?;

        self.publish(expected, tree, control)
    }

    fn snapshot_of(tree: Tree) -> Snapshot {
        Snapshot {
            tree: Arc::new(tree),
            revision: Revision(Arc::new(())),
        }
    }

    fn check(&self, expected: &Snapshot) -> Result<(), DocumentError> {
        if self.is_current(&expected.revision) {
            Ok(())
        } else {
            Err(DocumentError::Conflict)
        }
    }

    fn publish(
        &self,
        expected: &Snapshot,
        tree: Tree,
        control: &Control,
    ) -> Result<Snapshot, DocumentError> {
        let next = Self::snapshot_of(tree);

        let mut current = self
            .current
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);

        if current.revision != expected.revision {
            return Err(DocumentError::Conflict);
        }

        if control
            .cancellation
            .as_ref()
            .is_some_and(|cancellation| cancellation.load(Ordering::Relaxed))
        {
            return Err(DocumentError::Edit(EditError::Parse(ParseError::Cancelled)));
        }

        *current = next.clone();

        Ok(next)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicBool;

    use super::*;
    use crate::parse;

    #[test]
    fn cancellation_after_staging_prevents_publication() {
        let document = Document::new(parse(b"return 1\n"));
        let original = document.snapshot();
        let cancellation = Arc::new(AtomicBool::new(false));

        let control = Control {
            cancellation: Some(Arc::clone(&cancellation)),
            ..Control::default()
        };

        let staged = original
            .tree()
            .update_with(Span { start: 7, end: 8 }, b"200", &control)
            .unwrap();

        cancellation.store(true, Ordering::Relaxed);

        assert!(matches!(
            document.publish(&original, staged, &control),
            Err(DocumentError::Edit(EditError::Parse(ParseError::Cancelled)))
        ));

        let current = document.snapshot();
        assert_eq!(current.revision(), original.revision());
        assert!(document.is_current(&original.revision()));
        assert!(Arc::ptr_eq(current.tree(), original.tree()));
        assert_eq!(current.tree().source(), b"return 1\n");
    }
}
