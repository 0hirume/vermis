pub(crate) mod builder;
pub(crate) mod context;
pub(crate) mod control;
mod expression;
mod markup;
mod statement;
mod types;

use crate::lexer::{Checkpoint as LexicalCheckpoint, State};
use crate::{Diagnostic, InterpolatedKind, Keyword, Kind, Operator, Span, Token, TokenKind, Tree};
use builder::{Builder, Node};
use context::{Boundary, Context, Expectation, Expected, Rule};
use control::{Control, Execution, Ledger, ParseError};
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

struct Checkpoint<'source> {
    nodes: usize,
    children: usize,
    tokens: usize,
    pending: Vec<Token>,
    checkpoints: Vec<LexicalCheckpoint>,
    diagnostics: usize,
    origins: usize,
    cursor: usize,
    end: usize,
    depth: usize,
    previous: Option<Kind>,
    lexer: Option<crate::Lexer<'source>>,
    lexical: State,
    frames: Vec<(usize, usize, usize)>,
    ledger: Option<Ledger>,
}

struct Parser<'source, const MARKUP: bool> {
    builder: Builder<'source>,
    source: &'source [u8],
    cursor: usize,
    end: usize,
    depth: usize,
    lexer: Option<crate::Lexer<'source>>,
    previous: Option<Kind>,
    lexical: State,
    frames: RefCell<Vec<Frame>>,
    execution: Option<Arc<Execution>>,
}

#[must_use]
pub fn parse(source: &[u8]) -> Tree {
    parse_source::<false>(source, None).finish(crate::source::Source::from(source), false)
}

#[must_use]
pub fn parse_luaux(source: &[u8]) -> Tree {
    parse_source::<true>(source, None).finish(crate::source::Source::from(source), true)
}

pub(crate) fn controlled(
    source: &[u8],
    markup: bool,
    control: &Control,
) -> Result<Tree, ParseError> {
    let execution = Execution::new(control);
    let builder = controlled_in(source, markup, &execution)?;

    Tree::freeze(
        builder,
        crate::source::Source::from(source),
        markup,
        None,
        Some(&execution),
    )
}

pub(crate) fn controlled_in<'source>(
    source: &'source [u8],
    markup: bool,
    execution: &Arc<Execution>,
) -> Result<Builder<'source>, ParseError> {
    let builder = if markup {
        parse_source::<true>(source, Some(execution.clone()))
    } else {
        parse_source::<false>(source, Some(execution.clone()))
    };

    match builder.error {
        Some(error) => Err(error),
        None => Ok(builder),
    }
}

pub(crate) fn controlled_unit_in<'source>(
    source: &'source [u8],
    markup: bool,
    context: &Context,
    execution: &Arc<Execution>,
) -> Result<Builder<'source>, ParseError> {
    let builder = if markup {
        replay::<true>(source, context, Some(execution.clone()))
    } else {
        replay::<false>(source, context, Some(execution.clone()))
    };

    match builder.error {
        Some(error) => Err(error),
        None => Ok(builder),
    }
}

fn replay<'source, const MARKUP: bool>(
    source: &'source [u8],
    context: &Context,
    execution: Option<Arc<Execution>>,
) -> Builder<'source> {
    let mut parser = Parser::<MARKUP>::new(
        source,
        context.depth,
        context.previous,
        context.lexical.clone(),
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

        Rule::Markup => {
            if MARKUP {
                parser.markup()
            } else {
                Err(parser.error("markup is disabled"))
            }
        }
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

fn parse_source<const MARKUP: bool>(
    source: &[u8],
    execution: Option<Arc<Execution>>,
) -> Builder<'_> {
    let mut parser = Parser::<MARKUP>::new(source, 0, None, State::default(), execution);
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

impl<'source, const MARKUP: bool> Parser<'source, MARKUP> {
    fn new(
        source: &'source [u8],
        depth: usize,
        previous: Option<Kind>,
        lexical: State,
        execution: Option<Arc<Execution>>,
    ) -> Self {
        if let Some(execution) = &execution {
            execution.source(0, source.len());
            execution.depth(depth);
        }

        Self {
            builder: Builder::with_state(source, MARKUP, &lexical, execution.as_ref()),
            source,
            cursor: 0,
            end: 0,
            depth,
            lexer: if MARKUP {
                Some(crate::Lexer::controlled(
                    source,
                    0,
                    &lexical,
                    execution.clone(),
                ))
            } else {
                None
            },
            previous,
            lexical,
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
                .max(self.lexical.braces.len());

            if let Some(checkpoint) = self.builder.checkpoints.get(self.cursor) {
                frame.maximum_depth = frame.maximum_depth.max(checkpoint.state.braces.len());
            }
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
        if let Some(checkpoint) = self.builder.checkpoints.get(self.cursor) {
            checkpoint.state.clone()
        } else {
            self.lexical.clone()
        }
    }

    fn enter(&self, rule: Rule, start: usize, lexical: State) {
        self.frames.borrow_mut().push(Frame {
            context: Context {
                rule,
                markup: MARKUP,
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

        if MARKUP {
            let mut lexer = self.lexer.as_ref().expect("markup token stream").clone();

            for token in &mut lexer {
                self.inspect(token.span);

                if !trivia(token.kind) {
                    return token.kind;
                }
            }

            self.inspect(Span {
                start: self.source.len(),
                end: self.source.len(),
            });

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
        if !MARKUP {
            while trivia(self.raw_current().kind) {
                self.cursor += 1;
            }

            return;
        }

        loop {
            if self.cursor == self.builder.tokens.len() {
                let lexer = self.lexer.as_mut().expect("lazy token stream");
                let checkpoint = lexer.checkpoint();

                let token = match lexer.next() {
                    Some(token)
                        if self
                            .execution
                            .as_ref()
                            .is_none_or(|execution| execution.token()) =>
                    {
                        token
                    }

                    _ if self
                        .execution
                        .as_ref()
                        .is_some_and(|execution| !execution.poll()) =>
                    {
                        Token {
                            kind: TokenKind::Eof,
                            span: Span {
                                start: checkpoint.cursor,
                                end: checkpoint.cursor,
                            },
                        }
                    }

                    _ => panic!("token stream ends in EOF"),
                };

                self.builder.tokens.push(token);
                self.builder.checkpoints.push(checkpoint);
            }

            if !trivia(self.raw_current().kind) {
                break;
            }

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

    fn checkpoint(&self) -> Checkpoint<'source> {
        Checkpoint {
            nodes: self.builder.nodes.len(),
            children: self.builder.children.len(),
            tokens: self.builder.tokens.len(),
            pending: if MARKUP {
                self.builder.tokens[self.cursor..].to_vec()
            } else {
                Vec::new()
            },
            checkpoints: if MARKUP {
                self.builder.checkpoints[self.cursor..].to_vec()
            } else {
                Vec::new()
            },
            diagnostics: self.builder.diagnostics.len(),
            origins: self.builder.origins.len(),
            cursor: self.cursor,
            end: self.end,
            depth: self.depth,
            previous: self.previous,
            lexer: self.lexer.clone(),
            lexical: self.lexical.clone(),
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

    fn restore(&mut self, checkpoint: Checkpoint<'source>) {
        self.builder.nodes.truncate(checkpoint.nodes);
        self.builder.children.truncate(checkpoint.children);

        self.builder.tokens.truncate(if MARKUP {
            checkpoint.cursor
        } else {
            checkpoint.tokens
        });

        self.builder.checkpoints.truncate(if MARKUP {
            checkpoint.cursor
        } else {
            checkpoint.tokens
        });

        self.builder.tokens.extend(checkpoint.pending);
        self.builder.checkpoints.extend(checkpoint.checkpoints);
        self.builder.diagnostics.truncate(checkpoint.diagnostics);
        self.builder.origins.truncate(checkpoint.origins);
        self.cursor = checkpoint.cursor;
        self.end = checkpoint.end;
        self.depth = checkpoint.depth;
        self.previous = checkpoint.previous;
        self.lexer = checkpoint.lexer;
        self.lexical = checkpoint.lexical;
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

    fn rollback(&mut self, checkpoint: Checkpoint<'source>) {
        let nodes = checkpoint.nodes;
        let ledger = checkpoint.ledger;
        let tokens_before = checkpoint.tokens;
        let diagnostics_before = checkpoint.diagnostics;
        let diagnostics = self.builder.diagnostics[checkpoint.diagnostics..].to_vec();

        self.builder
            .origins
            .resize(self.builder.diagnostics.len(), None);

        let origins = self.builder.origins[checkpoint.diagnostics..].to_vec();
        let tokens = std::mem::take(&mut self.builder.tokens);
        let checkpoints = std::mem::take(&mut self.builder.checkpoints);
        let cursor = self.cursor;
        let end = self.end;
        let lexer = self.lexer.take();
        let lexical = self.lexical.clone();

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
        self.builder.tokens = tokens;
        self.builder.checkpoints = checkpoints;
        self.cursor = cursor;
        self.end = end;
        self.lexer = lexer;
        self.lexical = lexical;

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
            execution.retain(
                ledger,
                tokens_before,
                self.builder.tokens.len(),
                diagnostics_before,
                self.builder.diagnostics.len(),
            );
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

    fn truncate_tokens(&mut self, length: usize) {
        if let Some(execution) = &self.execution {
            execution.discard_tokens(self.builder.tokens.len() - length);
        }

        self.builder.tokens.truncate(length);
        self.builder.checkpoints.truncate(length);
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
