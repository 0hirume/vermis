use std::{ops::Range, sync::Arc};

use super::context::{Boundary, Expectation};
use super::control::{Execution, ParseError};

use crate::{
    Diagnostic, Kind, Lexer, Span, Token,
    lexer::{Checkpoint, State},
    source::Source,
    tree::Tree,
};

pub(crate) struct Node {
    pub kind: Kind,
    pub span: Span,
    pub children: Range<usize>,
    pub boundaries: Vec<Boundary>,
    pub recovery: Vec<Expectation>,
    pub diagnostic_end: usize,
}

pub(crate) struct Builder<'source> {
    pub source: &'source [u8],
    pub tokens: Vec<Token>,
    pub checkpoints: Vec<Checkpoint>,
    pub nodes: Vec<Node>,
    pub children: Vec<usize>,
    pub root: usize,
    pub diagnostics: Vec<Diagnostic>,
    pub origins: Vec<Option<usize>>,
    pub error: Option<ParseError>,
}

impl<'source> Builder<'source> {
    pub(super) fn with_state(
        source: &'source [u8],
        lazy: bool,
        state: &State,
        execution: Option<&Arc<Execution>>,
    ) -> Self {
        let mut builder = Self {
            source,
            tokens: Vec::new(),
            checkpoints: Vec::new(),
            nodes: Vec::new(),
            children: Vec::new(),
            root: 0,
            diagnostics: Vec::new(),
            origins: Vec::new(),
            error: None,
        };

        if !lazy {
            let mut lexer = Lexer::controlled(source, 0, state, execution.cloned());

            loop {
                let checkpoint = lexer.checkpoint();

                let Some(token) = lexer.next() else { break };

                if execution.is_some_and(|execution| !execution.token()) {
                    break;
                }

                builder.tokens.push(token);
                builder.checkpoints.push(checkpoint);
            }

            if builder
                .tokens
                .last()
                .is_none_or(|token| token.kind != crate::TokenKind::Eof)
            {
                let cursor = builder.tokens.last().map_or(0, |token| token.span.end);

                builder.tokens.push(Token {
                    kind: crate::TokenKind::Eof,
                    span: Span {
                        start: cursor,
                        end: cursor,
                    },
                });

                builder.checkpoints.push(Checkpoint {
                    cursor,
                    state: lexer.state(),
                    finished: false,
                });
            }
        }

        builder.error = execution.and_then(|execution| execution.error());

        builder
    }

    pub(super) fn text(&self, node: usize) -> &[u8] {
        self.nodes[node].span.bytes(self.source)
    }

    pub(crate) fn finish(self, source: Source, markup: bool) -> Tree {
        Tree::from_builder(self, source, markup)
    }
}
