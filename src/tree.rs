use std::{
    collections::{HashMap, HashSet},
    ops::Range,
    sync::{Arc, OnceLock},
};

use crate::{
    Span, Token, TokenKind,
    diagnostics::{Events, Owned, Positions},
    lexer::Checkpoint,
    parser::{
        builder::Builder,
        context::{Boundary, Context, Expectation, Expected},
        control::{Execution, ParseError},
    },
    sequence::{Measure, Measured, Sequence, envelope},
    source::Source,
    update::Reuse,
};

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub enum Kind {
    Root,
    Block,
    Error,
    Missing,

    Name,
    Number,
    String,
    Boolean,
    Nil,
    Variadic,
    Operator,

    Local,
    Constant,
    Assignment,
    CompoundAssignment,
    CallStatement,

    Function,
    LocalFunction,
    FunctionName,
    Parameters,
    Binding,
    Returns,

    If,
    Branch,
    Else,
    While,
    Repeat,
    NumericFor,
    GenericFor,
    Do,
    Return,
    Break,
    Continue,

    Export,
    TypeAlias,
    TypeFunction,
    Declaration,

    Class,
    Property,
    Method,
    Extends,

    Attributes,
    Attribute,
    Arguments,
    Generics,
    Generic,
    GenericPack,

    Unary,
    Binary,
    Group,
    Call,
    MethodCall,
    Field,
    Index,
    Instantiate,
    Assertion,
    Conditional,
    Interpolation,
    Table,
    TableField,

    TypeName,
    TypeTable,
    TypeField,
    TypeIndexer,
    TypeFunctionExpression,
    TypeGroup,
    TypePack,
    VariadicType,
    TypeParameter,
    TypeArguments,
    TypeUnion,
    TypeIntersection,
    TypeOptional,
    TypeOf,
}

#[derive(Clone, Debug)]
struct InspectionRun {
    width: usize,
    inspected: bool,
}

impl Measured for InspectionRun {
    fn measure(&self) -> Measure {
        Measure {
            width: self.width,
            inspected: self.inspected.then_some(Span {
                start: 0,
                end: self.width,
            }),
            ..Measure::default()
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct Inspection {
    runs: Sequence<InspectionRun>,
}

impl Inspection {
    fn new(width: usize, spans: impl IntoIterator<Item = Span>) -> Self {
        let mut spans: Vec<_> = spans.into_iter().collect();
        spans.sort_by_key(|span| (span.start, span.end));
        let mut merged: Vec<Span> = Vec::new();
        let mut points = Vec::new();

        for span in spans {
            if span.is_empty() {
                points.push(span.start);
                continue;
            }

            if let Some(previous) = merged.last_mut()
                && span.start <= previous.end
            {
                previous.end = previous.end.max(span.end);
            } else {
                merged.push(span);
            }
        }

        let mut runs = Vec::new();
        let mut cursor = 0;

        for span in merged {
            if span.start > cursor {
                runs.push(InspectionRun {
                    width: span.start - cursor,
                    inspected: false,
                });
            }

            runs.push(InspectionRun {
                width: span.len(),
                inspected: true,
            });

            cursor = span.end;
        }

        let extent = width.max(points.last().copied().unwrap_or(0));

        if extent > cursor {
            runs.push(InspectionRun {
                width: extent - cursor,
                inspected: false,
            });
        }

        let mut runs: Sequence<_> = runs.into_iter().collect();
        points.dedup();

        for point in points {
            let (prefix, suffix) = runs.split_width(point, |run, position| {
                (
                    InspectionRun {
                        width: position,
                        inspected: run.inspected,
                    },
                    InspectionRun {
                        width: run.width - position,
                        inspected: run.inspected,
                    },
                )
            });

            runs = prefix
                .concat(&Sequence::singleton(InspectionRun {
                    width: 0,
                    inspected: true,
                }))
                .concat(&suffix);
        }

        Self { runs }
    }

    pub(crate) fn reach(&self) -> Option<Span> {
        self.runs.measure().inspected
    }

    pub(crate) fn intersects(&self, edit: Span) -> bool {
        self.runs.intersecting(edit).next().is_some()
    }

    pub(crate) fn intersects_nonempty(&self, edit: Span) -> bool {
        self.runs.intersecting(edit).any(|(_, run, prefix)| {
            run.width != 0
                && prefix.width + run.width > edit.start
                && if edit.is_empty() {
                    prefix.width <= edit.start
                } else {
                    prefix.width < edit.end
                }
        })
    }

    pub(crate) fn spans(&self) -> impl Iterator<Item = Span> + Clone + '_ {
        self.runs
            .iter()
            .scan(0, |cursor, run| {
                let start = *cursor;
                *cursor += run.width;

                Some(run.inspected.then_some(Span {
                    start,
                    end: *cursor,
                }))
            })
            .flatten()
    }

    fn splice(&self, edit: Span, replacement_width: usize) -> Option<Self> {
        if self.intersects(edit) && self.intersects_nonempty(edit) {
            return None;
        }

        if edit.start > self.runs.measure().width {
            return Some(self.clone());
        }

        let gap = |width| InspectionRun {
            width,
            inspected: false,
        };

        let (prefix, removed, suffix) = split_edit(&self.runs, edit, gap);
        let mut replacement = Sequence::singleton(gap(replacement_width));

        if removed.iter().any(|run| run.inspected)
            && !suffix
                .get(0)
                .is_some_and(|run| run.inspected && run.width == 0)
        {
            replacement = replacement.concat(&Sequence::singleton(InspectionRun {
                width: 0,
                inspected: true,
            }));
        }

        Some(Self {
            runs: prefix.concat(&replacement).concat(&suffix),
        })
    }
}

impl PartialEq for Inspection {
    fn eq(&self, other: &Self) -> bool {
        self.spans().eq(other.spans())
    }
}

impl Eq for Inspection {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Contract {
    pub context: Context,
    pub consumed: Span,
    pub inspected: Inspection,
    pub exit: crate::lexer::State,
    pub current: Token,
    pub maximum_depth: usize,
}

impl Contract {
    fn freeze(boundary: Boundary, start: usize, width: usize) -> Self {
        let relative = |span: Span| Span {
            start: span.start - start,
            end: span.end - start,
        };

        Self {
            context: boundary.context,
            consumed: relative(boundary.consumed),
            inspected: Inspection::new(width, boundary.inspected.into_iter().map(relative)),
            exit: boundary.exit,
            current: Token {
                kind: boundary.current.kind,
                span: relative(boundary.current.span),
            },
            maximum_depth: boundary.maximum_depth,
        }
    }

    pub(crate) fn equivalent(&self, other: &Self, lexical: &mut HashSet<(usize, usize)>) -> bool {
        self.consumed == other.consumed
            && self.current == other.current
            && self.maximum_depth == other.maximum_depth
            && self.inspected == other.inspected
            && self.context.equivalent(&other.context, lexical)
            && self.exit.equivalent(&other.exit, lexical)
    }

    fn splice(&self, edit: Span, replacement_width: usize) -> Option<Self> {
        let mut consumed = shifted(self.consumed, edit, replacement_width);

        if self.consumed.start <= edit.start {
            consumed.start = self.consumed.start;
        }

        Some(Self {
            context: self.context.clone(),
            consumed,
            inspected: self.inspected.splice(edit, replacement_width)?,
            exit: self.exit.clone(),
            current: Token {
                kind: self.current.kind,
                span: shifted(self.current.span, edit, replacement_width),
            },
            maximum_depth: self.maximum_depth,
        })
    }
}

#[derive(Clone, Debug)]
struct RecoveryRun {
    width: usize,
    expected: Option<Expected>,
    length: usize,
}

impl Measured for RecoveryRun {
    fn measure(&self) -> Measure {
        Measure {
            width: self.width,
            inspected: self.expected.as_ref().map(|_| Span {
                start: 0,
                end: self.length,
            }),
            ..Measure::default()
        }
    }
}

#[derive(Clone, Debug, Default)]
pub(crate) struct Recovery {
    runs: Sequence<RecoveryRun>,
}

impl Recovery {
    fn new(width: usize, expectations: impl IntoIterator<Item = Expectation>) -> Self {
        let mut expectations: Vec<_> = expectations.into_iter().collect();
        expectations.sort_by_key(|expectation| (expectation.span.start, expectation.span.end));
        let mut runs = Vec::new();
        let mut cursor = 0;

        for expectation in expectations {
            if expectation.span.start > cursor {
                runs.push(RecoveryRun {
                    width: expectation.span.start - cursor,
                    expected: None,
                    length: 0,
                });
            }

            runs.push(RecoveryRun {
                width: 0,
                expected: Some(expectation.expected),
                length: expectation.span.len(),
            });

            cursor = expectation.span.start;
        }

        if width > cursor {
            runs.push(RecoveryRun {
                width: width - cursor,
                expected: None,
                length: 0,
            });
        }

        Self {
            runs: runs.into_iter().collect(),
        }
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = Expectation> + Clone + '_ {
        self.runs
            .iter()
            .scan(0, |cursor, run| {
                let start = *cursor;
                *cursor += run.width;

                Some(run.expected.clone().map(|expected| Expectation {
                    span: Span {
                        start,
                        end: start + run.length,
                    },
                    expected,
                }))
            })
            .flatten()
    }

    fn splice(&self, edit: Span, replacement_width: usize) -> Option<Self> {
        if self.runs.intersecting(edit).next().is_some() {
            return None;
        }

        if edit.start > self.runs.measure().width {
            return Some(self.clone());
        }

        let gap = |width| RecoveryRun {
            width,
            expected: None,
            length: 0,
        };

        let (prefix, _, suffix) = split_edit(&self.runs, edit, gap);

        Some(Self {
            runs: prefix
                .concat(&Sequence::singleton(gap(replacement_width)))
                .concat(&suffix),
        })
    }
}

impl PartialEq for Recovery {
    fn eq(&self, other: &Self) -> bool {
        self.iter().eq(other.iter())
    }
}

impl Eq for Recovery {}

fn split_edit<T: Measured>(
    runs: &Sequence<T>,
    edit: Span,
    gap: impl Fn(usize) -> T,
) -> (Sequence<T>, Sequence<T>, Sequence<T>) {
    let mut runs = runs.clone();

    if edit.end > runs.measure().width {
        runs = runs.concat(&Sequence::singleton(gap(edit.end - runs.measure().width)));
    }

    let split = |run: &T, position| (gap(position), gap(run.measure().width - position));
    let (prefix, remainder) = runs.split_width(edit.start, split);
    let (removed, suffix) = remainder.split_width(edit.len(), split);

    (prefix, removed, suffix)
}

pub(crate) struct Node {
    pub identity: Arc<()>,
    pub kind: Kind,
    pub width: usize,
    pub edges: Sequence<Syntax>,
    pub tokens: Sequence<Arc<Leaf>>,
    pub count: usize,
    pub maximum_depth: usize,
    pub boundaries: Box<[Contract]>,
    pub recovery: Recovery,
    pub positions: Positions,
    pub events: Events,
    pub inspected: Option<Span>,
    text: OnceLock<Arc<[u8]>>,
}

impl std::fmt::Debug for Node {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Node")
            .field("kind", &self.kind)
            .field("width", &self.width)
            .field("children", &self.edges.measure().children)
            .field("tokens", &self.tokens.len())
            .field("nodes", &self.count)
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Debug)]
pub(crate) enum Syntax {
    Node(Arc<Node>),
    Token(Arc<Leaf>),
}

#[derive(Debug)]
pub(crate) struct Leaf {
    pub kind: TokenKind,
    pub source: Source,
    pub checkpoint: Checkpoint,
}

impl Measured for Arc<Leaf> {
    fn measure(&self) -> Measure {
        Measure {
            width: self.source.len(),
            tokens: 1,
            nodes: 0,
            children: 0,
            inspected: None,
            maximum_depth: self.checkpoint.state.braces.len(),
        }
    }
}

impl Measured for Syntax {
    fn measure(&self) -> Measure {
        match self {
            Self::Node(node) => Measure {
                width: node.width,
                tokens: node.tokens.len(),
                nodes: node.count,
                children: 1,
                inspected: node.inspected,
                maximum_depth: node.maximum_depth,
            },

            Self::Token(leaf) => leaf.measure(),
        }
    }
}

impl Node {
    fn tokens(edges: &Sequence<Syntax>) -> Sequence<Arc<Leaf>> {
        Sequence::concatenate(edges.iter().map(|edge| match edge {
            Syntax::Node(node) => node.tokens.clone(),
            Syntax::Token(leaf) => Sequence::singleton(Arc::clone(leaf)),
        }))
    }

    fn assembled(kind: Kind, edges: Sequence<Syntax>, tokens: Sequence<Arc<Leaf>>) -> Arc<Self> {
        Arc::new(Self {
            identity: Arc::new(()),
            kind,
            width: edges.measure().width,
            count: edges.measure().nodes + 1,
            maximum_depth: edges.measure().maximum_depth,
            boundaries: Box::default(),
            recovery: Recovery::default(),
            positions: Positions::default(),
            events: Events::default(),
            inspected: edges.measure().inspected,
            edges,
            tokens,
            text: OnceLock::new(),
        })
    }

    pub(crate) fn splice_edit(
        &self,
        range: Range<usize>,
        replacement: &Sequence<Syntax>,
        edit: Span,
        replacement_width: usize,
    ) -> Option<Arc<Self>> {
        let begin = self.edges.prefix(range.start).tokens;
        let end = self.edges.prefix(range.end).tokens;

        let children =
            self.edges.prefix(range.start).children..self.edges.prefix(range.end).children;

        let tokens = Self::tokens(replacement);

        let mut node = Self::assembled(
            self.kind,
            self.edges.splice(range, replacement),
            self.tokens.splice(begin..end, &tokens),
        );

        let syntax = Arc::get_mut(&mut node).expect("new syntax is unique");

        syntax.boundaries = self
            .boundaries
            .iter()
            .map(|boundary| boundary.splice(edit, replacement_width))
            .collect::<Option<Box<[_]>>>()?;

        syntax.recovery = self.recovery.splice(edit, replacement_width)?;
        syntax.positions = self.positions.splice(edit, replacement_width)?;

        syntax.events = self.events.splice_children(
            children,
            replacement.iter().filter_map(|edge| match edge {
                Syntax::Node(child) => Some(Arc::clone(child)),
                Syntax::Token(_) => None,
            }),
        )?;

        syntax.refresh();

        Some(node)
    }

    pub(crate) fn child(&self, position: usize) -> Option<(usize, &Arc<Node>, Measure)> {
        let (index, syntax, prefix) = self.edges.select(position, |measure| measure.children)?;

        let Syntax::Node(node) = syntax else {
            unreachable!()
        };

        Some((index, node, prefix))
    }

    fn refresh(&mut self) {
        self.inspected = self
            .boundaries
            .iter()
            .fold(self.edges.measure().inspected, |reach, boundary| {
                envelope(reach, boundary.inspected.reach())
            });

        self.inspected = envelope(self.inspected, self.positions.inspected());
        self.inspected = envelope(self.inspected, self.recovery.runs.measure().inspected);

        self.maximum_depth = self
            .boundaries
            .iter()
            .fold(self.edges.measure().maximum_depth, |depth, boundary| {
                depth.max(boundary.maximum_depth)
            });
    }

    pub(crate) fn reusable_with(
        &self,
        other: &Self,
        execution: Option<&Execution>,
        lexical: &mut HashSet<(usize, usize)>,
    ) -> bool {
        let mut pending = vec![(self, other)];

        while let Some((left, right)) = pending.pop() {
            if poll(execution).is_err() {
                return false;
            }

            if std::ptr::eq(left, right) {
                continue;
            }

            if left.kind != right.kind
                || left.width != right.width
                || left.count != right.count
                || left.maximum_depth != right.maximum_depth
                || left.edges.len() != right.edges.len()
                || left.recovery != right.recovery
                || left.boundaries.len() != right.boundaries.len()
                || left
                    .boundaries
                    .iter()
                    .zip(right.boundaries.iter())
                    .any(|(left, right)| !left.equivalent(right, lexical))
                || !left.positions.equivalent(&right.positions)
                || !left
                    .events
                    .equivalent(&right.events, &left.positions, &right.positions)
            {
                return false;
            }

            for (left, right) in left.edges.iter().zip(right.edges.iter()) {
                if poll(execution).is_err() {
                    return false;
                }

                match (left, right) {
                    (Syntax::Node(left), Syntax::Node(right)) => {
                        if !Arc::ptr_eq(left, right) {
                            pending.push((left, right));
                        }
                    }

                    (Syntax::Token(left), Syntax::Token(right)) => {
                        if !Arc::ptr_eq(left, right)
                            && (left.kind != right.kind
                                || !left.checkpoint.equivalent(&right.checkpoint, lexical)
                                || left.source.len() != right.source.len()
                                || !left
                                    .source
                                    .chunks()
                                    .flatten()
                                    .eq(right.source.chunks().flatten()))
                        {
                            return false;
                        }
                    }

                    _ => return false,
                }
            }
        }

        true
    }

    pub(crate) fn text(&self) -> &[u8] {
        if self.tokens.is_empty() {
            return &[];
        }

        self.text.get_or_init(|| {
            let mut bytes = Vec::with_capacity(self.width);

            for leaf in self.tokens.iter() {
                for chunk in leaf.source.chunks() {
                    bytes.extend_from_slice(chunk);
                }
            }

            Arc::from(bytes)
        })
    }
}

fn poll(execution: Option<&Execution>) -> Result<(), ParseError> {
    if let Some(execution) = execution
        && !execution.poll()
    {
        Err(execution.error().expect("interrupted freezing"))
    } else {
        Ok(())
    }
}

fn shifted(span: Span, edit: Span, replacement_width: usize) -> Span {
    let start = if span.start < edit.start {
        span.start
    } else if span.start >= edit.end {
        span.start - edit.len() + replacement_width
    } else {
        edit.start
    };

    let end = if span.end < edit.start {
        span.end
    } else if span.end >= edit.end {
        span.end - edit.len() + replacement_width
    } else {
        edit.start + replacement_width
    };

    Span { start, end }
}

impl Drop for Node {
    fn drop(&mut self) {
        fn retain(edges: Sequence<Syntax>, pending: &mut Vec<Arc<Node>>) {
            pending.extend(edges.into_unique().filter_map(|edge| match edge {
                Syntax::Node(node) => Some(node),
                Syntax::Token(_) => None,
            }));
        }

        let mut pending = Vec::new();
        self.events.clear();
        retain(std::mem::take(&mut self.edges), &mut pending);

        while let Some(node) = pending.pop() {
            if let Some(mut node) = Arc::into_inner(node) {
                node.events.clear();
                retain(std::mem::take(&mut node.edges), &mut pending);
            }
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Occurrence<'tree> {
    pub syntax: &'tree Arc<Node>,
    pub span: Span,
    pub ordinal: usize,
    pub parent: Option<usize>,
    pub position: usize,
    pub token_start: usize,
}

#[derive(Clone, Debug)]
struct Frame<'tree> {
    occurrence: Occurrence<'tree>,
    ordinal: usize,
    position: usize,
}

#[derive(Clone, Debug)]
pub(crate) struct Occurrences<'tree> {
    root: Option<Occurrence<'tree>>,
    frames: Vec<Frame<'tree>>,
    remaining: usize,
}

impl Occurrences<'_> {
    pub(crate) fn new(root: Occurrence<'_>) -> Occurrences<'_> {
        Occurrences {
            remaining: root.syntax.count,
            root: Some(root),
            frames: Vec::new(),
        }
    }
}

impl<'tree> Iterator for Occurrences<'tree> {
    type Item = Occurrence<'tree>;

    fn next(&mut self) -> Option<Self::Item> {
        if let Some(root) = self.root.take() {
            self.frames.push(Frame {
                occurrence: root,
                ordinal: root.ordinal,
                position: 0,
            });

            self.remaining -= 1;

            return Some(root);
        }

        loop {
            let frame = self.frames.last_mut()?;

            let Some((_, child, prefix)) = frame.occurrence.syntax.child(frame.position) else {
                self.frames.pop();
                continue;
            };

            let start = frame.occurrence.span.start + prefix.width;
            let ordinal = frame.ordinal + 1 + prefix.nodes;

            let occurrence = Occurrence {
                syntax: child,
                span: Span {
                    start,
                    end: start + child.width,
                },
                ordinal,
                parent: Some(frame.ordinal),
                position: frame.position,
                token_start: frame.occurrence.token_start + prefix.tokens,
            };

            frame.position += 1;

            self.frames.push(Frame {
                occurrence,
                ordinal,
                position: 0,
            });

            self.remaining -= 1;

            return Some(occurrence);
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.remaining, Some(self.remaining))
    }
}

impl std::iter::FusedIterator for Occurrences<'_> {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    pub span: Span,
    pub message: &'static str,
}

#[derive(Debug)]
pub struct Tree {
    pub(crate) source: Source,
    pub(crate) syntax: Arc<Node>,
    pub(crate) tokens: Sequence<Arc<Leaf>>,
    pub(crate) diagnostics_cache: OnceLock<Vec<Diagnostic>>,
}

impl Clone for Tree {
    fn clone(&self) -> Self {
        Self::from_syntax(self.source.clone(), Arc::clone(&self.syntax))
    }
}

impl Tree {
    #[must_use]
    pub fn source(&self) -> &[u8] {
        self.source.bytes()
    }

    pub fn source_chunks(&self) -> impl Iterator<Item = &[u8]> + Clone {
        self.source.chunks()
    }

    #[must_use]
    pub fn line_count(&self) -> usize {
        self.source.line_count()
    }

    /// # Errors
    ///
    /// Returns a coordinate error for an invalid offset or Unicode boundary.
    pub fn position(&self, offset: usize) -> Result<crate::Position, crate::CoordinateError> {
        self.source.position(offset)
    }

    /// # Errors
    ///
    /// Returns a coordinate error for an invalid position or Unicode boundary.
    pub fn offset(&self, position: crate::Position) -> Result<usize, crate::CoordinateError> {
        self.source.offset(position)
    }

    #[must_use]
    pub fn diagnostics(&self) -> &[Diagnostic] {
        self.diagnostics_cache
            .get_or_init(|| crate::diagnostics::materialize(&self.syntax))
    }

    pub(crate) fn syntax(&self) -> &Arc<Node> {
        &self.syntax
    }

    pub(crate) fn find(
        &self,
        span: Span,
        kind: Kind,
        visited: &mut HashMap<(Span, Kind), Arc<Node>>,
    ) -> Option<Arc<Node>> {
        if span.start > span.end || span.end > self.syntax.width {
            return None;
        }

        if let Some(node) = visited.get(&(span, kind)) {
            return Some(Arc::clone(node));
        }

        let mut pending = vec![(&self.syntax, 0)];

        while let Some((node, start)) = pending.pop() {
            let location = Span {
                start,
                end: start + node.width,
            };

            visited
                .entry((location, node.kind))
                .or_insert_with(|| Arc::clone(node));

            if node.kind == kind && span == location {
                return Some(Arc::clone(node));
            }

            let mut first = 0;
            let mut after = node.edges.measure().children;

            while first < after {
                let middle = first + (after - first) / 2;
                let (_, child, prefix) = node.child(middle).expect("child ordinal exists");
                let end = start + prefix.width + child.width;

                if end < span.start || (!span.is_empty() && end == span.start) {
                    first = middle + 1;
                } else {
                    after = middle;
                }
            }

            let mut candidates = Vec::new();

            while let Some((_, child, prefix)) = node.child(first) {
                let begin = start + prefix.width;

                if begin > span.start {
                    break;
                }

                if begin + child.width >= span.end {
                    candidates.push((child, begin));

                    if !span.is_empty() {
                        for position in [first.checked_sub(1), first.checked_add(1)]
                            .into_iter()
                            .flatten()
                        {
                            if let Some((_, neighbor, offset)) = node.child(position)
                                && neighbor.width != 0
                            {
                                let begin = start + offset.width;

                                let location = Span {
                                    start: begin,
                                    end: begin + neighbor.width,
                                };

                                visited
                                    .entry((location, neighbor.kind))
                                    .or_insert_with(|| Arc::clone(neighbor));
                            }
                        }
                    }
                }

                first += 1;
            }

            pending.extend(candidates.into_iter().rev());
        }

        None
    }

    pub(crate) fn occurrence(&self, index: usize) -> Occurrence<'_> {
        assert!(index < self.syntax.count);

        let mut occurrence = Occurrence {
            syntax: &self.syntax,
            span: Span {
                start: 0,
                end: self.syntax.width,
            },
            ordinal: 0,
            parent: None,
            position: 0,
            token_start: 0,
        };

        let mut ordinal = 0;

        while ordinal != index {
            let (_, edge, prefix) = occurrence
                .syntax
                .edges
                .select(index - ordinal - 1, |measure| measure.nodes)
                .expect("node ordinal exists");

            let Syntax::Node(node) = edge else {
                unreachable!()
            };

            let start = occurrence.span.start + prefix.width;

            occurrence = Occurrence {
                syntax: node,
                span: Span {
                    start,
                    end: start + node.width,
                },
                ordinal: ordinal + 1 + prefix.nodes,
                parent: Some(ordinal),
                position: prefix.children,
                token_start: occurrence.token_start + prefix.tokens,
            };

            ordinal += 1 + prefix.nodes;
        }

        occurrence
    }

    pub(crate) fn occurrences(&self) -> Occurrences<'_> {
        Occurrences::new(self.occurrence(0))
    }

    pub(crate) fn path(&self, index: usize) -> Vec<usize> {
        assert!(index < self.syntax.count);
        let mut path = Vec::new();
        let mut node = &self.syntax;
        let mut ordinal = 0;

        while ordinal != index {
            let (position, edge, prefix) = node
                .edges
                .select(index - ordinal - 1, |measure| measure.nodes)
                .expect("node ordinal exists");

            let Syntax::Node(child) = edge else {
                unreachable!()
            };

            path.push(position);
            node = child;
            ordinal += 1 + prefix.nodes;
        }

        path
    }

    pub(crate) fn replace_path_edit(
        &self,
        path: &[usize],
        replacement: Arc<Node>,
        edit: Span,
        replacement_width: usize,
    ) -> Option<Arc<Node>> {
        let mut ancestors = Vec::with_capacity(path.len());
        let mut node = &self.syntax;
        let mut start = 0;

        for position in path {
            ancestors.push((node, *position, start));
            start += node.edges.prefix(*position).width;

            let Some(Syntax::Node(child)) = node.edges.get(*position) else {
                panic!("path identifies a syntax node")
            };

            node = child;
        }

        let mut replacement = replacement;

        for (ancestor, position, start) in ancestors.into_iter().rev() {
            replacement = ancestor.splice_edit(
                position..position + 1,
                &Sequence::singleton(Syntax::Node(replacement)),
                Span {
                    start: edit.start - start,
                    end: edit.end - start,
                },
                replacement_width,
            )?;
        }

        Some(replacement)
    }

    pub(crate) fn token(&self, index: usize) -> Option<(Token, &Arc<Leaf>)> {
        let leaf = self.tokens.get(index)?;
        let start = self.tokens.prefix(index).width;

        Some((
            Token {
                kind: leaf.kind,
                span: Span {
                    start,
                    end: start + leaf.source.len(),
                },
            },
            leaf,
        ))
    }

    pub(crate) fn from_builder(builder: Builder<'_>, source: Source) -> Self {
        Self::freeze(builder, source, None, None).expect("unrestricted parse completed")
    }

    pub(crate) fn freeze(
        builder: Builder<'_>,
        source: Source,
        reuse: Option<&Reuse<'_>>,
        execution: Option<&Execution>,
    ) -> Result<Self, ParseError> {
        if let Some(error) = builder.error {
            return Err(error);
        }

        poll(execution)?;
        let tokens = leaves(&builder, &source, reuse, execution)?;
        assert_eq!(tokens.len(), builder.tokens.len());

        let mut diagnostics: Vec<Vec<(usize, Span, Arc<Owned>)>> =
            (0..builder.nodes.len()).map(|_| Vec::new()).collect();

        for (ordinal, (diagnostic, origin)) in
            builder.diagnostics.iter().zip(&builder.origins).enumerate()
        {
            poll(execution)?;
            let owner = origin.unwrap_or(builder.root);
            let start = builder.nodes[owner].span.start;

            diagnostics[owner].push((
                ordinal,
                Span {
                    start: diagnostic.span.start - start,
                    end: diagnostic.span.end - start,
                },
                Arc::new(Owned {
                    message: diagnostic.message,
                    length: diagnostic.span.len(),
                }),
            ));
        }

        assert_eq!(builder.diagnostics.len(), builder.origins.len());
        let nodes = builder.nodes;
        let mut syntax: Vec<Arc<Node>> = Vec::with_capacity(nodes.len());
        let mut first_diagnostics: Vec<Option<usize>> = Vec::with_capacity(nodes.len());

        for (index, node) in nodes.iter().enumerate() {
            poll(execution)?;

            let children = builder.children[node.children.clone()]
                .iter()
                .map(|index| (nodes[*index].span, Arc::clone(&syntax[*index])));

            let mut candidate = compose(node.kind, node.span, children, &tokens);
            let persistent = Arc::get_mut(&mut candidate).expect("new syntax is unique");

            let relative = |span: Span| Span {
                start: span.start - node.span.start,
                end: span.end - node.span.start,
            };

            persistent.boundaries = node
                .boundaries
                .iter()
                .cloned()
                .map(|boundary| Contract::freeze(boundary, node.span.start, node.span.len()))
                .collect();

            persistent.recovery = Recovery::new(
                node.span.len(),
                node.recovery.iter().cloned().map(|mut expectation| {
                    expectation.span = relative(expectation.span);

                    expectation
                }),
            );

            let direct = std::mem::take(&mut diagnostics[index]);

            persistent.positions = Positions::new(
                node.span.len(),
                direct
                    .iter()
                    .map(|(_, span, record)| (*span, Arc::clone(record))),
            );

            let first = direct
                .iter()
                .map(|(ordinal, _, _)| *ordinal)
                .chain(
                    builder.children[node.children.clone()]
                        .iter()
                        .filter_map(|child| first_diagnostics[*child]),
                )
                .min();

            persistent.events = Events::new(
                direct
                    .into_iter()
                    .map(|(ordinal, _, record)| (ordinal, record)),
                builder.children[node.children.clone()].iter().map(|child| {
                    (
                        Arc::clone(&syntax[*child]),
                        first_diagnostics[*child].unwrap_or(nodes[*child].diagnostic_end),
                    )
                }),
            );

            first_diagnostics.push(first);
            persistent.refresh();
            let shared = reuse.and_then(|reuse| reuse.node(node.span, &candidate));
            syntax.push(shared.unwrap_or(candidate));
        }

        poll(execution)?;

        Ok(Self::from_syntax(source, Arc::clone(&syntax[builder.root])))
    }

    pub(crate) fn from_syntax(source: Source, syntax: Arc<Node>) -> Self {
        Self {
            source,
            tokens: syntax.tokens.clone(),
            syntax,
            diagnostics_cache: OnceLock::new(),
        }
    }
}

fn leaves(
    builder: &Builder<'_>,
    source: &Source,
    reuse: Option<&Reuse<'_>>,
    execution: Option<&Execution>,
) -> Result<Vec<(Token, Arc<Leaf>)>, ParseError> {
    builder
        .tokens
        .iter()
        .zip(&builder.checkpoints)
        .map(|(token, checkpoint)| {
            poll(execution)?;
            let mut checkpoint = checkpoint.clone();
            checkpoint.cursor -= token.span.start;

            let leaf = reuse
                .and_then(|reuse| reuse.token(*token, &checkpoint))
                .unwrap_or_else(|| {
                    Arc::new(Leaf {
                        kind: token.kind,
                        source: source.slice(token.span),
                        checkpoint,
                    })
                });

            Ok((*token, leaf))
        })
        .collect()
}

pub(crate) fn compose(
    kind: Kind,
    span: Span,
    children: impl IntoIterator<Item = (Span, Arc<Node>)>,
    tokens: &[(Token, Arc<Leaf>)],
) -> Arc<Node> {
    let mut edges = Vec::new();
    let mut cursor = tokens.partition_point(|(token, _)| token.span.start < span.start);

    for (child, syntax) in children {
        while let Some((token, leaf)) = tokens.get(cursor)
            && token.span.start < child.start
        {
            edges.push(Syntax::Token(Arc::clone(leaf)));
            cursor += 1;
        }

        edges.push(Syntax::Node(syntax));

        cursor = cursor.max(tokens.partition_point(|(token, _)| {
            token.span.end <= child.end && token.kind != TokenKind::Eof
        }));
    }

    while let Some((token, leaf)) = tokens.get(cursor)
        && token.span.end <= span.end
        && (kind == Kind::Root || token.kind != TokenKind::Eof)
    {
        edges.push(Syntax::Token(Arc::clone(leaf)));
        cursor += 1;
    }

    let edges = edges.into_iter().collect();
    let tokens = Node::tokens(&edges);
    let node = Node::assembled(kind, edges, tokens);
    assert_eq!(node.width, span.len());

    node
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::State;

    impl Node {
        fn new(kind: Kind, edges: Sequence<Syntax>) -> Arc<Self> {
            let tokens = Self::tokens(&edges);
            let mut node = Self::assembled(kind, edges, tokens);
            let syntax = Arc::get_mut(&mut node).expect("new syntax is unique");
            syntax.recovery = Recovery::new(syntax.width, std::iter::empty());
            syntax.positions = Positions::new(syntax.width, std::iter::empty());

            syntax.events = Events::new(
                std::iter::empty(),
                syntax.edges.iter().filter_map(|edge| match edge {
                    Syntax::Node(child) => Some((Arc::clone(child), 0)),
                    Syntax::Token(_) => None,
                }),
            );

            node
        }
    }

    fn leaf(kind: TokenKind, source: &[u8]) -> Arc<Leaf> {
        Arc::new(Leaf {
            kind,
            source: Source::from(source),
            checkpoint: Checkpoint {
                cursor: 0,
                state: State::default(),
                finished: false,
            },
        })
    }

    #[test]
    fn scoped_text_keeps_snapshot_source_lazy() {
        let mut source = b"return [[".to_vec();
        source.extend(std::iter::repeat_n(b'x', 12000));
        source.extend_from_slice(b"]]\n");
        let tree = crate::parse(&source);
        assert!(!tree.source.materialized());
        assert!(tree.syntax.text.get().is_none());

        let string = tree
            .root()
            .descendants()
            .find(|view| view.kind() == Kind::String)
            .unwrap();

        assert_eq!(string.text(), string.span().bytes(&source));
        let token = string.tokens().next().unwrap();
        assert_eq!(token.text(), string.text());
        assert_eq!(tree.token_at(token.span().start), Some(token));
        assert_eq!(tree.node_at(token.span().start), Some(string));
        assert!(!tree.source.materialized());
        assert!(tree.syntax.text.get().is_none());
        assert_eq!(tree.root().text(), source);
        assert!(!tree.source.materialized());
        assert_eq!(tree.source(), source);
        assert!(tree.source.materialized());
    }

    #[test]
    fn large_child_splices_share_sequences() {
        let child = Node::new(
            Kind::Name,
            Sequence::singleton(Syntax::Token(leaf(TokenKind::Name, b"x"))),
        );

        let edges = (0..4096)
            .map(|_| Syntax::Node(Arc::clone(&child)))
            .collect();

        let original = Node::new(Kind::Block, edges);

        let replacement = Node::new(
            Kind::Name,
            Sequence::singleton(Syntax::Token(leaf(TokenKind::Name, b"longer"))),
        );

        let edited = original
            .splice_edit(
                2048..2049,
                &Sequence::singleton(Syntax::Node(replacement)),
                Span {
                    start: 2048,
                    end: 2049,
                },
                6,
            )
            .unwrap();

        assert!(std::ptr::eq(
            original.edges.get(4000).unwrap(),
            edited.edges.get(4000).unwrap()
        ));

        assert!(std::ptr::eq(
            original.tokens.get(4000).unwrap(),
            edited.tokens.get(4000).unwrap()
        ));

        assert_eq!(original.width, 4096);
        assert_eq!(edited.width, 4101);
        assert_eq!(edited.count, original.count);
        assert_eq!(edited.edges.prefix(2049).width, 2054);
        assert!(original.text.get().is_none() && edited.text.get().is_none());
    }

    #[test]
    fn shared_missing_nodes_keep_occurrence_identity() {
        let missing = Node::new(Kind::Missing, Sequence::default());

        let block = Node::new(
            Kind::Block,
            [
                Syntax::Node(Arc::clone(&missing)),
                Syntax::Node(Arc::clone(&missing)),
            ]
            .into_iter()
            .collect(),
        );

        let root = Node::new(
            Kind::Root,
            [
                Syntax::Node(block),
                Syntax::Token(leaf(TokenKind::Eof, b"")),
            ]
            .into_iter()
            .collect(),
        );

        let tree = Tree::from_syntax(Source::from(b"".as_slice()), root);
        let block = tree.root().children().next().unwrap();
        let first = block.children().next().unwrap();
        let second = block.children().next_back().unwrap();
        assert_ne!(first, second);
        assert_eq!(first.span(), second.span());
        assert_eq!(first.parent(), Some(block));
        assert_eq!(first.next_sibling(), Some(second));
        assert_eq!(second.previous_sibling(), Some(first));
        assert!(first.tokens().next().is_none() && second.tokens().next().is_none());

        assert_eq!(
            tree.root().descendants().collect::<Vec<_>>(),
            [tree.root(), block, first, second]
        );

        assert!(Arc::ptr_eq(
            &tree
                .find(first.span(), Kind::Missing, &mut HashMap::new())
                .unwrap(),
            &missing
        ));

        assert_eq!(tree.occurrences().count(), 4);
        assert_eq!(tree.path(second.index), [0, 1]);
        let replacement = Node::new(Kind::Error, Sequence::default());

        let replaced = tree
            .replace_path_edit(&tree.path(second.index), replacement, first.span(), 0)
            .unwrap();

        let changed = Tree::from_syntax(tree.source.clone(), replaced);

        assert_eq!(
            changed
                .root()
                .children()
                .next()
                .unwrap()
                .children()
                .next_back()
                .unwrap()
                .kind(),
            Kind::Error
        );

        assert_eq!(first.kind(), Kind::Missing);
        assert_eq!(tree.tokens().next().unwrap().kind(), TokenKind::Eof);
    }

    #[test]
    fn metadata_and_inspection_indexes_follow_actual_edits() {
        use crate::parser::context::{Context, Expected, Rule};
        let edges = Sequence::singleton(Syntax::Token(leaf(TokenKind::Name, b"abcdef")));
        let mut original = Node::new(Kind::Root, edges);
        let syntax = Arc::get_mut(&mut original).unwrap();

        syntax.boundaries = Box::from([Contract::freeze(
            Boundary {
                context: Context {
                    rule: Rule::Expression(0),
                    depth: 0,
                    previous: None,
                    lexical: State::default(),
                },
                consumed: Span { start: 0, end: 6 },
                inspected: vec![Span { start: 4, end: 5 }, Span { start: 9, end: 9 }],
                exit: State::default(),
                current: Token {
                    kind: TokenKind::Name,
                    span: Span { start: 6, end: 7 },
                },
                maximum_depth: 0,
            },
            0,
            6,
        )]);

        syntax.recovery = Recovery::new(
            6,
            [Expectation {
                span: Span { start: 5, end: 5 },
                expected: Expected::Token(TokenKind::Byte(b')')),
            }],
        );

        let record = Arc::new(Owned {
            message: "expected syntax",
            length: 1,
        });

        syntax.positions = Positions::new(6, [(Span { start: 4, end: 5 }, Arc::clone(&record))]);
        syntax.events = Events::new([(0, record)], std::iter::empty());
        syntax.refresh();

        let edited = original
            .splice_edit(
                0..1,
                &Sequence::singleton(Syntax::Token(leaf(TokenKind::Name, b"abXXcdef"))),
                Span { start: 2, end: 2 },
                2,
            )
            .unwrap();

        assert_eq!(edited.boundaries[0].consumed, Span { start: 0, end: 8 });

        assert_eq!(
            edited.boundaries[0].inspected.spans().collect::<Vec<_>>(),
            [Span { start: 6, end: 7 }, Span { start: 11, end: 11 }]
        );

        assert_eq!(edited.boundaries[0].current.span, Span { start: 8, end: 9 });

        assert_eq!(
            edited.recovery.iter().next().unwrap().span,
            Span { start: 7, end: 7 }
        );

        assert_eq!(
            crate::diagnostics::materialize(&edited)[0].span,
            Span { start: 6, end: 7 }
        );

        let children = [
            Syntax::Token(leaf(TokenKind::Whitespace, b"   ")),
            Syntax::Node(Arc::clone(&edited)),
        ];

        let parent = Node::new(Kind::Block, children.into_iter().collect());
        assert_eq!(parent.inspected, Some(Span { start: 9, end: 14 }));

        let matches: Vec<_> = parent
            .edges
            .intersecting(Span { start: 14, end: 14 })
            .collect();

        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].0, 1);
        assert_eq!(matches[0].2.width, 3);

        assert!(
            parent
                .edges
                .intersecting(Span { start: 15, end: 15 })
                .next()
                .is_none()
        );

        let tree = Tree::from_syntax(Source::from(b"   abXXcdef".as_slice()), parent);
        assert!(tree.diagnostics_cache.get().is_none());

        assert_eq!(
            tree.diagnostics(),
            [Diagnostic {
                span: Span { start: 9, end: 10 },
                message: "expected syntax"
            }]
        );

        let clone = tree.clone();
        assert!(clone.diagnostics_cache.get().is_none());
        assert_eq!(clone.diagnostics(), tree.diagnostics());

        assert_eq!(
            original.boundaries[0].inspected.spans().nth(1),
            Some(Span { start: 9, end: 9 })
        );
    }

    #[test]
    fn inspection_gaps_and_eof_points_share_unchanged_paths() {
        let width = 4096 * 4;

        let inspected = Inspection::new(
            width,
            (0..4096)
                .map(|index| Span {
                    start: index * 4,
                    end: index * 4 + 1,
                })
                .chain([Span {
                    start: width,
                    end: width,
                }]),
        );

        let edited = inspected.splice(Span { start: 2, end: 3 }, 4).unwrap();

        assert_eq!(
            edited.spans().last(),
            Some(Span {
                start: width + 3,
                end: width + 3
            })
        );

        assert!(std::ptr::eq(
            inspected.runs.get(inspected.runs.len() - 1).unwrap(),
            edited.runs.get(edited.runs.len() - 1).unwrap(),
        ));

        assert!(inspected.intersects(Span {
            start: width,
            end: width
        }));

        assert!(!inspected.intersects_nonempty(Span {
            start: width,
            end: width
        }));

        let appended = inspected
            .splice(
                Span {
                    start: width,
                    end: width,
                },
                5,
            )
            .unwrap();

        assert_eq!(
            appended.spans().last(),
            Some(Span {
                start: width + 5,
                end: width + 5
            })
        );

        assert!(std::ptr::eq(
            inspected.runs.get(inspected.runs.len() - 1).unwrap(),
            appended.runs.get(appended.runs.len() - 1).unwrap(),
        ));

        assert!(inspected.splice(Span { start: 0, end: 1 }, 2).is_none());

        assert_eq!(
            inspected.spans().last(),
            Some(Span {
                start: width,
                end: width
            })
        );

        let adjacent = Inspection::new(1, [Span { start: 0, end: 1 }, Span { start: 1, end: 1 }]);
        assert!(!adjacent.intersects_nonempty(Span { start: 1, end: 1 }));
        let adjacent = adjacent.splice(Span { start: 1, end: 1 }, 2).unwrap();

        assert_eq!(
            adjacent.spans().collect::<Vec<_>>(),
            [Span { start: 0, end: 1 }, Span { start: 3, end: 3 }]
        );
    }

    #[test]
    fn dependency_queries_reach_preceding_siblings() {
        use crate::parser::context::Rule;

        let mut first = Node::new(
            Kind::Name,
            Sequence::singleton(Syntax::Token(leaf(TokenKind::Name, b"a"))),
        );

        let syntax = Arc::get_mut(&mut first).unwrap();

        syntax.boundaries = Box::from([Contract::freeze(
            Boundary {
                context: Context {
                    rule: Rule::Expression(0),
                    depth: 0,
                    previous: None,
                    lexical: State::default(),
                },
                consumed: Span { start: 0, end: 1 },
                inspected: vec![Span { start: 4, end: 5 }],
                exit: State::default(),
                current: Token {
                    kind: TokenKind::Name,
                    span: Span { start: 4, end: 5 },
                },
                maximum_depth: 0,
            },
            0,
            1,
        )]);

        syntax.refresh();

        let second = Node::new(
            Kind::Name,
            Sequence::singleton(Syntax::Token(leaf(TokenKind::Name, b"bcdef"))),
        );

        let parent = Node::new(
            Kind::Block,
            [Syntax::Node(Arc::clone(&first)), Syntax::Node(second)]
                .into_iter()
                .collect(),
        );

        let dependencies: Vec<_> = parent
            .edges
            .intersecting(Span { start: 4, end: 5 })
            .collect();

        assert_eq!(dependencies.len(), 1);
        assert_eq!(dependencies[0].0, 0);
        assert_eq!(dependencies[0].2.width, 0);

        assert!(
            first.boundaries[0]
                .inspected
                .intersects_nonempty(Span { start: 4, end: 5 })
        );

        assert!(
            parent
                .edges
                .intersecting(Span { start: 2, end: 3 })
                .next()
                .is_none()
        );
    }

    #[test]
    fn removed_child_depth_does_not_survive_splicing() {
        use crate::lexer::{Brace, Braces};
        let mut token = leaf(TokenKind::Name, b"x");

        Arc::get_mut(&mut token).unwrap().checkpoint.state.braces =
            Braces::from([Brace::Normal; 8]);

        let deep = Node::new(Kind::Name, Sequence::singleton(Syntax::Token(token)));

        let shallow = Node::new(
            Kind::Name,
            Sequence::singleton(Syntax::Token(leaf(TokenKind::Name, b"x"))),
        );

        let original = Node::new(
            Kind::Block,
            [
                Syntax::Node(Arc::clone(&shallow)),
                Syntax::Node(Arc::clone(&deep)),
            ]
            .into_iter()
            .collect(),
        );

        assert_eq!(original.maximum_depth, 8);
        assert!(!deep.reusable_with(&shallow, None, &mut HashSet::new()));

        let edited = original
            .splice_edit(
                1..2,
                &Sequence::singleton(Syntax::Node(shallow)),
                Span { start: 1, end: 2 },
                1,
            )
            .unwrap();

        assert_eq!(edited.maximum_depth, 0);
        assert_eq!(original.maximum_depth, 8);
    }

    #[test]
    fn reuse_memo_visits_only_paths_and_immediate_neighbors() {
        let child = Node::new(
            Kind::Name,
            Sequence::singleton(Syntax::Token(leaf(TokenKind::Name, b"x"))),
        );

        let root = Node::new(
            Kind::Block,
            (0..4096)
                .map(|_| Syntax::Node(Arc::clone(&child)))
                .collect(),
        );

        let source = vec![b'x'; 4096];
        let tree = Tree::from_syntax(Source::from(source.as_slice()), root);
        let mut visited = HashMap::new();

        let found = tree
            .find(
                Span {
                    start: 2048,
                    end: 2049,
                },
                Kind::Name,
                &mut visited,
            )
            .unwrap();

        assert!(Arc::ptr_eq(&found, &child));

        assert!(visited.contains_key(&(
            Span {
                start: 2048,
                end: 2049
            },
            Kind::Name
        )));

        assert!(visited.keys().all(|(span, _)| {
            (span.start <= 2048 && span.end >= 2049) || span.end == 2048 || span.start == 2049
        }));

        let mut source = b"return value".to_vec();

        for _ in 0..256 {
            source.extend_from_slice(b"()");
        }

        let tree = crate::parse(&source);
        let mut visited = HashMap::new();

        let name = tree
            .find(Span { start: 7, end: 12 }, Kind::Name, &mut visited)
            .unwrap();

        assert_eq!(name.text(), b"value");
        let arguments = Span { start: 12, end: 14 };
        assert!(visited.contains_key(&(arguments, Kind::Arguments)));
        let count = visited.len();
        let found = tree.find(arguments, Kind::Arguments, &mut visited).unwrap();
        assert_eq!(found.text(), b"()");
        assert_eq!(visited.len(), count);
    }

    #[test]
    fn cached_lexical_equality_preserves_token_comparisons() {
        use crate::lexer::{Brace, Braces};
        use crate::parser::context::Rule;

        fn contextual(source: &[u8], state: State) -> Arc<Node> {
            let depth = state.braces.len();

            let token = Arc::new(Leaf {
                kind: TokenKind::Name,
                source: Source::from(source),
                checkpoint: Checkpoint {
                    cursor: 0,
                    state: state.clone(),
                    finished: false,
                },
            });

            let mut node = Node::new(Kind::Name, Sequence::singleton(Syntax::Token(token)));
            let syntax = Arc::get_mut(&mut node).unwrap();

            syntax.boundaries = Box::from([Contract::freeze(
                Boundary {
                    context: Context {
                        rule: Rule::Expression(0),
                        depth: 0,
                        previous: None,
                        lexical: state.clone(),
                    },
                    consumed: Span { start: 0, end: 1 },
                    inspected: vec![Span { start: 0, end: 1 }],
                    exit: state,
                    current: Token {
                        kind: TokenKind::Eof,
                        span: Span { start: 1, end: 1 },
                    },
                    maximum_depth: depth,
                },
                0,
                1,
            )]);

            syntax.refresh();

            node
        }

        let first = State {
            braces: Braces::from([Brace::Normal; 32]),
        };

        let second = State {
            braces: Braces::from([Brace::Normal; 32]),
        };

        let left = contextual(b"x", first);
        let right = contextual(b"x", second.clone());
        let different = contextual(b"y", second);
        let mut lexical = HashSet::new();
        assert!(left.reusable_with(&right, None, &mut lexical));
        assert!(!lexical.is_empty());
        let cached = lexical.clone();
        assert!(left.reusable_with(&right, None, &mut lexical));
        assert_eq!(lexical, cached);
        assert!(!left.reusable_with(&different, None, &mut lexical));
    }

    #[test]
    fn deep_syntax_traversal_and_destruction() {
        let mut syntax = Node::new(
            Kind::Name,
            Sequence::singleton(Syntax::Token(leaf(TokenKind::Name, b"x"))),
        );

        for _ in 0..20000 {
            syntax = Node::new(Kind::Group, Sequence::singleton(Syntax::Node(syntax)));
        }

        let tree = Tree::from_syntax(Source::from(b"x".as_slice()), syntax);
        assert_eq!(tree.root().descendants().count(), 20001);
        assert_eq!(tree.occurrences().count(), 20001);
        assert_eq!(tree.root().descendants().last().unwrap().text(), b"x");
        assert_eq!(tree.node_at(0).unwrap().kind(), Kind::Name);
        assert_eq!(tree.root().text(), b"x");
        drop(tree);
    }

    #[test]
    fn cancellation_prevents_freezing_staged_syntax() {
        use crate::parser::control::Control;
        use std::sync::atomic::{AtomicBool, Ordering};
        let cancellation = Arc::new(AtomicBool::new(false));

        let execution = Execution::new(&Control {
            cancellation: Some(Arc::clone(&cancellation)),
            ..Control::default()
        });

        let source = b"local value = 1";
        let builder = crate::parser::controlled_in(source, &execution).unwrap();
        cancellation.store(true, Ordering::Relaxed);

        assert_eq!(
            Tree::freeze(
                builder,
                Source::from(source.as_slice()),
                None,
                Some(&execution)
            )
            .unwrap_err(),
            ParseError::Cancelled
        );
    }

    #[test]
    fn frozen_diagnostics_preserve_parser_emission_order() {
        use crate::parser::control::Control;

        for source in [
            b"if const value = f//nd".as_slice(),
            b"if x local a = end",
            b"local function f(value: ) local x =",
            b"class C function f(value: ) local x = end",
        ] {
            let execution = Execution::new(&Control::default());
            let builder = crate::parser::controlled_in(source, &execution).unwrap();
            let expected = builder.diagnostics.clone();

            let tree = Tree::freeze(builder, Source::from(source), None, Some(&execution)).unwrap();

            assert_eq!(tree.diagnostics(), expected, "{source:?}");
        }
    }
}
