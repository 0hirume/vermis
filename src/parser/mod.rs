pub(crate) mod builder;
pub(crate) mod context;
pub(crate) mod control;
mod expression;
mod statement;
mod types;

use crate::lexer::State;
use crate::{Diagnostic, InterpolatedKind, Keyword, Kind, Operator, Span, Token, TokenKind, Tree};
use builder::{Builder, Node};
use context::{Boundary, Context, Expectation, Expected, Rule};
use control::{Execution, Ledger, ParseError};
use std::{cell::RefCell, sync::Arc};

type Parsed = Result<usize, Diagnostic>;

struct Frame {
    context: Context,
    start: usize,
    inspected: Vec<Span>,
    diagnostics: usize,
    recovery: Vec<Expectation>,
    maximum_depth: usize,
}

struct Checkpoint {
    nodes: usize,
    children: usize,
    diagnostics: usize,
    origins: usize,
    cursor: usize,
    end: usize,
    depth: usize,
    previous: Option<Kind>,
    frames: Vec<(usize, usize, usize)>,
    ledger: Option<Ledger>,
}

struct Parser<'source> {
    builder: Builder<'source>,
    cursor: usize,
    end: usize,
    depth: usize,
    previous: Option<Kind>,
    frames: RefCell<Vec<Frame>>,
    execution: Option<Arc<Execution>>,
}

#[must_use]
pub fn parse(source: &[u8]) -> Tree {
    parse_source(source, None).finish(crate::source::Source::from(source))
}

pub(crate) fn controlled_in<'source>(
    source: &'source [u8],
    execution: &Arc<Execution>,
) -> Result<Builder<'source>, ParseError> {
    let builder = parse_source(source, Some(execution.clone()));

    match builder.error {
        Some(error) => Err(error),
        None => Ok(builder),
    }
}

pub(crate) fn controlled_unit_in<'source>(
    source: &'source [u8],
    context: &Context,
    execution: &Arc<Execution>,
) -> Result<Builder<'source>, ParseError> {
    let builder = replay(source, context, Some(execution.clone()));

    match builder.error {
        Some(error) => Err(error),
        None => Ok(builder),
    }
}

fn replay<'source>(
    source: &'source [u8],
    context: &Context,
    execution: Option<Arc<Execution>>,
) -> Builder<'source> {
    let mut parser = Parser::new(
        source,
        context.depth,
        context.previous,
        &context.lexical,
        execution,
    );

    parser.skip_trivia();
    let start = parser.raw_current().span.start;
    let cursor = parser.cursor;
    parser.enter(context.rule.clone(), start, parser.entry_state());
    let checkpoint = parser.checkpoint();

    let result = match &context.rule {
        Rule::Statement => parser.statement(),
        Rule::Expression(minimum) => parser.expression(*minimum),

        Rule::Annotation {
            allow_pack,
            declaration,
        } => parser.annotation_context(*allow_pack, *declaration),

        Rule::Condition => parser.condition(),
        Rule::Arguments => parser.arguments(),
        Rule::TypeArguments => parser.type_arguments(),

        Rule::Parameters { types } => {
            if *types {
                parser.type_parameters()
            } else {
                parser.parameters()
            }
        }

        Rule::Generics { defaults } => parser.generics(*defaults),
        Rule::Block(stops) => Ok(parser.block(stops)),
    };

    parser.builder.root = match result {
        Ok(node) => node,

        Err(error) => {
            parser.rollback(checkpoint);
            parser.diagnose(error);
            parser.recover(cursor, &[]);
            let node = parser.node(Kind::Error, start, []);
            parser.claim(node, 0);

            node
        }
    };

    if parser.builder.nodes[parser.builder.root].span.start > start {
        let recovery: Vec<_> = parser
            .builder
            .nodes
            .iter()
            .flat_map(|node| node.recovery.iter().cloned())
            .collect();

        parser.builder.nodes.clear();
        parser.builder.children.clear();
        parser.builder.origins.fill(None);
        parser.builder.root = parser.node(Kind::Error, start, []);

        parser.builder.nodes[parser.builder.root]
            .recovery
            .extend(recovery);

        parser.claim(parser.builder.root, 0);
    }

    let frame = parser
        .frames
        .borrow_mut()
        .pop()
        .expect("replay recovery scope");

    parser.builder.nodes[parser.builder.root]
        .recovery
        .extend(frame.recovery);

    if !frame.inspected.is_empty() {
        let consumed = parser.builder.nodes[parser.builder.root].span;
        let current = parser.raw_current();
        let exit = parser.entry_state();

        parser.builder.nodes[parser.builder.root]
            .boundaries
            .push(Boundary {
                context: frame.context,
                consumed,
                inspected: frame.inspected,
                exit,
                current,
                maximum_depth: frame.maximum_depth,
            });
    }

    parser.claim(parser.builder.root, 0);

    parser.builder.error = parser
        .execution
        .as_ref()
        .and_then(|execution| execution.error());

    parser.builder
}

fn parse_source(source: &[u8], execution: Option<Arc<Execution>>) -> Builder<'_> {
    let mut parser = Parser::new(source, 0, None, &State::default(), execution);
    parser.skip_trivia();
    let block = parser.block(&[]);
    parser.end = source.len();
    parser.builder.root = parser.node(Kind::Root, 0, [block]);
    parser.claim(parser.builder.root, 0);

    parser.builder.error = parser
        .execution
        .as_ref()
        .and_then(|execution| execution.error());

    parser.builder
}

impl<'source> Parser<'source> {
    fn new(
        source: &'source [u8],
        depth: usize,
        previous: Option<Kind>,
        lexical: &State,
        execution: Option<Arc<Execution>>,
    ) -> Self {
        if let Some(execution) = &execution {
            execution.source(0, source.len());
            execution.depth(depth);
        }

        Self {
            builder: Builder::with_state(source, lexical, execution.as_ref()),
            cursor: 0,
            end: 0,
            depth,
            previous,
            frames: RefCell::new(Vec::new()),
            execution,
        }
    }

    fn raw_current(&self) -> Token {
        self.builder.tokens[self.cursor]
    }

    fn active(&self) -> bool {
        self.execution
            .as_ref()
            .is_none_or(|execution| execution.poll())
    }

    fn inspect(&self, span: Span) {
        if let Some(frame) = self.frames.borrow_mut().last_mut() {
            frame.inspected.push(span);

            frame.maximum_depth = frame
                .maximum_depth
                .max(self.depth)
                .max(self.builder.checkpoints[self.cursor].state.braces.len());
        }
    }

    fn current(&self) -> Token {
        let mut token = self.raw_current();

        if !self.active() {
            token.kind = TokenKind::Eof;
            token.span.end = token.span.start;
        }

        self.inspect(token.span);

        token
    }

    fn entry_state(&self) -> State {
        self.builder.checkpoints[self.cursor].state.clone()
    }

    fn enter(&self, rule: Rule, start: usize, lexical: State) {
        self.frames.borrow_mut().push(Frame {
            context: Context {
                rule,
                depth: self.depth,
                previous: self.previous,
                lexical,
            },
            start,
            inspected: Vec::new(),
            diagnostics: self.builder.diagnostics.len(),
            recovery: Vec::new(),
            maximum_depth: self.depth,
        });
    }

    fn leave(&mut self, result: Parsed, current: Token, exit: State) -> Parsed {
        let mut frame = self.frames.borrow_mut().pop().expect("active grammar unit");

        match result {
            Ok(node) => {
                let start = self.builder.nodes[node].span.start;

                if frame.start < start {
                    if let Some(parent) = self.frames.borrow_mut().last_mut() {
                        parent.inspected.extend(frame.inspected);
                        parent.recovery.extend(frame.recovery);
                        parent.maximum_depth = parent.maximum_depth.max(frame.maximum_depth);
                    }

                    self.claim(node, frame.diagnostics);

                    return result;
                }

                frame
                    .inspected
                    .sort_unstable_by_key(|span| (span.start, span.end));

                frame.inspected.dedup();
                debug_assert!(frame.inspected.iter().all(|span| span.start >= frame.start));
                let mut inspected: Vec<Span> = Vec::new();

                for span in frame.inspected {
                    if let Some(previous) = inspected.last_mut()
                        && !span.is_empty()
                        && !previous.is_empty()
                        && span.start <= previous.end
                    {
                        previous.end = previous.end.max(span.end);
                    } else {
                        inspected.push(span);
                    }
                }

                let consumed = self.builder.nodes[node].span;

                self.builder.nodes[node].boundaries.push(Boundary {
                    context: frame.context,
                    consumed,
                    inspected,
                    exit,
                    current,
                    maximum_depth: frame.maximum_depth,
                });

                self.builder.nodes[node].recovery.extend(frame.recovery);
                self.builder.nodes[node].diagnostic_end = self.builder.diagnostics.len();
                self.claim(node, frame.diagnostics);
            }

            Err(_) => {
                if let Some(parent) = self.frames.borrow_mut().last_mut() {
                    parent.inspected.extend(frame.inspected);
                    parent.recovery.extend(frame.recovery);
                    parent.maximum_depth = parent.maximum_depth.max(frame.maximum_depth);
                }
            }
        }

        result
    }

    fn scoped(&mut self, rule: Rule, parse: impl FnOnce(&mut Self) -> Parsed) -> Parsed {
        let start = self.raw_current().span.start;
        self.enter(rule, start, self.entry_state());
        let result = parse(self);

        self.leave(result, self.raw_current(), self.entry_state())
    }

    fn diagnose(&mut self, diagnostic: Diagnostic) {
        if let Some(execution) = &self.execution {
            execution.diagnostic();
        }

        self.builder
            .origins
            .resize(self.builder.diagnostics.len(), None);

        self.builder.diagnostics.push(diagnostic);
        self.builder.origins.push(None);
    }

    fn claim(&mut self, node: usize, begin: usize) {
        self.builder
            .origins
            .resize(self.builder.diagnostics.len(), None);

        let start = self.builder.nodes[node].span.start;

        for (origin, diagnostic) in self.builder.origins[begin..]
            .iter_mut()
            .zip(&self.builder.diagnostics[begin..])
        {
            if origin.is_none() && diagnostic.span.start >= start {
                *origin = Some(node);
            }
        }
    }

    fn expectation(&self, span: Span, expected: Expected) {
        if let Some(frame) = self.frames.borrow_mut().last_mut() {
            frame.recovery.push(Expectation { span, expected });
        }
    }

    fn at(&self, kind: TokenKind) -> bool {
        self.current().kind == kind
    }

    fn byte(&self, byte: u8) -> bool {
        self.at(TokenKind::Byte(byte))
    }

    fn keyword(&self, keyword: Keyword) -> bool {
        self.at(TokenKind::Keyword(keyword))
    }

    fn named(&self, name: &[u8]) -> bool {
        self.at(TokenKind::Name) && self.current().bytes(self.builder.source) == name
    }

    fn next(&self) -> TokenKind {
        if !self.active() {
            return TokenKind::Eof;
        }

        let begin = self.cursor + usize::from(self.raw_current().kind != TokenKind::Eof);

        for token in &self.builder.tokens[begin..] {
            self.inspect(token.span);

            if !trivia(token.kind) {
                return token.kind;
            }
        }

        unreachable!("token stream ends in EOF")
    }

    fn skip_trivia(&mut self) {
        while trivia(self.raw_current().kind) {
            self.cursor += 1;
        }
    }

    fn take(&mut self) -> Token {
        let token = self.current();

        if token.kind != TokenKind::Eof {
            self.end = token.span.end;
            self.cursor += 1;
            self.skip_trivia();
        }

        token
    }

    fn consume(&mut self, kind: TokenKind) -> bool {
        if self.at(kind) {
            self.take();

            true
        } else {
            false
        }
    }

    fn expect(&mut self, kind: TokenKind, message: &'static str) {
        if !self.consume(kind) {
            let start = self.current().span.start;
            self.expectation(Span { start, end: start }, Expected::Token(kind));
            self.diagnose(self.error(message));
        }
    }

    fn close(&mut self, keyword: Keyword) {
        self.expect(TokenKind::Keyword(keyword), "missing block terminator");
    }

    fn error(&self, message: &'static str) -> Diagnostic {
        Diagnostic {
            span: self.current().span,
            message: match self.current().kind {
                TokenKind::Error(crate::LexError::BrokenString) => "unterminated string",
                TokenKind::Error(crate::LexError::BrokenComment) => "unterminated comment",
                TokenKind::Error(crate::LexError::BrokenUnicode { .. }) => "unexpected character",

                TokenKind::Error(crate::LexError::BrokenInterpolatedDoubleBrace) => {
                    "invalid interpolation delimiter"
                }

                _ => message,
            },
        }
    }

    fn node(
        &mut self,
        kind: Kind,
        start: usize,
        children: impl IntoIterator<Item = usize>,
    ) -> usize {
        if let Some(execution) = &self.execution {
            execution.node();
        }

        let index = self.builder.nodes.len();
        let begin = self.builder.children.len();
        self.builder.children.extend(children);
        let children = begin..self.builder.children.len();

        self.builder.nodes.push(Node {
            kind,
            span: Span {
                start,
                end: self.end.max(start).max(
                    self.builder.children[children.clone()]
                        .last()
                        .map_or(start, |child| self.builder.nodes[*child].span.end),
                ),
            },
            children,
            boundaries: Vec::new(),
            recovery: Vec::new(),
            diagnostic_end: self.builder.diagnostics.len(),
        });

        index
    }

    fn prepend(&mut self, node: usize, child: usize) {
        let children = &mut self.builder.nodes[node].children;
        assert_eq!(children.end, self.builder.children.len());
        self.builder.children.insert(children.start, child);
        children.end += 1;
    }

    fn leaf(&mut self, kind: Kind) -> usize {
        let token = self.take();

        self.node(kind, token.span.start, [])
    }

    fn name(&mut self) -> usize {
        self.required("name", |parser| {
            if parser.at(TokenKind::Name) {
                Ok(parser.leaf(Kind::Name))
            } else {
                Err(parser.error("expected name"))
            }
        })
    }

    fn missing(&mut self, error: Diagnostic, role: &'static str) -> usize {
        let start = self.current().span.start;
        let diagnostic = self.builder.diagnostics.len();
        self.diagnose(error);
        let node = self.node(Kind::Missing, start, []);
        self.builder.nodes[node].span.end = start;

        self.builder.nodes[node].recovery.push(Expectation {
            span: Span { start, end: start },
            expected: Expected::Role(role),
        });

        self.claim(node, diagnostic);

        node
    }

    fn checkpoint(&self) -> Checkpoint {
        Checkpoint {
            nodes: self.builder.nodes.len(),
            children: self.builder.children.len(),
            diagnostics: self.builder.diagnostics.len(),
            origins: self.builder.origins.len(),
            cursor: self.cursor,
            end: self.end,
            depth: self.depth,
            previous: self.previous,
            frames: self
                .frames
                .borrow()
                .iter()
                .map(|frame| {
                    (
                        frame.inspected.len(),
                        frame.recovery.len(),
                        frame.maximum_depth,
                    )
                })
                .collect(),
            ledger: self
                .execution
                .as_ref()
                .map(|execution| execution.snapshot()),
        }
    }

    fn restore(&mut self, checkpoint: Checkpoint) {
        self.builder.nodes.truncate(checkpoint.nodes);
        self.builder.children.truncate(checkpoint.children);
        self.builder.diagnostics.truncate(checkpoint.diagnostics);
        self.builder.origins.truncate(checkpoint.origins);
        self.cursor = checkpoint.cursor;
        self.end = checkpoint.end;
        self.depth = checkpoint.depth;
        self.previous = checkpoint.previous;
        self.frames.borrow_mut().truncate(checkpoint.frames.len());

        for (frame, (inspected, recovery, maximum_depth)) in
            self.frames.borrow_mut().iter_mut().zip(checkpoint.frames)
        {
            frame.inspected.truncate(inspected);
            frame.recovery.truncate(recovery);
            frame.maximum_depth = maximum_depth;
        }

        if let (Some(execution), Some(ledger)) = (&self.execution, checkpoint.ledger) {
            execution.restore(ledger);
        }
    }

    fn rollback(&mut self, checkpoint: Checkpoint) {
        let nodes = checkpoint.nodes;
        let ledger = checkpoint.ledger;
        let diagnostics_before = checkpoint.diagnostics;
        let diagnostics = self.builder.diagnostics[checkpoint.diagnostics..].to_vec();

        self.builder
            .origins
            .resize(self.builder.diagnostics.len(), None);

        let origins = self.builder.origins[checkpoint.diagnostics..].to_vec();
        let cursor = self.cursor;
        let end = self.end;

        let frames: Vec<_> = self
            .frames
            .borrow()
            .iter()
            .zip(&checkpoint.frames)
            .map(|(frame, (inspected, recovery, _))| {
                (
                    frame.inspected[*inspected..].to_vec(),
                    frame.recovery[*recovery..].to_vec(),
                    frame.maximum_depth,
                )
            })
            .collect();

        let discarded: Vec<_> = self.builder.nodes[nodes..]
            .iter()
            .flat_map(|node| node.boundaries.iter())
            .flat_map(|boundary| boundary.inspected.iter().copied())
            .collect();

        let recovery: Vec<_> = self.builder.nodes[nodes..]
            .iter()
            .flat_map(|node| node.recovery.iter().cloned())
            .collect();

        self.restore(checkpoint);
        self.cursor = cursor;
        self.end = end;

        for (frame, (inspected, recovery, maximum_depth)) in
            self.frames.borrow_mut().iter_mut().zip(frames)
        {
            frame.inspected.extend(inspected);
            frame.recovery.extend(recovery);
            frame.maximum_depth = frame.maximum_depth.max(maximum_depth);
        }

        if let Some(frame) = self.frames.borrow_mut().last_mut() {
            frame.inspected.extend(discarded);
            frame.recovery.extend(recovery);
        }

        self.builder.diagnostics.extend(diagnostics);

        self.builder.origins.extend(
            origins
                .into_iter()
                .map(|origin| origin.filter(|origin| *origin < nodes)),
        );

        for origin in &mut self.builder.origins {
            if origin.is_some_and(|origin| origin >= nodes) {
                *origin = None;
            }
        }

        if let (Some(execution), Some(ledger)) = (&self.execution, ledger) {
            execution.retain(ledger, diagnostics_before, self.builder.diagnostics.len());
        }
    }

    fn required(&mut self, role: &'static str, parse: impl FnOnce(&mut Self) -> Parsed) -> usize {
        let checkpoint = self.checkpoint();
        let diagnostic = checkpoint.diagnostics;

        match parse(self) {
            Ok(node) => node,

            Err(error) => {
                self.rollback(checkpoint);
                let node = self.missing(error, role);
                self.claim(node, diagnostic);

                node
            }
        }
    }

    fn nested(&mut self, parse: impl FnOnce(&mut Self) -> Parsed) -> Parsed {
        if self
            .execution
            .as_ref()
            .is_some_and(|execution| !execution.depth(self.depth.saturating_add(1)))
        {
            return Err(self.error("parser resource limit exceeded"));
        }

        if self.depth >= 256 {
            return Err(self.error("syntax nesting limit exceeded"));
        }

        self.depth += 1;
        let result = parse(self);
        self.depth -= 1;

        result
    }

    fn block(&mut self, stops: &[Keyword]) -> usize {
        self.scoped(Rule::Block(stops.to_vec()), |parser| {
            Ok(parser.block_contents(stops))
        })
        .expect("block recovers")
    }

    fn block_contents(&mut self, stops: &[Keyword]) -> usize {
        let start = self.current().span.start;
        let mut statements: Vec<usize> = Vec::new();
        let previous = self.previous.take();

        while !self.at(TokenKind::Eof) && !stops.iter().any(|stop| self.keyword(*stop)) {
            let previous = statements
                .last()
                .map(|statement| self.builder.nodes[*statement].kind)
                .or(previous);

            if previous
                .is_some_and(|kind| matches!(kind, Kind::Return | Kind::Break | Kind::Continue))
            {
                self.diagnose(self.error("statement follows a block-ending statement"));
            }

            let cursor = self.cursor;
            let checkpoint = self.checkpoint();
            let diagnostic = self.builder.diagnostics.len();
            let begin = self.current().span.start;
            self.previous = previous;

            match self.nested(Self::statement) {
                Ok(statement) => {
                    statements.push(statement);
                    self.consume(TokenKind::Byte(b';'));

                    if self.cursor == cursor {
                        self.take();
                    }
                }

                Err(error) => {
                    self.rollback(checkpoint);
                    self.diagnose(error);
                    self.recover(cursor, stops);
                    let recovery = self.node(Kind::Error, begin, []);
                    self.claim(recovery, diagnostic);
                    statements.push(recovery);
                }
            }

            self.previous = None;
        }

        self.node(Kind::Block, start, statements)
    }

    fn recover(&mut self, start: usize, stops: &[Keyword]) {
        if self.consume(TokenKind::Byte(b';')) {
            return;
        }

        while !self.at(TokenKind::Eof) {
            if stops.iter().any(|stop| self.keyword(*stop)) {
                break;
            }

            if self.cursor > start {
                if self.consume(TokenKind::Byte(b';')) {
                    break;
                }

                let span = Span {
                    start: self.end,
                    end: self.current().span.start,
                };

                self.inspect(span);
                let gap = span.bytes(self.builder.source);

                if gap.contains(&b'\n')
                    || matches!(
                        self.current().kind,
                        TokenKind::Keyword(
                            Keyword::Local
                                | Keyword::Function
                                | Keyword::If
                                | Keyword::While
                                | Keyword::For
                                | Keyword::Return
                        )
                    )
                {
                    break;
                }
            }

            self.take();
        }
    }

    fn condition(&mut self) -> Parsed {
        self.scoped(Rule::Condition, Self::condition_contents)
    }

    fn condition_contents(&mut self) -> Parsed {
        let constant = self.named(b"const") && self.next() == TokenKind::Name;

        if !self.keyword(Keyword::Local) && !constant {
            return self.expression(0);
        }

        let start = self.take().span.start;
        let binding = self.binding()?;
        self.expect(TokenKind::Byte(b'='), "expected condition initializer");
        let value = self.expression(0)?;

        Ok(self.node(
            if constant {
                Kind::Constant
            } else {
                Kind::Local
            },
            start,
            [binding, value],
        ))
    }

    fn binding(&mut self) -> Parsed {
        let start = self.current().span.start;
        let name = self.name();

        let annotation = if self.consume(TokenKind::Byte(b':')) {
            Some(self.annotation()?)
        } else {
            None
        };

        Ok(self.node(Kind::Binding, start, [name].into_iter().chain(annotation)))
    }

    fn expressions(&mut self) -> Result<Vec<usize>, Diagnostic> {
        let mut children = vec![self.expression(0)?];

        while self.consume(TokenKind::Byte(b',')) {
            children.push(self.expression(0)?);
        }

        Ok(children)
    }
}

fn trivia(kind: TokenKind) -> bool {
    matches!(
        kind,
        TokenKind::Whitespace | TokenKind::Comment | TokenKind::BlockComment
    )
}
