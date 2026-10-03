use std::{
    iter::FusedIterator,
    ops::{Add, Range},
    sync::Arc,
};

use crate::Span;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Measure {
    pub width: usize,
    pub tokens: usize,
    pub nodes: usize,
    pub children: usize,
    pub inspected: Option<Span>,
    pub maximum_depth: usize,
}

impl Add for Measure {
    type Output = Self;

    fn add(self, other: Self) -> Self {
        Self {
            width: self.width + other.width,
            tokens: self.tokens + other.tokens,
            nodes: self.nodes + other.nodes,
            children: self.children + other.children,
            maximum_depth: self.maximum_depth.max(other.maximum_depth),
            inspected: envelope(
                self.inspected,
                other.inspected.map(|span| Span {
                    start: self.width + span.start,
                    end: self.width + span.end,
                }),
            ),
        }
    }
}

pub(crate) fn envelope(left: Option<Span>, right: Option<Span>) -> Option<Span> {
    match (left, right) {
        (Some(left), Some(right)) => Some(Span {
            start: left.start.min(right.start),
            end: left.end.max(right.end),
        }),

        (Some(span), None) | (None, Some(span)) => Some(span),
        (None, None) => None,
    }
}

pub(crate) trait Measured {
    fn measure(&self) -> Measure;
}

#[derive(Debug)]
enum Branch<T> {
    Leaf(T),

    Fork {
        left: Arc<Self>,
        right: Arc<Self>,
        length: usize,
        height: usize,
        measure: Measure,
    },
}

impl<T: Measured> Branch<T> {
    fn length(&self) -> usize {
        match self {
            Self::Leaf(_) => 1,
            Self::Fork { length, .. } => *length,
        }
    }

    fn height(&self) -> usize {
        match self {
            Self::Leaf(_) => 1,
            Self::Fork { height, .. } => *height,
        }
    }

    fn measure(&self) -> Measure {
        match self {
            Self::Leaf(value) => value.measure(),
            Self::Fork { measure, .. } => *measure,
        }
    }
}

#[derive(Debug)]
pub(crate) struct Sequence<T> {
    root: Option<Arc<Branch<T>>>,
}

impl<T> Sequence<T> {
    pub(crate) fn into_unique(self) -> Unique<T> {
        Unique {
            pending: self.root.into_iter().collect(),
        }
    }
}

pub(crate) struct Unique<T> {
    pending: Vec<Arc<Branch<T>>>,
}

impl<T> Iterator for Unique<T> {
    type Item = T;

    fn next(&mut self) -> Option<Self::Item> {
        while let Some(branch) = self.pending.pop() {
            match Arc::into_inner(branch) {
                Some(Branch::Leaf(value)) => return Some(value),

                Some(Branch::Fork { left, right, .. }) => {
                    self.pending.push(right);
                    self.pending.push(left);
                }

                None => {}
            }
        }

        None
    }
}

impl<T> FusedIterator for Unique<T> {}

impl<T> Clone for Sequence<T> {
    fn clone(&self) -> Self {
        Self {
            root: self.root.clone(),
        }
    }
}

impl<T> Default for Sequence<T> {
    fn default() -> Self {
        Self { root: None }
    }
}

fn fork<T: Measured>(left: Arc<Branch<T>>, right: Arc<Branch<T>>) -> Arc<Branch<T>> {
    Arc::new(Branch::Fork {
        length: left.length() + right.length(),
        height: left.height().max(right.height()) + 1,
        measure: left.measure() + right.measure(),
        left,
        right,
    })
}

fn balance<T: Measured>(left: Arc<Branch<T>>, right: Arc<Branch<T>>) -> Arc<Branch<T>> {
    if left.height() > right.height() + 1 {
        let Branch::Fork {
            left: first,
            right: second,
            ..
        } = left.as_ref()
        else {
            unreachable!()
        };

        if first.height() >= second.height() {
            fork(Arc::clone(first), fork(Arc::clone(second), right))
        } else {
            let Branch::Fork {
                left: middle,
                right: last,
                ..
            } = second.as_ref()
            else {
                unreachable!()
            };

            fork(
                fork(Arc::clone(first), Arc::clone(middle)),
                fork(Arc::clone(last), right),
            )
        }
    } else if right.height() > left.height() + 1 {
        let Branch::Fork {
            left: first,
            right: second,
            ..
        } = right.as_ref()
        else {
            unreachable!()
        };

        if second.height() >= first.height() {
            fork(fork(left, Arc::clone(first)), Arc::clone(second))
        } else {
            let Branch::Fork {
                left: middle,
                right: last,
                ..
            } = first.as_ref()
            else {
                unreachable!()
            };

            fork(
                fork(left, Arc::clone(middle)),
                fork(Arc::clone(last), Arc::clone(second)),
            )
        }
    } else {
        fork(left, right)
    }
}

fn join<T: Measured>(left: Arc<Branch<T>>, right: Arc<Branch<T>>) -> Arc<Branch<T>> {
    if left.height() > right.height() + 1 {
        let Branch::Fork {
            left: first,
            right: second,
            ..
        } = left.as_ref()
        else {
            unreachable!()
        };

        balance(Arc::clone(first), join(Arc::clone(second), right))
    } else if right.height() > left.height() + 1 {
        let Branch::Fork {
            left: first,
            right: second,
            ..
        } = right.as_ref()
        else {
            unreachable!()
        };

        balance(join(left, Arc::clone(first)), Arc::clone(second))
    } else {
        fork(left, right)
    }
}

impl<T: Measured> Sequence<T> {
    pub(crate) fn singleton(value: T) -> Self {
        Self {
            root: Some(Arc::new(Branch::Leaf(value))),
        }
    }

    pub(crate) fn len(&self) -> usize {
        self.root.as_ref().map_or(0, |root| root.length())
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.root.is_none()
    }

    pub(crate) fn measure(&self) -> Measure {
        self.root
            .as_ref()
            .map_or(Measure::default(), |root| root.measure())
    }

    pub(crate) fn get(&self, mut index: usize) -> Option<&T> {
        let mut branch = self.root.as_deref()?;

        if index >= branch.length() {
            return None;
        }

        loop {
            match branch {
                Branch::Leaf(value) => return Some(value),

                Branch::Fork { left, right, .. } => {
                    if index < left.length() {
                        branch = left;
                    } else {
                        index -= left.length();
                        branch = right;
                    }
                }
            }
        }
    }

    pub(crate) fn prefix(&self, mut index: usize) -> Measure {
        assert!(index <= self.len());
        let mut measure = Measure::default();

        let Some(mut branch) = self.root.as_deref() else {
            return measure;
        };

        loop {
            if index == branch.length() {
                return measure + branch.measure();
            }

            match branch {
                Branch::Leaf(_) => return measure,

                Branch::Fork { left, right, .. } => {
                    if index < left.length() {
                        branch = left;
                    } else {
                        measure = measure + left.measure();
                        index -= left.length();
                        branch = right;
                    }
                }
            }
        }
    }

    pub(crate) fn select(
        &self,
        mut ordinal: usize,
        weight: impl Fn(Measure) -> usize,
    ) -> Option<(usize, &T, Measure)> {
        let mut branch = self.root.as_deref()?;

        if ordinal >= weight(branch.measure()) {
            return None;
        }

        let mut index = 0;
        let mut prefix = Measure::default();

        loop {
            match branch {
                Branch::Leaf(value) => return Some((index, value, prefix)),

                Branch::Fork { left, right, .. } => {
                    let count = weight(left.measure());

                    if ordinal < count {
                        branch = left;
                    } else {
                        ordinal -= count;
                        index += left.length();
                        prefix = prefix + left.measure();
                        branch = right;
                    }
                }
            }
        }
    }

    pub(crate) fn concat(&self, other: &Self) -> Self {
        Self {
            root: match (&self.root, &other.root) {
                (Some(left), Some(right)) => Some(join(Arc::clone(left), Arc::clone(right))),
                (Some(root), None) | (None, Some(root)) => Some(Arc::clone(root)),
                (None, None) => None,
            },
        }
    }

    pub(crate) fn concatenate(values: impl IntoIterator<Item = Self>) -> Self {
        fn build<T: Measured>(values: &[Arc<Branch<T>>]) -> Arc<Branch<T>> {
            if values.len() == 1 {
                return Arc::clone(&values[0]);
            }

            let middle = values.len() / 2;

            join(build(&values[..middle]), build(&values[middle..]))
        }

        let mut values = values.into_iter().filter_map(|sequence| sequence.root);
        let Some(first) = values.next() else {
            return Self::default();
        };
        let Some(second) = values.next() else {
            return Self { root: Some(first) };
        };
        let values: Vec<_> = [first, second].into_iter().chain(values).collect();

        Self {
            root: Some(build(&values)),
        }
    }

    pub(crate) fn slice(&self, range: Range<usize>) -> Self {
        fn extract<T: Measured>(branch: &Arc<Branch<T>>, range: Range<usize>) -> Sequence<T> {
            if range.is_empty() {
                return Sequence::default();
            }

            if range.start == 0 && range.end == branch.length() {
                return Sequence {
                    root: Some(Arc::clone(branch)),
                };
            }

            let Branch::Fork { left, right, .. } = branch.as_ref() else {
                unreachable!()
            };

            let length = left.length();

            if range.end <= length {
                extract(left, range)
            } else if range.start >= length {
                extract(right, range.start - length..range.end - length)
            } else {
                extract(left, range.start..length).concat(&extract(right, 0..range.end - length))
            }
        }

        assert!(range.start <= range.end && range.end <= self.len());

        self.root
            .as_ref()
            .map_or_else(Self::default, |root| extract(root, range))
    }

    pub(crate) fn splice(&self, range: Range<usize>, replacement: &Self) -> Self {
        self.slice(0..range.start)
            .concat(replacement)
            .concat(&self.slice(range.end..self.len()))
    }

    pub(crate) fn split_width(
        &self,
        offset: usize,
        split: impl Fn(&T, usize) -> (T, T),
    ) -> (Self, Self) {
        assert!(offset <= self.measure().width);

        let Some(mut branch) = self.root.as_deref() else {
            return (Self::default(), Self::default());
        };

        let mut index = 0;
        let mut position = offset;

        let value = loop {
            match branch {
                Branch::Leaf(value) => break value,

                Branch::Fork { left, right, .. } => {
                    if position <= left.measure().width {
                        branch = left;
                    } else {
                        position -= left.measure().width;
                        index += left.length();
                        branch = right;
                    }
                }
            }
        };

        if position == 0 {
            return (self.slice(0..index), self.slice(index..self.len()));
        }

        if position == value.measure().width {
            return (self.slice(0..index + 1), self.slice(index + 1..self.len()));
        }

        let (left, right) = split(value, position);

        (
            self.slice(0..index).concat(&Self::singleton(left)),
            Self::singleton(right).concat(&self.slice(index + 1..self.len())),
        )
    }

    pub(crate) fn iter(&self) -> Iter<'_, T> {
        Iter {
            sequence: self,
            indices: 0..self.len(),
        }
    }

    pub(crate) fn intersecting(&self, range: Span) -> Intersecting<'_, T> {
        Intersecting {
            pending: self
                .root
                .as_deref()
                .map(|root| (root, 0, Measure::default()))
                .into_iter()
                .collect(),
            range,
        }
    }
}

impl<T: Measured> FromIterator<T> for Sequence<T> {
    fn from_iter<I: IntoIterator<Item = T>>(values: I) -> Self {
        Self::concatenate(values.into_iter().map(Self::singleton))
    }
}

pub(crate) struct Iter<'sequence, T> {
    sequence: &'sequence Sequence<T>,
    indices: Range<usize>,
}

impl<T> Clone for Iter<'_, T> {
    fn clone(&self) -> Self {
        Self {
            sequence: self.sequence,
            indices: self.indices.clone(),
        }
    }
}

impl<'sequence, T: Measured> Iterator for Iter<'sequence, T> {
    type Item = &'sequence T;

    fn next(&mut self) -> Option<Self::Item> {
        self.indices
            .next()
            .and_then(|index| self.sequence.get(index))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.indices.size_hint()
    }
}

impl<T: Measured> DoubleEndedIterator for Iter<'_, T> {
    fn next_back(&mut self) -> Option<Self::Item> {
        self.indices
            .next_back()
            .and_then(|index| self.sequence.get(index))
    }
}

impl<T: Measured> ExactSizeIterator for Iter<'_, T> {}
impl<T: Measured> FusedIterator for Iter<'_, T> {}

pub(crate) struct Intersecting<'sequence, T> {
    pending: Vec<(&'sequence Branch<T>, usize, Measure)>,
    range: Span,
}

impl<T> Clone for Intersecting<'_, T> {
    fn clone(&self) -> Self {
        Self {
            pending: self.pending.clone(),
            range: self.range,
        }
    }
}

impl<'sequence, T: Measured> Iterator for Intersecting<'sequence, T> {
    type Item = (usize, &'sequence T, Measure);

    fn next(&mut self) -> Option<Self::Item> {
        while let Some((branch, index, prefix)) = self.pending.pop() {
            let Some(span) = branch.measure().inspected else {
                continue;
            };

            if prefix.width + span.start > self.range.end
                || prefix.width + span.end < self.range.start
            {
                continue;
            }

            match branch {
                Branch::Leaf(value) => return Some((index, value, prefix)),

                Branch::Fork { left, right, .. } => {
                    self.pending
                        .push((right, index + left.length(), prefix + left.measure()));

                    self.pending.push((left, index, prefix));
                }
            }
        }

        None
    }
}

impl<T: Measured> FusedIterator for Intersecting<'_, T> {}

#[cfg(test)]
mod tests {
    use super::*;

    impl Measured for usize {
        fn measure(&self) -> Measure {
            Measure {
                width: *self,
                tokens: 1,
                nodes: 0,
                children: 0,
                inspected: None,
                maximum_depth: 0,
            }
        }
    }

    fn validate(branch: &Branch<usize>) -> usize {
        match branch {
            Branch::Leaf(_) => 1,

            Branch::Fork {
                left,
                right,
                height,
                length,
                measure,
            } => {
                let first = validate(left);
                let second = validate(right);
                assert!(first.abs_diff(second) <= 1);
                assert_eq!(*height, first.max(second) + 1);
                assert_eq!(*length, left.length() + right.length());
                assert_eq!(*measure, left.measure() + right.measure());

                *height
            }
        }
    }

    #[test]
    fn persistent_splices() {
        let original: Sequence<_> = (1..=4096).collect();
        let mut edited = original.clone();
        let mut expected: Vec<_> = (1..=4096).collect();

        for position in 0..512 {
            let replacement = Sequence::singleton(9000 + position);
            edited = edited.splice(position * 3..position * 3 + 1, &replacement);
            expected[position * 3] = 9000 + position;
            validate(edited.root.as_deref().unwrap());
        }

        assert_eq!(edited.iter().copied().collect::<Vec<_>>(), expected);
        assert_eq!(original.get(0), Some(&1));

        assert!(std::ptr::eq(
            original.get(4095).unwrap(),
            edited.get(4095).unwrap()
        ));

        assert_eq!(edited.measure().width, expected.iter().sum());

        for (index, value) in expected.iter().enumerate() {
            let (selected, item, prefix) = edited.select(index, |measure| measure.tokens).unwrap();
            assert_eq!((selected, item), (index, value));
            assert_eq!(prefix, edited.prefix(index));
        }

        assert_eq!(
            edited.slice(17..123).iter().copied().collect::<Vec<_>>(),
            expected[17..123]
        );
    }

    #[test]
    fn changing_lengths_remain_balanced() {
        let mut expected: Vec<usize> = (1..=1024).collect();
        let mut sequence: Sequence<_> = expected.iter().copied().collect();

        for step in 0..1024 {
            let position = step * 17 % expected.len();
            let removed = (step % 4).min(expected.len() - position);
            let replacement: Vec<_> = (0..step % 7).map(|offset| step + offset + 1).collect();

            sequence = sequence.splice(
                position..position + removed,
                &replacement.iter().copied().collect(),
            );

            expected
                .splice(position..position + removed, replacement)
                .for_each(drop);

            validate(sequence.root.as_deref().unwrap());
            assert_eq!(sequence.iter().copied().collect::<Vec<_>>(), expected);
            assert_eq!(sequence.measure().width, expected.iter().sum());
            assert_eq!(sequence.measure().tokens, expected.len());
        }
    }
}
