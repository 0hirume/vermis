use std::{
    ops::Range,
    sync::{Arc, OnceLock},
};

use crate::Span;

const BUFFER_CAPACITY: usize = 4096;
const FANOUT: usize = 16;

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub struct Position {
    pub line: usize,
    pub column: usize,
}

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub enum CoordinateError {
    OutOfBounds,
    InvalidBoundary,
}

#[derive(Clone, Debug)]
struct Piece {
    bytes: Arc<[u8]>,
    range: Range<usize>,
}

#[derive(Clone, Copy, Debug, Default)]
struct Measure {
    length: usize,
    lines: usize,
    units: usize,
    prefix: [u8; 3],
    suffix: [u8; 3],
}

impl Measure {
    fn from_bytes(bytes: &[u8]) -> Self {
        let mut measure = Self {
            length: bytes.len(),
            ..Self::default()
        };

        let mut position = 0;

        while position < bytes.len() {
            let width = scalar_width(&bytes[position..]);

            measure.units += if width == 4 { 2 } else { 1 };

            if bytes[position] == b'\r'
                || (bytes[position] == b'\n' && (position == 0 || bytes[position - 1] != b'\r'))
            {
                measure.lines += 1;
            }

            position += width;
        }

        let edges = bytes.len().min(3);
        measure.prefix[..edges].copy_from_slice(&bytes[..edges]);
        measure.suffix[..edges].copy_from_slice(&bytes[bytes.len() - edges..]);

        measure
    }

    fn append(self, other: Self) -> Self {
        if self.length == 0 {
            return other;
        }

        if other.length == 0 {
            return self;
        }

        let left = self.length.min(3);
        let right = other.length.min(3);
        let mut boundary = [0; 6];
        boundary[..left].copy_from_slice(&self.suffix[..left]);
        boundary[left..left + right].copy_from_slice(&other.prefix[..right]);
        let mut units = self.units + other.units;

        for start in 0..left {
            let width = scalar_width(&boundary[start..left + right]);

            if width > 1 && start + width > left {
                units -= width - if width == 4 { 2 } else { 1 };

                break;
            }
        }

        let length = self.length + other.length;
        let edges = length.min(3);
        let mut prefix = [0; 3];
        let mut suffix = [0; 3];

        if self.length >= 3 {
            prefix = self.prefix;
        } else {
            prefix[..left].copy_from_slice(&self.prefix[..left]);
            prefix[left..edges].copy_from_slice(&other.prefix[..edges - left]);
        }

        if other.length >= 3 {
            suffix = other.suffix;
        } else {
            let retained = edges - right;
            suffix[..retained].copy_from_slice(&self.suffix[left - retained..left]);
            suffix[retained..edges].copy_from_slice(&other.suffix[..right]);
        }

        Self {
            length,
            lines: self.lines + other.lines
                - usize::from(self.suffix[left - 1] == b'\r' && other.prefix[0] == b'\n'),
            units,
            prefix,
            suffix,
        }
    }
}

fn scalar_width(bytes: &[u8]) -> usize {
    let Some(&first) = bytes.first() else {
        return 0;
    };

    let width = match first {
        0xc2..=0xdf => 2,
        0xe0..=0xef => 3,
        0xf0..=0xf4 => 4,
        _ => return 1,
    };

    if bytes.len() < width
        || bytes[1..width]
            .iter()
            .any(|byte| !(0x80..=0xbf).contains(byte))
        || (first == 0xe0 && bytes[1] < 0xa0)
        || (first == 0xed && bytes[1] >= 0xa0)
        || (first == 0xf0 && bytes[1] < 0x90)
        || (first == 0xf4 && bytes[1] >= 0x90)
    {
        1
    } else {
        width
    }
}

#[derive(Debug)]
enum Contents {
    Piece(Piece),
    Children(Box<[Arc<Node>]>),
}

#[derive(Debug)]
struct Node {
    measure: Measure,
    height: usize,
    contents: Contents,
}

impl Node {
    fn piece(piece: Piece) -> Arc<Self> {
        Arc::new(Self {
            measure: Measure::from_bytes(&piece.bytes[piece.range.clone()]),
            height: 0,
            contents: Contents::Piece(piece),
        })
    }

    fn branch(children: Vec<Arc<Self>>) -> Arc<Self> {
        assert!(!children.is_empty() && children.len() <= FANOUT);

        Arc::new(Self {
            measure: children.iter().fold(Measure::default(), |measure, child| {
                measure.append(child.measure)
            }),
            height: children[0].height + 1,
            contents: Contents::Children(children.into_boxed_slice()),
        })
    }

    fn byte(&self, mut offset: usize) -> Option<u8> {
        if offset >= self.measure.length {
            return None;
        }

        match &self.contents {
            Contents::Piece(piece) => Some(piece.bytes[piece.range.start + offset]),

            Contents::Children(children) => {
                for child in children {
                    if offset < child.measure.length {
                        return child.byte(offset);
                    }

                    offset -= child.measure.length;
                }

                unreachable!()
            }
        }
    }

    fn prefix(&self, mut length: usize) -> Measure {
        if length == self.measure.length {
            return self.measure;
        }

        match &self.contents {
            Contents::Piece(piece) => {
                Measure::from_bytes(&piece.bytes[piece.range.start..piece.range.start + length])
            }

            Contents::Children(children) => {
                let mut measure = Measure::default();

                for child in children {
                    if length < child.measure.length {
                        return measure.append(child.prefix(length));
                    }

                    measure = measure.append(child.measure);
                    length -= child.measure.length;
                }

                measure
            }
        }
    }
}

fn pack(mut children: Vec<Arc<Node>>) -> Vec<Arc<Node>> {
    if children.len() <= FANOUT {
        return vec![Node::branch(children)];
    }

    let right = children.split_off(children.len() / 2);

    vec![Node::branch(children), Node::branch(right)]
}

fn join(left: Arc<Node>, right: Arc<Node>) -> Vec<Arc<Node>> {
    match (&left.contents, &right.contents) {
        (Contents::Piece(first), Contents::Piece(second)) => {
            if Arc::ptr_eq(&first.bytes, &second.bytes) && first.range.end == second.range.start {
                return vec![Node::piece(Piece {
                    bytes: Arc::clone(&first.bytes),
                    range: first.range.start..second.range.end,
                })];
            }

            vec![left, right]
        }

        (Contents::Children(first), Contents::Children(second)) if left.height == right.height => {
            pack(first.iter().chain(second.iter()).cloned().collect())
        }

        (Contents::Children(children), _) if left.height > right.height => {
            let mut result = children.to_vec();
            let edge = result.pop().unwrap();
            result.extend(join(edge, right));

            pack(result)
        }

        (_, Contents::Children(children)) if right.height > left.height => {
            let mut result = join(left, Arc::clone(&children[0]));
            result.extend(children[1..].iter().cloned());

            pack(result)
        }

        _ => unreachable!(),
    }
}

fn concatenate(left: Option<Arc<Node>>, right: Option<Arc<Node>>) -> Option<Arc<Node>> {
    match (left, right) {
        (None, root) | (root, None) => root,

        (Some(left), Some(right)) => {
            let mut roots = join(left, right);

            Some(if roots.len() == 1 {
                roots.pop().unwrap()
            } else {
                Node::branch(roots)
            })
        }
    }
}

fn rebuild(mut children: Vec<Arc<Node>>) -> Option<Arc<Node>> {
    if children.len() < 2 {
        return children.pop();
    }

    let height = children.iter().map(|child| child.height).max().unwrap();

    let needs_neighbor = |child: &Arc<Node>| {
        child.height < height
            || matches!(&child.contents, Contents::Children(descendants) if descendants.len() < FANOUT / 2)
    };

    if needs_neighbor(&children[0]) {
        let first = children.remove(0);
        let second = children.remove(0);
        children.splice(0..0, join(first, second));
    }

    if children.len() > 1 && needs_neighbor(children.last().unwrap()) {
        let last = children.pop().unwrap();
        let previous = children.pop().unwrap();
        children.extend(join(previous, last));
    }

    Some(if children.len() == 1 {
        children.pop().unwrap()
    } else {
        Node::branch(children)
    })
}

fn split(root: Arc<Node>, offset: usize) -> (Option<Arc<Node>>, Option<Arc<Node>>) {
    if offset == 0 {
        return (None, Some(root));
    }

    if offset == root.measure.length {
        return (Some(root), None);
    }

    match &root.contents {
        Contents::Piece(piece) => {
            let middle = piece.range.start + offset;

            (
                Some(Node::piece(Piece {
                    bytes: Arc::clone(&piece.bytes),
                    range: piece.range.start..middle,
                })),
                Some(Node::piece(Piece {
                    bytes: Arc::clone(&piece.bytes),
                    range: middle..piece.range.end,
                })),
            )
        }

        Contents::Children(children) => {
            let mut remaining = offset;
            let mut left = Vec::new();
            let mut right = Vec::new();

            for child in children {
                if remaining == 0 {
                    right.push(Arc::clone(child));
                } else if remaining >= child.measure.length {
                    remaining -= child.measure.length;
                    left.push(Arc::clone(child));
                } else {
                    let (before, after) = split(Arc::clone(child), remaining);
                    left.extend(before);
                    right.extend(after);
                    remaining = 0;
                }
            }

            (rebuild(left), rebuild(right))
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct Source {
    root: Option<Arc<Node>>,
    contiguous: Option<Arc<OnceLock<Arc<[u8]>>>>,
}

impl From<&[u8]> for Source {
    fn from(bytes: &[u8]) -> Self {
        let mut nodes: Vec<_> = bytes
            .chunks(BUFFER_CAPACITY)
            .map(|chunk| {
                Node::piece(Piece {
                    bytes: Arc::from(chunk),
                    range: 0..chunk.len(),
                })
            })
            .collect();

        while nodes.len() > 1 {
            let groups = nodes.len().div_ceil(FANOUT);
            let width = nodes.len() / groups;
            let extra = nodes.len() % groups;
            let mut children = nodes.into_iter();

            nodes = (0..groups)
                .map(|group| {
                    Node::branch(
                        children
                            .by_ref()
                            .take(width + usize::from(group < extra))
                            .collect(),
                    )
                })
                .collect();
        }

        Self::from_root(nodes.pop())
    }
}

impl Source {
    fn from_root(root: Option<Arc<Node>>) -> Self {
        let contiguous = root
            .as_ref()
            .filter(|root| matches!(root.contents, Contents::Children(_)))
            .map(|_| Arc::new(OnceLock::new()));

        Self {
            root,
            contiguous,
        }
    }

    pub(crate) fn len(&self) -> usize {
        self.root.as_ref().map_or(0, |root| root.measure.length)
    }

    pub(crate) fn bytes(&self) -> &[u8] {
        match self.root.as_deref().map(|root| &root.contents) {
            None => &[],
            Some(Contents::Piece(piece)) => &piece.bytes[piece.range.clone()],

            Some(Contents::Children(_)) => self
                .contiguous
                .as_ref()
                .expect("composite source has a flattening cache")
                .get_or_init(|| {
                    let mut bytes = Vec::with_capacity(self.len());

                    for chunk in self.chunks() {
                        bytes.extend_from_slice(chunk);
                    }

                    Arc::from(bytes)
                }),
        }
    }

    #[cfg(test)]
    pub(crate) fn materialized(&self) -> bool {
        self.contiguous
            .as_ref()
            .is_some_and(|contiguous| contiguous.get().is_some())
    }

    pub(crate) fn chunks(&self) -> impl Iterator<Item = &[u8]> + Clone {
        Chunks {
            pending: self.root.as_deref().into_iter().collect(),
        }
    }

    pub(crate) fn slice(&self, span: Span) -> Self {
        fn extract(root: &Arc<Node>, span: Span) -> Arc<Node> {
            if span.start == 0 && span.end == root.measure.length {
                return Arc::clone(root);
            }

            match &root.contents {
                Contents::Piece(piece) => Node::piece(Piece {
                    bytes: Arc::clone(&piece.bytes),
                    range: piece.range.start + span.start..piece.range.start + span.end,
                }),

                Contents::Children(children) => {
                    let mut selected = Vec::new();
                    let mut offset = 0;

                    for child in children {
                        let end = offset + child.measure.length;

                        if span.start < end && offset < span.end {
                            selected.push(extract(
                                child,
                                Span {
                                    start: span.start.saturating_sub(offset),
                                    end: span.end.min(end) - offset,
                                },
                            ));
                        }

                        offset = end;

                        if offset >= span.end {
                            break;
                        }
                    }

                    rebuild(selected).expect("nonempty source slice")
                }
            }
        }

        assert!(span.start <= span.end && span.end <= self.len());

        if span.start == 0 && span.end == self.len() {
            return self.clone();
        }

        if span.is_empty() {
            return Self::from_root(None);
        }

        Self::from_root(self.root.as_ref().map(|root| extract(root, span)))
    }

    pub(crate) fn replace(&self, span: Span, replacement: &[u8]) -> Self {
        assert!(span.start <= span.end && span.end <= self.len());

        if span.is_empty() && replacement.is_empty() {
            return self.clone();
        }

        let (before, after) = self.root.as_ref().map_or((None, None), |root| {
            let (before, tail) = split(Arc::clone(root), span.start);
            let after = tail.and_then(|tail| split(tail, span.len()).1);

            (before, after)
        });

        let replacement = Self::from(replacement);

        Self::from_root(concatenate(concatenate(before, replacement.root), after))
    }

    pub(crate) fn byte(&self, offset: usize) -> Option<u8> {
        self.root.as_ref().and_then(|root| root.byte(offset))
    }

    fn prefix(&self, length: usize) -> Measure {
        self.root
            .as_ref()
            .map_or_else(Measure::default, |root| root.prefix(length))
    }

    fn scalar_start(&self, offset: usize) -> Option<usize> {
        for start in offset.saturating_sub(3)..offset {
            let mut bytes = [0; 4];
            let length = (self.len() - start).min(4);

            for (index, byte) in bytes[..length].iter_mut().enumerate() {
                *byte = self.byte(start + index).unwrap();
            }

            if start + scalar_width(&bytes[..length]) > offset {
                return Some(start);
            }
        }

        None
    }

    fn units_before(&self, offset: usize) -> usize {
        let units = self.prefix(offset).units;

        self.scalar_start(offset)
            .map_or(units, |start| units - (offset - start))
    }

    pub(crate) fn line_count(&self) -> usize {
        self.root.as_ref().map_or(1, |root| root.measure.lines + 1)
    }

    fn line_start(&self, line: usize) -> Result<usize, CoordinateError> {
        if line >= self.line_count() {
            return Err(CoordinateError::OutOfBounds);
        }

        if line == 0 {
            return Ok(0);
        }

        let mut low = 0;
        let mut high = self.len();

        while low < high {
            let middle = low + (high - low) / 2;

            if self.prefix(middle).lines < line {
                low = middle + 1;
            } else {
                high = middle;
            }
        }

        if self.byte(low - 1) == Some(b'\r') && self.byte(low) == Some(b'\n') {
            low += 1;
        }

        Ok(low)
    }

    pub(crate) fn position(&self, offset: usize) -> Result<Position, CoordinateError> {
        if offset > self.len() {
            return Err(CoordinateError::OutOfBounds);
        }

        if self.scalar_start(offset).is_some()
            || (offset > 0
                && self.byte(offset - 1) == Some(b'\r')
                && self.byte(offset) == Some(b'\n'))
        {
            return Err(CoordinateError::InvalidBoundary);
        }

        let measure = self.prefix(offset);
        let start = self.line_start(measure.lines)?;

        Ok(Position {
            line: measure.lines,
            column: measure.units - self.prefix(start).units,
        })
    }

    pub(crate) fn offset(&self, position: Position) -> Result<usize, CoordinateError> {
        let start = self.line_start(position.line)?;

        let mut end = if position.line + 1 == self.line_count() {
            self.len()
        } else {
            self.line_start(position.line + 1)?
        };

        if position.line + 1 < self.line_count() {
            end -= 1;

            if self.byte(end) == Some(b'\n') && end > start && self.byte(end - 1) == Some(b'\r') {
                end -= 1;
            }
        }

        let initial = self.prefix(start).units;
        let available = self.prefix(end).units - initial;

        if position.column > available {
            return Err(CoordinateError::OutOfBounds);
        }

        let target = initial + position.column;
        let mut low = start;
        let mut high = end;

        while low < high {
            let middle = low + (high - low) / 2;

            if self.units_before(middle) < target {
                low = middle + 1;
            } else {
                high = middle;
            }
        }

        if self.units_before(low) != target || self.scalar_start(low).is_some() {
            return Err(CoordinateError::InvalidBoundary);
        }

        Ok(low)
    }
}

#[derive(Clone)]
struct Chunks<'source> {
    pending: Vec<&'source Node>,
}

impl<'source> Iterator for Chunks<'source> {
    type Item = &'source [u8];

    fn next(&mut self) -> Option<Self::Item> {
        while let Some(node) = self.pending.pop() {
            match &node.contents {
                Contents::Piece(piece) => return Some(&piece.bytes[piece.range.clone()]),

                Contents::Children(children) => self
                    .pending
                    .extend(children.iter().rev().map(AsRef::as_ref)),
            }
        }

        None
    }
}

#[cfg(test)]
mod tests {
    use super::{BUFFER_CAPACITY, Contents, CoordinateError, FANOUT, Node, Position, Source};
    use crate::Span;
    use std::sync::Arc;

    fn leaves(root: &Arc<Node>) -> Vec<&Arc<Node>> {
        match &root.contents {
            Contents::Piece(_) => vec![root],
            Contents::Children(children) => children.iter().flat_map(leaves).collect(),
        }
    }

    fn validate(root: &Arc<Node>, top: bool) -> usize {
        match &root.contents {
            Contents::Piece(piece) => {
                assert_eq!(root.height, 0);
                assert!(piece.bytes.len() <= BUFFER_CAPACITY);
                assert!(!piece.range.is_empty());
                assert_eq!(root.measure.length, piece.range.len());

                1
            }

            Contents::Children(children) => {
                assert!((2..=FANOUT).contains(&children.len()));

                if !top {
                    assert!(children.len() >= FANOUT / 2);
                }

                let mut count = 0;
                let mut length = 0;

                for child in children {
                    assert_eq!(child.height + 1, root.height);
                    count += validate(child, false);
                    length += child.measure.length;
                }

                assert_eq!(root.measure.length, length);

                count
            }
        }
    }

    #[test]
    fn replacement_shares_unchanged_sides() {
        let source = Source::from(b"abcdef".as_slice());
        let edited = source.replace(Span { start: 2, end: 4 }, b"XY");
        assert_eq!(edited.bytes(), b"abXYef");
        assert_eq!(source.bytes(), b"abcdef");
        let original = source.chunks().next().unwrap();
        let chunks: Vec<_> = edited.chunks().collect();
        assert_eq!(chunks[0].as_ptr(), original.as_ptr());
        assert_eq!(chunks[2].as_ptr(), original[4..].as_ptr());
        let inserted = source.replace(Span { start: 3, end: 3 }, b"!");
        let restored = inserted.replace(Span { start: 3, end: 4 }, b"");
        assert_eq!(restored.bytes(), source.bytes());
        assert_eq!(restored.chunks().count(), 1);
        assert_eq!(restored.bytes().as_ptr(), source.bytes().as_ptr());
    }

    #[test]
    fn repeated_edits_and_slices_preserve_bytes() {
        let source = Source::from(b"abcdef".as_slice());
        let inserted = source.replace(Span { start: 0, end: 0 }, b"@");
        let appended = inserted.replace(Span { start: 7, end: 7 }, b"!");
        let replaced = appended.replace(Span { start: 2, end: 4 }, b"XY");
        let deleted = replaced.replace(Span { start: 1, end: 5 }, b"");
        assert_eq!(inserted.bytes(), b"@abcdef");
        assert_eq!(appended.bytes(), b"@abcdef!");
        assert_eq!(replaced.bytes(), b"@aXYdef!");
        assert_eq!(deleted.bytes(), b"@ef!");
        assert_eq!(deleted.len(), 4);
        let slice = deleted.slice(Span { start: 1, end: 3 });
        assert_eq!(slice.bytes(), b"ef");
        assert_eq!(slice.bytes().as_ptr(), source.bytes()[4..].as_ptr());
        assert_eq!(deleted.slice(Span { start: 0, end: 3 }).bytes(), b"@ef");

        assert!(Arc::ptr_eq(
            deleted.root.as_ref().unwrap(),
            deleted
                .slice(Span { start: 0, end: 4 })
                .root
                .as_ref()
                .unwrap(),
        ));

        assert_eq!(source.bytes(), b"abcdef");
    }

    #[test]
    fn contiguous_bytes_are_lazy_and_clones_share_roots() {
        let source = Source::from(b"abcd".as_slice());
        assert!(!source.materialized());
        let slice = source.slice(Span { start: 1, end: 3 });
        assert_eq!(slice.bytes().as_ptr(), source.bytes()[1..].as_ptr());
        assert!(!slice.materialized());
        let edited = source.replace(Span { start: 2, end: 2 }, b"!");
        let cloned = edited.clone();

        assert!(Arc::ptr_eq(
            edited.root.as_ref().unwrap(),
            cloned.root.as_ref().unwrap()
        ));

        let chunks = edited.chunks();

        assert_eq!(
            chunks.clone().collect::<Vec<_>>(),
            vec![b"ab".as_slice(), b"!", b"cd"]
        );

        assert_eq!(
            chunks.collect::<Vec<_>>(),
            vec![b"ab".as_slice(), b"!", b"cd"]
        );

        assert!(!edited.materialized());
        assert_eq!(edited.bytes(), b"ab!cd");
        assert_eq!(cloned.bytes().as_ptr(), edited.bytes().as_ptr());
        assert!(Arc::ptr_eq(
            edited.contiguous.as_ref().unwrap(),
            cloned.contiguous.as_ref().unwrap()
        ));
    }

    #[test]
    fn empty_sources_and_whole_edits() {
        let empty = Source::from(b"".as_slice());
        assert_eq!(empty.len(), 0);
        assert_eq!(empty.bytes(), b"");
        assert_eq!(empty.chunks().count(), 0);
        assert!(!empty.materialized());
        assert_eq!(empty.slice(Span { start: 0, end: 0 }).bytes(), b"");
        let source = empty.replace(Span { start: 0, end: 0 }, b"abc");
        let deleted = source.replace(Span { start: 0, end: 3 }, b"");
        assert_eq!(deleted.len(), 0);
        assert_eq!(deleted.chunks().count(), 0);
        assert_eq!(deleted.bytes(), b"");

        assert_eq!(
            source.replace(Span { start: 0, end: 3 }, b"xy").bytes(),
            b"xy"
        );

        assert!(Arc::ptr_eq(
            source.root.as_ref().unwrap(),
            source
                .replace(Span { start: 1, end: 1 }, b"")
                .root
                .as_ref()
                .unwrap(),
        ));

        assert_eq!(source.slice(Span { start: 3, end: 3 }).bytes(), b"");
        assert_eq!(source.bytes(), b"abc");
    }

    #[test]
    fn large_edits_share_subtrees_and_bound_retained_buffers() {
        let bytes = vec![b'x'; BUFFER_CAPACITY * FANOUT * 3];
        let original = Source::from(bytes.as_slice());
        let edited = original.replace(Span { start: 2, end: 3 }, b"change");

        let Contents::Children(before) = &original.root.as_ref().unwrap().contents else {
            panic!()
        };

        let Contents::Children(after) = &edited.root.as_ref().unwrap().contents else {
            panic!()
        };

        assert!(
            before
                .iter()
                .skip(1)
                .all(|child| after.iter().any(|other| Arc::ptr_eq(child, other)))
        );

        let original_leaves = leaves(original.root.as_ref().unwrap());
        let edited_leaves = leaves(edited.root.as_ref().unwrap());

        assert!(
            original_leaves
                .iter()
                .skip(1)
                .all(|leaf| edited_leaves.iter().any(|other| Arc::ptr_eq(leaf, other)))
        );

        let tiny = original.slice(Span {
            start: BUFFER_CAPACITY * 20 + 1,
            end: BUFFER_CAPACITY * 20 + 2,
        });

        let Contents::Piece(piece) = &tiny.root.as_ref().unwrap().contents else {
            panic!()
        };

        assert_eq!(piece.bytes.len(), BUFFER_CAPACITY);
        assert_eq!(tiny.bytes(), b"x");
        assert!(!original.materialized());
        assert!(!edited.materialized());
        validate(original.root.as_ref().unwrap(), true);
        validate(edited.root.as_ref().unwrap(), true);
    }

    #[test]
    fn many_edits_remain_balanced_and_preserve_old_sources() {
        let original = Source::from(b"stable".as_slice());
        let mut source = original.clone();
        let mut expected = b"stable".to_vec();
        let mut state = 1usize;

        for step in 0..4000 {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let start = state % (expected.len() + 1);

            let end = if step % 3 == 0 {
                (start + 2).min(expected.len())
            } else {
                start
            };

            let replacement = if step % 5 == 0 {
                b"".as_slice()
            } else {
                b"z".as_slice()
            };

            source = source.replace(Span { start, end }, replacement);
            expected.splice(start..end, replacement.iter().copied());

            if let Some(root) = &source.root {
                let count = validate(root, true);
                let mut capacity = 1;
                let mut maximum = 1;

                while capacity < count {
                    capacity *= 2;
                    maximum += 1;
                }

                assert!(root.height <= maximum);
            }

            if step % 101 == 0 {
                assert_eq!(
                    source.chunks().flatten().copied().collect::<Vec<_>>(),
                    expected
                );

                let middle = expected.len() / 2;
                assert_eq!(source.byte(middle), expected.get(middle).copied());
                assert_eq!(source.byte(source.len()), None);
                assert_eq!(source.byte(usize::MAX), None);

                assert_eq!(
                    source
                        .slice(Span {
                            start: middle,
                            end: expected.len()
                        })
                        .bytes(),
                    &expected[middle..]
                );
            }
        }

        assert_eq!(source.bytes(), expected);
        assert_eq!(original.bytes(), b"stable");
    }

    #[test]
    fn coordinates_across_pieces_and_invalid_bytes() {
        let bytes = b"a\r\n\xc3\xa9\xf0\x9f\x98\x80\xff\rZ\n";
        let mut source = Source::from(b"".as_slice());

        for byte in bytes {
            source = source.replace(
                Span {
                    start: source.len(),
                    end: source.len(),
                },
                &[*byte],
            );
        }

        assert_eq!(source.line_count(), 4);

        for (offset, line, column) in [
            (0, 0, 0),
            (1, 0, 1),
            (3, 1, 0),
            (5, 1, 1),
            (9, 1, 3),
            (10, 1, 4),
            (11, 2, 0),
            (12, 2, 1),
            (13, 3, 0),
        ] {
            let position = Position { line, column };
            assert_eq!(source.position(offset), Ok(position));
            assert_eq!(source.offset(position), Ok(offset));
        }

        for offset in [2, 4, 6, 7, 8] {
            assert_eq!(
                source.position(offset),
                Err(CoordinateError::InvalidBoundary)
            );
        }

        assert_eq!(
            source.offset(Position { line: 1, column: 2 }),
            Err(CoordinateError::InvalidBoundary)
        );

        assert_eq!(
            source.offset(Position { line: 1, column: 5 }),
            Err(CoordinateError::OutOfBounds)
        );

        assert_eq!(
            source.offset(Position {
                line: usize::MAX,
                column: 0
            }),
            Err(CoordinateError::OutOfBounds)
        );

        assert_eq!(source.position(14), Err(CoordinateError::OutOfBounds));
        assert!(!source.materialized());
        assert_eq!(source.bytes(), bytes);
    }

    #[test]
    fn coordinates_reindex_arbitrary_byte_splices() {
        let original = Source::from("\r\né😀".as_bytes());
        let broken = original.replace(Span { start: 3, end: 3 }, b"x");
        assert_eq!(broken.position(5), Ok(Position { line: 1, column: 3 }));
        let restored = broken.replace(Span { start: 3, end: 4 }, b"");
        assert_eq!(restored.position(8), Ok(Position { line: 1, column: 3 }));
        assert_eq!(original.position(3), Err(CoordinateError::InvalidBoundary));
        let separate = original.replace(Span { start: 1, end: 1 }, b"x");
        assert_eq!(separate.line_count(), 3);
        assert_eq!(separate.position(2), Ok(Position { line: 1, column: 1 }));
        assert_eq!(original.line_count(), 2);

        let bytes = [
            0xc0, 0xaf, 0xed, 0xa0, 0x80, 0xf4, 0x90, 0x80, 0x80, 0xe2, 0x82,
        ];

        let invalid = Source::from(bytes.as_slice());

        for offset in 0..=bytes.len() {
            assert_eq!(
                invalid.position(offset),
                Ok(Position {
                    line: 0,
                    column: offset
                })
            );

            assert_eq!(
                invalid.offset(Position {
                    line: 0,
                    column: offset
                }),
                Ok(offset)
            );
        }

        assert!(!original.materialized());
        assert!(!restored.materialized());
    }

    #[test]
    fn coordinates_cross_buffer_and_branch_boundaries_without_materialization() {
        let mut bytes = vec![b'a'; BUFFER_CAPACITY * FANOUT - 1];
        bytes.extend_from_slice("😀\r\né".as_bytes());
        bytes.extend_from_slice(&vec![b'b'; BUFFER_CAPACITY * FANOUT]);
        let source = Source::from(bytes.as_slice());
        let boundary = BUFFER_CAPACITY * FANOUT - 1;
        assert_eq!(source.line_count(), 2);

        assert_eq!(
            source.position(boundary),
            Ok(Position {
                line: 0,
                column: boundary
            })
        );

        assert_eq!(
            source.position(boundary + 4),
            Ok(Position {
                line: 0,
                column: boundary + 2
            })
        );

        assert_eq!(
            source.offset(Position {
                line: 0,
                column: boundary + 1
            }),
            Err(CoordinateError::InvalidBoundary)
        );

        assert_eq!(
            source.position(boundary + 5),
            Err(CoordinateError::InvalidBoundary)
        );

        assert_eq!(
            source.position(boundary + 6),
            Ok(Position { line: 1, column: 0 })
        );

        assert_eq!(
            source.offset(Position { line: 1, column: 1 }),
            Ok(boundary + 8)
        );

        assert_eq!(
            source.position(source.len()),
            Ok(Position {
                line: 1,
                column: BUFFER_CAPACITY * FANOUT + 1
            })
        );

        assert_eq!(
            source.offset(Position {
                line: 1,
                column: BUFFER_CAPACITY * FANOUT + 1
            }),
            Ok(source.len())
        );

        assert!(!source.materialized());
        validate(source.root.as_ref().unwrap(), true);
    }

    #[test]
    fn slices_validate_ranges_and_reindex_boundaries() {
        let source = Source::from("é\r\n😀".as_bytes());
        let continuation = source.slice(Span { start: 1, end: 2 });

        assert_eq!(
            continuation.position(1),
            Ok(Position { line: 0, column: 1 })
        );

        let newline = source.slice(Span { start: 3, end: 4 });
        assert_eq!(newline.line_count(), 2);
        assert_eq!(newline.offset(Position { line: 1, column: 0 }), Ok(1));

        assert_eq!(
            Source::from(b"".as_slice()).position(0),
            Ok(Position { line: 0, column: 0 })
        );

        assert!(std::panic::catch_unwind(|| source.slice(Span { start: 2, end: 1 })).is_err());
        assert!(std::panic::catch_unwind(|| source.slice(Span { start: 0, end: 9 })).is_err());

        assert!(
            std::panic::catch_unwind(|| source.replace(Span { start: 0, end: 9 }, b"")).is_err()
        );
    }
}
