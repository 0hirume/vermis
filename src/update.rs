use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    error::Error,
    fmt,
    sync::Arc,
};

use crate::lexer::Checkpoint;

use crate::parser::{
    self,
    control::{Control, Execution, ParseError, Resource},
};

use crate::source::Source;
use crate::tree::{Node, Syntax};
use crate::{Kind, Span, Token, Tree};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Edit {
    pub range: Span,
    pub replacement: Vec<u8>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditError {
    InvalidRange,
    OverlappingEdits,
    Parse(ParseError),
}

impl fmt::Display for EditError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidRange => {
                formatter.write_str("edit range is reversed or outside the source")
            }

            Self::OverlappingEdits => {
                formatter.write_str("edit ranges overlap or share an insertion position")
            }

            Self::Parse(error) => error.fmt(formatter),
        }
    }
}

impl Error for EditError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Parse(error) => Some(error),
            _ => None,
        }
    }
}

impl From<ParseError> for EditError {
    fn from(error: ParseError) -> Self {
        Self::Parse(error)
    }
}

pub(crate) struct Reuse<'tree> {
    tree: &'tree Tree,
    offset: usize,
    range: Span,
    replacement: usize,
    execution: &'tree Execution,
    nodes: RefCell<HashMap<(Span, Kind), Arc<Node>>>,
    lexical: RefCell<HashSet<(usize, usize)>>,
}

impl<'tree> Reuse<'tree> {
    fn new(
        tree: &'tree Tree,
        offset: usize,
        range: Span,
        replacement: usize,
        execution: &'tree Execution,
    ) -> Self {
        Self {
            tree,
            offset,
            range,
            replacement,
            execution,
            nodes: RefCell::new(HashMap::new()),
            lexical: RefCell::new(HashSet::new()),
        }
    }

    fn original(&self, span: Span) -> Option<Span> {
        let span = translated(span, self.offset);

        if span.end <= self.range.start {
            Some(span)
        } else if span.start >= self.range.start + self.replacement {
            Some(Span {
                start: span.start - self.replacement + self.range.len(),
                end: span.end - self.replacement + self.range.len(),
            })
        } else {
            None
        }
    }

    pub(crate) fn node(&self, span: Span, syntax: &Node) -> Option<Arc<Node>> {
        let original = self.tree.find(
            self.original(span)?,
            syntax.kind,
            &mut self.nodes.borrow_mut(),
        )?;

        original
            .reusable_with(syntax, Some(self.execution), &mut self.lexical.borrow_mut())
            .then_some(original)
    }

    pub(crate) fn token(
        &self,
        token: Token,
        checkpoint: &Checkpoint,
    ) -> Option<Arc<crate::tree::Leaf>> {
        let span = self.original(token.span)?;

        let (_, _, prefix) = if span.start < self.tree.source.len() {
            self.tree
                .tokens
                .select(span.start, |measure| measure.width)?
        } else {
            let index = self.tree.tokens.len().checked_sub(1)?;

            (
                index,
                self.tree.tokens.get(index)?,
                self.tree.tokens.prefix(index),
            )
        };

        let index = prefix.tokens;
        let (original, leaf) = self.tree.token(index)?;

        (original.span == span
            && original.kind == token.kind
            && leaf
                .checkpoint
                .equivalent(checkpoint, &mut self.lexical.borrow_mut()))
        .then(|| Arc::clone(leaf))
    }
}

fn translated(span: Span, offset: usize) -> Span {
    Span {
        start: span.start + offset,
        end: span.end + offset,
    }
}

fn common_path(path: &mut Vec<usize>, other: &[usize]) {
    let length = path
        .iter()
        .zip(other)
        .take_while(|(first, second)| first == second)
        .count();

    path.truncate(length);
}

struct Candidate<'tree> {
    node: &'tree Arc<Node>,
    start: usize,
}

impl Tree {
    /// # Errors
    ///
    /// Returns an error for an invalid range or interrupted parsing.
    pub fn update(&self, range: Span, replacement: &[u8]) -> Result<Self, EditError> {
        self.update_with(range, replacement, &Control::default())
    }

    /// # Errors
    ///
    /// Returns an error for an invalid range, cancellation or exceeded resource limits.
    pub fn update_with(
        &self,
        range: Span,
        replacement: &[u8],
        control: &Control,
    ) -> Result<Self, EditError> {
        self.update_in(range, replacement, control, &Execution::new(control))
    }

    fn update_in(
        &self,
        range: Span,
        replacement: &[u8],
        control: &Control,
        execution: &Arc<Execution>,
    ) -> Result<Self, EditError> {
        self.validate_range(range)?;

        if !execution.source(self.source.len() - range.len(), replacement.len()) {
            return Err(execution.error().expect("interrupted edit").into());
        }

        if self
            .source
            .slice(range)
            .chunks()
            .flatten()
            .copied()
            .eq(replacement.iter().copied())
        {
            self.validate_limits(control, execution)?;

            return Ok(self.clone());
        }

        let source = self.source.replace(range, replacement);
        let mut path = self.invalidated_path(range, execution)?;

        loop {
            let candidate = self.candidate(&path);

            if let Some(boundary) = candidate.node.boundaries.last()
                && let Some(syntax) = self.replay(
                    &source,
                    range,
                    replacement.len(),
                    &candidate,
                    boundary,
                    execution,
                )?
                && let Some(root) = self.replace_path_edit(&path, syntax, range, replacement.len())
            {
                let tree = Self::from_syntax(source, root, self.markup);
                tree.validate_limits(control, execution)?;

                return Ok(tree);
            }

            if path.pop().is_none() {
                let tree = self.reparse_root(&source, range, replacement.len(), execution)?;
                tree.validate_limits(control, execution)?;

                return Ok(tree);
            }
        }
    }

    /// # Errors
    ///
    /// Rejects invalid or overlapping original-snapshot ranges before applying any edit.
    pub fn update_many(&self, edits: &[Edit]) -> Result<Self, EditError> {
        self.update_many_with(edits, &Control::default())
    }

    /// # Errors
    ///
    /// Rejects invalid or overlapping ranges, cancellation and exceeded resource limits.
    pub fn update_many_with(&self, edits: &[Edit], control: &Control) -> Result<Self, EditError> {
        let mut ordered: Vec<_> = edits.iter().collect();

        for edit in &ordered {
            self.validate_range(edit.range)?;
        }

        ordered.sort_by_key(|edit| (edit.range.start, edit.range.end));

        if ordered.windows(2).any(|pair| {
            pair[0].range.end > pair[1].range.start || pair[0].range.start == pair[1].range.start
        }) {
            return Err(EditError::OverlappingEdits);
        }

        let execution = Execution::new(control);
        let mut tree = self.clone();

        for edit in ordered.into_iter().rev() {
            tree = tree.update_in(edit.range, &edit.replacement, control, &execution)?;
        }

        tree.validate_limits(control, &execution)?;

        Ok(tree)
    }

    fn validate_range(&self, range: Span) -> Result<(), EditError> {
        if range.start > range.end || range.end > self.source.len() {
            Err(EditError::InvalidRange)
        } else {
            Ok(())
        }
    }

    fn validate_limits(&self, control: &Control, execution: &Execution) -> Result<(), EditError> {
        if !execution.poll() {
            return Err(execution.error().expect("interrupted edit").into());
        }

        for (limit, count, resource) in [
            (
                control.limits.source_bytes,
                self.source.len(),
                Resource::SourceBytes,
            ),
            (control.limits.tokens, self.tokens.len(), Resource::Tokens),
            (control.limits.nodes, self.syntax.count, Resource::Nodes),
            (
                control.limits.diagnostics,
                self.syntax.events.len(),
                Resource::Diagnostics,
            ),
            (
                control.limits.depth,
                self.syntax.maximum_depth,
                Resource::Depth,
            ),
        ] {
            if limit.is_some_and(|limit| count > limit) {
                return Err(ParseError::Limit(resource).into());
            }
        }

        Ok(())
    }

    fn candidate(&self, path: &[usize]) -> Candidate<'_> {
        let mut node = self.syntax();
        let mut start = 0;

        for position in path {
            start += node.edges.prefix(*position).width;

            let Syntax::Node(child) = node.edges.get(*position).expect("syntax path exists") else {
                unreachable!("syntax path selects nodes")
            };

            node = child;
        }

        Candidate { node, start }
    }

    fn invalidated_path(
        &self,
        range: Span,
        execution: &Execution,
    ) -> Result<Vec<usize>, EditError> {
        let covering = if range.is_empty() && range.start == self.source.len() && range.start > 0 {
            self.node_at(range.start - 1)
                .expect("last source byte exists")
        } else {
            self.covering(range).expect("validated source range")
        };

        let mut path = self.path(covering.index);

        while !path.is_empty() {
            let candidate = self.candidate(&path);

            if candidate.start <= range.start
                && range.end <= candidate.start + candidate.node.width
                && !candidate.node.boundaries.is_empty()
            {
                break;
            }

            path.pop();
        }

        let mut pending = vec![(self.syntax(), 0usize, Vec::new())];

        while let Some((node, start, current_path)) = pending.pop() {
            if !execution.poll() {
                return Err(execution.error().expect("interrupted invalidation").into());
            }

            if current_path.starts_with(&path) {
                continue;
            }

            if range.end < start {
                continue;
            }

            let local = Span {
                start: range.start.saturating_sub(start),
                end: range.end - start,
            };

            if node
                .boundaries
                .iter()
                .any(|boundary| boundary.inspected.intersects_nonempty(local))
            {
                common_path(&mut path, &current_path);

                if current_path.starts_with(&path) {
                    continue;
                }
            }

            for (position, syntax, prefix) in node.edges.intersecting(local) {
                if let Syntax::Node(child) = syntax {
                    let mut child_path = current_path.clone();
                    child_path.push(position);
                    pending.push((child, start + prefix.width, child_path));
                }
            }
        }

        Ok(path)
    }

    fn replay(
        &self,
        source: &Source,
        range: Span,
        replacement: usize,
        candidate: &Candidate<'_>,
        boundary: &crate::tree::Contract,
        execution: &Arc<Execution>,
    ) -> Result<Option<Arc<Node>>, EditError> {
        if candidate.start > range.start || range.end > candidate.start + candidate.node.width {
            return Ok(None);
        }

        let old_end = candidate.start + candidate.node.width;
        let expected_end = old_end - range.len() + replacement;
        let current = translated(boundary.current.span, candidate.start);

        let inspected = candidate
            .node
            .inspected
            .map_or(old_end, |span| candidate.start + span.end);

        let limit = inspected
            .max(current.end)
            .max(old_end)
            .min(self.source.len());

        let mut finish = (limit - range.len() + replacement)
            .saturating_add(1)
            .min(source.len());

        if !self.valid_entry(source, candidate.start, finish, execution)? {
            return Ok(None);
        }

        loop {
            let slice = source.slice(Span {
                start: candidate.start,
                end: finish,
            });

            let builder = parser::controlled_unit_in(
                slice.bytes(),
                self.markup,
                &boundary.context,
                execution,
            )?;

            let reuse = Reuse::new(self, candidate.start, range, replacement, execution);

            let temporary = Self::freeze(
                builder,
                slice.clone(),
                self.markup,
                Some(&reuse),
                Some(execution),
            )?;

            let new = temporary.syntax();

            if finish < source.len()
                && new
                    .inspected
                    .is_some_and(|span| span.end >= finish - candidate.start)
            {
                finish = candidate
                    .start
                    .saturating_add((finish - candidate.start).saturating_mul(2))
                    .min(source.len());

                continue;
            }

            let Some(exit) = new.boundaries.last() else {
                return Ok(None);
            };

            let expected_current = Span {
                start: current.start - range.len() + replacement,
                end: current.end - range.len() + replacement,
            };

            if new.kind != candidate.node.kind
                || candidate.start + new.width != expected_end
                || exit.context.rule != boundary.context.rule
                || exit.exit != boundary.exit
                || exit.current.kind != boundary.current.kind
                || translated(exit.current.span, candidate.start) != expected_current
            {
                return Ok(None);
            }

            return Ok(Some(Arc::clone(new)));
        }
    }

    fn valid_entry(
        &self,
        source: &Source,
        start: usize,
        finish: usize,
        execution: &Arc<Execution>,
    ) -> Result<bool, EditError> {
        if start == 0 {
            return Ok(true);
        }

        let (index, _, _) = self
            .tokens
            .select(start - 1, |measure| measure.width)
            .expect("preceding source byte exists");

        let (previous, leaf) = self.token(index).expect("preceding token exists");

        if !matches!(
            leaf.checkpoint.state.mode,
            crate::lexer::Mode::Code | crate::lexer::Mode::MarkupHole
        ) {
            return Ok(true);
        }

        let input = source.slice(Span {
            start: previous.span.start,
            end: finish,
        });

        let mut lexer = crate::Lexer::controlled(
            input.bytes(),
            leaf.checkpoint.cursor,
            &leaf.checkpoint.state,
            Some(Arc::clone(execution)),
        );

        let token = lexer.next();

        if let Some(error) = execution.error() {
            return Err(error.into());
        }

        if !execution.token() {
            return Err(execution.error().expect("interrupted lexical entry").into());
        }

        Ok(token.is_some_and(|token| {
            token.kind == previous.kind
                && translated(token.span, previous.span.start) == previous.span
        }))
    }

    fn reparse_root(
        &self,
        source: &Source,
        range: Span,
        replacement: usize,
        execution: &Arc<Execution>,
    ) -> Result<Self, EditError> {
        let builder = parser::controlled_in(source.bytes(), self.markup, execution)?;
        let reuse = Reuse::new(self, 0, range, replacement, execution);

        Self::freeze(
            builder,
            source.clone(),
            self.markup,
            Some(&reuse),
            Some(execution),
        )
        .map_err(Into::into)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Parts, parse};

    #[test]
    fn nested_updates_share_storage_without_projecting_or_flattening() {
        let source = b"local before = 1\nlocal function run(value)\nlocal data = {left = 3, right = 4}\nreturn data\nend\nlocal tail = 5";
        let original = parse(source);
        let position = source.iter().position(|byte| *byte == b'3').unwrap();

        let updated = original
            .update(
                Span {
                    start: position,
                    end: position + 1,
                },
                b"300",
            )
            .unwrap();

        assert!(!updated.source.materialized());
        assert!(updated.diagnostics_cache.get().is_none());
        let old: Vec<_> = original.root().descendants().collect();
        let new: Vec<_> = updated.root().descendants().collect();

        let changed = new
            .iter()
            .find(|node| node.kind() == Kind::Number && node.span().start == position)
            .unwrap();

        assert_eq!(changed.text(), b"300");
        assert_eq!(changed.span().end, position + 3);

        for text in [
            b"local before = 1".as_slice(),
            b"local tail = 5",
            b"right = 4",
        ] {
            let left = old.iter().find(|node| node.text() == text).unwrap();
            let right = new.iter().find(|node| node.text() == text).unwrap();
            assert!(Arc::ptr_eq(left.node().syntax, right.node().syntax));
        }

        assert!(matches!(updated.root().parts(), Some(Parts::Root { .. })));
        assert_eq!(original.source(), source);
        let expected = b"local before = 1\nlocal function run(value)\nlocal data = {left = 300, right = 4}\nreturn data\nend\nlocal tail = 5";

        assert_eq!(
            updated
                .source_chunks()
                .flatten()
                .copied()
                .collect::<Vec<_>>(),
            expected
        );

        assert!(!updated.source.materialized());
        assert_eq!(updated.source(), expected);
    }

    #[test]
    fn batches_preserve_original_coordinates_and_reject_overlaps() {
        let tree = parse(b"local first = 1\nlocal second = 2");
        let first = tree.source().iter().position(|byte| *byte == b'1').unwrap();
        let second = tree.source().iter().position(|byte| *byte == b'2').unwrap();

        let edits = [
            Edit {
                range: Span {
                    start: first,
                    end: first + 1,
                },
                replacement: b"100".to_vec(),
            },
            Edit {
                range: Span {
                    start: second,
                    end: second + 1,
                },
                replacement: b"200".to_vec(),
            },
        ];

        let updated = tree.update_many(&edits).unwrap();
        assert_eq!(updated.source(), b"local first = 100\nlocal second = 200");
        assert_eq!(tree.source(), b"local first = 1\nlocal second = 2");
        let conflicting = [edits[0].clone(), edits[0].clone()];

        assert_eq!(
            tree.update_many(&conflicting).unwrap_err(),
            EditError::OverlappingEdits
        );
    }
}
