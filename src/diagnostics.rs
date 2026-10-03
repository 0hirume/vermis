use std::{collections::HashMap, ops::Range, sync::Arc};

use crate::{
    Diagnostic, Span,
    sequence::{Measure, Measured, Sequence},
    tree::Node,
};

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub enum DiagnosticCode {
    ExpectedSyntax,
    InvalidSyntax,
    InvalidNumber,
    InvalidEscape,
    UnterminatedString,
    UnterminatedComment,
    UnexpectedCharacter,
    InvalidInterpolation,
    BlockEndingStatement,
    NestingLimit,
}

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub enum Severity {
    Error,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Owned {
    pub message: &'static str,
    pub length: usize,
}

#[derive(Clone, Debug)]
enum Position {
    Gap(usize),
    Anchor(Arc<Owned>),
}

impl Measured for Position {
    fn measure(&self) -> Measure {
        match self {
            Self::Gap(width) => Measure {
                width: *width,
                ..Measure::default()
            },

            Self::Anchor(record) => Measure {
                inspected: Some(Span {
                    start: 0,
                    end: record.length,
                }),
                ..Measure::default()
            },
        }
    }
}

#[derive(Clone, Debug, Default)]
pub(crate) struct Positions {
    sequence: Sequence<Position>,
}

impl Positions {
    pub(crate) fn new(width: usize, records: impl IntoIterator<Item = (Span, Arc<Owned>)>) -> Self {
        let mut records: Vec<_> = records.into_iter().collect();
        records.sort_by_key(|(span, _)| span.start);
        let mut positions = Vec::with_capacity(records.len() * 2 + 1);
        let mut previous = 0;

        for (span, record) in records {
            assert_eq!(span.len(), record.length);
            positions.push(Position::Gap(span.start - previous));
            positions.push(Position::Anchor(record));
            previous = span.start;
        }

        positions.push(Position::Gap(width.max(previous) - previous));

        Self {
            sequence: positions.into_iter().collect(),
        }
    }

    pub(crate) fn inspected(&self) -> Option<Span> {
        self.sequence.measure().inspected
    }

    pub(crate) fn equivalent(&self, other: &Self) -> bool {
        self.sequence.len() == other.sequence.len()
            && self
                .sequence
                .iter()
                .zip(other.sequence.iter())
                .all(|(left, right)| match (left, right) {
                    (Position::Gap(left), Position::Gap(right)) => left == right,
                    (Position::Anchor(left), Position::Anchor(right)) => left == right,
                    _ => false,
                })
    }

    pub(crate) fn splice(&self, edit: Span, replacement_width: usize) -> Option<Self> {
        if edit.start > edit.end
            || edit.end > self.sequence.measure().width
            || self.sequence.intersecting(edit).next().is_some()
        {
            return None;
        }

        let (index, position, prefix) = if edit.start == self.sequence.measure().width {
            let index = self.sequence.len().checked_sub(1)?;

            (
                index,
                self.sequence.get(index)?,
                self.sequence.prefix(index),
            )
        } else {
            self.sequence.select(edit.start, |measure| measure.width)?
        };

        let Position::Gap(width) = position else {
            return None;
        };

        if edit.end > prefix.width + width {
            return None;
        }

        let gap = Sequence::singleton(Position::Gap(width - edit.len() + replacement_width));

        Some(Self {
            sequence: self.sequence.splice(index..index + 1, &gap),
        })
    }

    fn starts(&self) -> HashMap<*const Owned, usize> {
        let mut starts = HashMap::new();
        let mut start = 0;

        for position in self.sequence.iter() {
            match position {
                Position::Gap(width) => start += width,

                Position::Anchor(record) => {
                    starts.insert(Arc::as_ptr(record), start);
                }
            }
        }

        starts
    }
}

#[derive(Clone, Debug)]
enum Event {
    Direct(Arc<Owned>),
    Child(Arc<Node>),
}

impl Measured for Event {
    fn measure(&self) -> Measure {
        match self {
            Self::Direct(_) => Measure {
                tokens: 1,
                nodes: 1,
                ..Measure::default()
            },

            Self::Child(node) => Measure {
                nodes: node.events.len(),
                children: 1,
                ..Measure::default()
            },
        }
    }
}

#[derive(Clone, Debug, Default)]
pub(crate) struct Events {
    sequence: Sequence<Event>,
}

impl Events {
    pub(crate) fn new(
        direct: impl IntoIterator<Item = (usize, Arc<Owned>)>,
        children: impl IntoIterator<Item = (Arc<Node>, usize)>,
    ) -> Self {
        let mut events: Vec<_> = children
            .into_iter()
            .map(|(child, ordinal)| (ordinal, Event::Child(child)))
            .collect();

        events.extend(
            direct
                .into_iter()
                .map(|(ordinal, record)| (ordinal, Event::Direct(record))),
        );

        events.sort_by_key(|(ordinal, _)| *ordinal);

        Self {
            sequence: events.into_iter().map(|(_, event)| event).collect(),
        }
    }

    pub(crate) fn len(&self) -> usize {
        self.sequence.measure().nodes
    }

    pub(crate) fn equivalent(
        &self,
        other: &Self,
        left_positions: &Positions,
        right_positions: &Positions,
    ) -> bool {
        if self.sequence.len() != other.sequence.len() {
            return false;
        }

        let left_positions = left_positions.starts();
        let right_positions = right_positions.starts();

        self.sequence
            .iter()
            .zip(other.sequence.iter())
            .all(|(left, right)| match (left, right) {
                (Event::Direct(left), Event::Direct(right)) => {
                    left == right
                        && match (
                            left_positions.get(&Arc::as_ptr(left)),
                            right_positions.get(&Arc::as_ptr(right)),
                        ) {
                            (Some(left), Some(right)) => left == right,
                            _ => false,
                        }
                }

                (Event::Child(_), Event::Child(_)) => true,
                _ => false,
            })
    }

    pub(crate) fn can_splice_children(
        &self,
        range: Range<usize>,
        replacement_children: usize,
    ) -> bool {
        range.start <= range.end
            && range.end <= self.sequence.measure().children
            && (self.sequence.measure().tokens == 0 || range.len() == replacement_children)
    }

    pub(crate) fn splice_children(
        &self,
        range: Range<usize>,
        replacement: impl IntoIterator<Item = Arc<Node>>,
    ) -> Option<Self> {
        let replacement: Vec<_> = replacement.into_iter().collect();

        if !self.can_splice_children(range.clone(), replacement.len()) {
            return None;
        }

        let child_slot = |ordinal| {
            self.sequence
                .select(ordinal, |measure| measure.children)
                .map_or(self.sequence.len(), |(index, _, _)| index)
        };

        if self.sequence.measure().tokens == 0 {
            let events = replacement.into_iter().map(Event::Child).collect();

            return Some(Self {
                sequence: self
                    .sequence
                    .splice(child_slot(range.start)..child_slot(range.end), &events),
            });
        }

        let mut sequence = self.sequence.clone();

        for (ordinal, child) in range.zip(replacement) {
            let slot = child_slot(ordinal);
            sequence = sequence.splice(slot..slot + 1, &Sequence::singleton(Event::Child(child)));
        }

        Some(Self { sequence })
    }

    pub(crate) fn clear(&mut self) {
        self.sequence = Sequence::default();
    }
}

pub(crate) fn materialize(root: &Arc<Node>) -> Vec<Diagnostic> {
    struct Frame<'syntax> {
        node: &'syntax Arc<Node>,
        start: usize,
        ordinal: usize,
        positions: Option<HashMap<*const Owned, usize>>,
    }

    let mut diagnostics = Vec::with_capacity(root.events.len());

    let mut pending = vec![Frame {
        node: root,
        start: 0,
        ordinal: 0,
        positions: None,
    }];

    while let Some(frame) = pending.last_mut() {
        if frame.ordinal == frame.node.events.len() {
            pending.pop();
            continue;
        }

        let owner = frame.node;

        let (_, event, prefix) = owner
            .events
            .sequence
            .select(frame.ordinal, |measure| measure.nodes)
            .expect("diagnostic event exists");

        match event {
            Event::Direct(record) => {
                let positions = frame
                    .positions
                    .get_or_insert_with(|| owner.positions.starts());

                let start = frame.start + positions[&Arc::as_ptr(record)];

                diagnostics.push(Diagnostic {
                    span: Span {
                        start,
                        end: start + record.length,
                    },
                    message: record.message,
                });

                frame.ordinal += 1;
            }

            Event::Child(node) => {
                let (_, _, child) = owner
                    .child(prefix.children)
                    .expect("child event has syntax slot");

                let start = frame.start + child.width;
                frame.ordinal += node.events.len();

                pending.push(Frame {
                    node,
                    start,
                    ordinal: 0,
                    positions: None,
                });
            }
        }
    }

    diagnostics
}

impl Diagnostic {
    #[must_use]
    pub fn code(&self) -> DiagnosticCode {
        match self.message {
            "malformed number" => DiagnosticCode::InvalidNumber,
            "malformed string escape" => DiagnosticCode::InvalidEscape,
            "unterminated string" => DiagnosticCode::UnterminatedString,
            "unterminated comment" => DiagnosticCode::UnterminatedComment,
            "unexpected character" => DiagnosticCode::UnexpectedCharacter,
            "invalid interpolation delimiter" => DiagnosticCode::InvalidInterpolation,
            "statement follows a block-ending statement" => DiagnosticCode::BlockEndingStatement,
            "syntax nesting limit exceeded" => DiagnosticCode::NestingLimit,

            message if message.starts_with("expected ") || message.starts_with("missing ") => {
                DiagnosticCode::ExpectedSyntax
            }

            _ => DiagnosticCode::InvalidSyntax,
        }
    }

    #[must_use]
    pub const fn severity(&self) -> Severity {
        Severity::Error
    }
}
