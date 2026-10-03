mod expression;
mod statement;
mod types;

use crate::lexer::Lexer;
use crate::token::{Keyword, Span, Symbol, Token, TokenKind};
use crate::tree::{Diagnostic, ListEntry, Node, NodeIndex, NodeKind, NodeList, TokenIndex, Tree};

/// Parser state and indexed syntax builder.
pub struct Parser<'source> {
    source: &'source [u8],
    tokens: Vec<Token>,
    nodes: Vec<Node>,
    lists: Vec<ListEntry>,
    diagnostics: Vec<Diagnostic>,
    cursor: usize,
    token_end: usize,
    end: usize,
    depth: usize,
    loop_depth: usize,
}

impl<'source> Parser<'source> {
    /// Tokenizes source and positions the parser at its first non-trivia token.
    pub fn new(source: &'source [u8]) -> Self {
        let mut parser = Self {
            source,
            tokens: Lexer::new(source).collect(),
            nodes: Vec::new(),
            lists: Vec::new(),
            diagnostics: Vec::new(),
            cursor: 0,
            token_end: 0,
            end: 0,
            depth: 0,
            loop_depth: 0,
        };

        parser.skip_trivia();

        parser
    }

    /// Returns the current token index.
    pub fn position(&self) -> TokenIndex {
        TokenIndex(self.cursor)
    }

    /// Returns the current non-trivia token.
    pub fn current(&self) -> Token {
        self.tokens[self.cursor]
    }

    /// Returns the next non-trivia token kind, staying at end-of-input.
    ///
    /// # Panics
    /// Panics if the internal token stream lacks its end-of-input sentinel.
    pub fn lookahead(&self) -> TokenKind {
        let start = self.cursor + usize::from(self.current().kind != TokenKind::EndOfFile);

        self.tokens[start..]
            .iter()
            .find(|token| !is_trivia(token.kind))
            .expect("token stream ends in end-of-file")
            .kind
    }

    /// Tests the current token kind.
    pub fn at(&self, kind: TokenKind) -> bool {
        self.current().kind == kind
    }

    /// Tests whether the current token is the given name.
    pub fn named(&self, name: &[u8]) -> bool {
        self.at(TokenKind::Name) && self.current().bytes(self.source) == name
    }

    /// Parses a name, inserting a missing node on failure.
    pub fn name(&mut self) -> NodeIndex {
        self.required("name", |parser| {
            if !parser.at(TokenKind::Name) {
                return Err(parser.error("expected name"));
            }

            let token = parser.take();

            Ok(parser.append_node(token, NodeKind::Name { token }))
        })
    }

    /// Consumes the current token, staying at end-of-input.
    pub fn take(&mut self) -> TokenIndex {
        let index = self.position();
        let token = self.current();

        if token.kind != TokenKind::EndOfFile {
            self.cursor += 1;
            self.token_end = self.cursor;
            self.end = token.span.end;
            self.skip_trivia();
        }

        index
    }

    /// Consumes the current token if its kind matches.
    pub fn consume(&mut self, kind: TokenKind) -> Option<TokenIndex> {
        if self.at(kind) {
            Some(self.take())
        } else {
            None
        }
    }

    /// Consumes a matching token or records the given diagnostic.
    pub fn expect(&mut self, kind: TokenKind, message: &'static str) -> Option<TokenIndex> {
        let token = self.consume(kind);

        if token.is_none() {
            self.diagnose(self.error(message));
        }

        token
    }

    /// Builds a diagnostic at the current token, preferring lexical errors.
    pub fn error(&self, message: &'static str) -> Diagnostic {
        let token = self.current();

        Diagnostic {
            span: token.diagnostic_span(),
            message: match token.kind {
                TokenKind::MalformedString => "unterminated string",
                TokenKind::MalformedComment => "unterminated comment",

                TokenKind::InvalidUnicode { .. } | TokenKind::InvalidCharacter { .. } => {
                    "unexpected character"
                }

                TokenKind::InvalidInterpolationDoubleBrace => "invalid interpolation delimiter",
                _ => message,
            },
        }
    }

    /// Appends a diagnostic in emission order.
    pub fn diagnose(&mut self, diagnostic: Diagnostic) {
        self.diagnostics.push(diagnostic);
    }

    /// Returns a previously constructed node.
    pub fn node(&self, index: NodeIndex) -> &Node {
        &self.nodes[index.0]
    }

    /// Appends a node covering tokens consumed since `start`.
    pub fn append_node(&mut self, start: TokenIndex, kind: NodeKind) -> NodeIndex {
        debug_assert!(start.0 <= self.cursor);
        let index = NodeIndex(self.nodes.len());
        let begin = self.tokens[start.0].span.start;
        self.end = self.end.max(begin);
        self.token_end = self.token_end.max(start.0);

        self.nodes.push(Node {
            kind,
            span: Span {
                start: begin,
                end: self.end,
            },
            tokens: start..TokenIndex(self.token_end),
        });

        index
    }

    /// Appends list entries and returns their storage range.
    pub fn append_list(&mut self, entries: impl IntoIterator<Item = ListEntry>) -> NodeList {
        let start = self.lists.len();
        self.lists.extend(entries);

        NodeList(start..self.lists.len())
    }

    /// Records a diagnostic and inserts a zero-width missing node.
    pub fn missing(&mut self, expected: &'static str, diagnostic: Diagnostic) -> NodeIndex {
        self.diagnose(diagnostic);
        let position = self.position();
        let start = self.current().span.start;
        let index = NodeIndex(self.nodes.len());

        self.nodes.push(Node {
            kind: NodeKind::Missing { expected },
            span: Span { start, end: start },
            tokens: position..position,
        });

        self.end = start;
        self.token_end = self.cursor;

        index
    }

    /// Runs a parser, replacing failed syntax with a missing node.
    pub fn required(
        &mut self,
        expected: &'static str,
        parse: impl FnOnce(&mut Self) -> Result<NodeIndex, Diagnostic>,
    ) -> NodeIndex {
        let nodes = self.nodes.len();
        let lists = self.lists.len();

        match parse(self) {
            Ok(node) => node,

            Err(diagnostic) => {
                self.nodes.truncate(nodes);
                self.lists.truncate(lists);

                self.missing(expected, diagnostic)
            }
        }
    }

    /// Runs a parser within the syntax nesting limit.
    ///
    /// # Errors
    /// Returns a diagnostic if the limit is reached or the parser fails.
    pub fn nested(
        &mut self,
        parse: impl FnOnce(&mut Self) -> Result<NodeIndex, Diagnostic>,
    ) -> Result<NodeIndex, Diagnostic> {
        if self.depth >= 256 {
            return Err(self.error("syntax nesting limit exceeded"));
        }

        self.depth += 1;
        let result = parse(self);
        self.depth -= 1;

        result
    }

    pub(super) fn block(&mut self, stops: &[Keyword]) -> NodeIndex {
        let start = self.position();
        let mut statements = Vec::new();
        let mut terminal = false;

        let boundaries = [
            TokenKind::Keyword(Keyword::End),
            TokenKind::Keyword(Keyword::Else),
            TokenKind::Keyword(Keyword::ElseIf),
            TokenKind::Keyword(Keyword::Until),
        ];

        while !self.at(TokenKind::EndOfFile) {
            if let TokenKind::Keyword(keyword) = self.current().kind
                && (stops.contains(&keyword) || (!stops.is_empty() && self.block_end()))
            {
                break;
            }

            if terminal {
                self.diagnose(self.error("statement follows a block-ending statement"));
                terminal = false;
            }

            let begin = self.position();
            let nodes = self.nodes.len();
            let lists = self.lists.len();
            let end = self.end;
            let token_end = self.token_end;

            match self.statement() {
                Ok(node) => {
                    terminal = matches!(
                        self.node(node).kind,
                        NodeKind::Return { .. }
                            | NodeKind::Break { .. }
                            | NodeKind::Continue { .. }
                    );

                    let separator = self.consume(TokenKind::Symbol(Symbol::Semicolon));
                    statements.push(ListEntry { node, separator });
                }

                Err(diagnostic) => {
                    self.nodes.truncate(nodes);
                    self.lists.truncate(lists);
                    self.cursor = begin.0;
                    self.end = end;
                    self.token_end = token_end;
                    let node = self.missing("statement", diagnostic);

                    statements.push(ListEntry {
                        node,
                        separator: None,
                    });

                    let node =
                        self.recover(begin, if stops.is_empty() { &[] } else { &boundaries });

                    statements.push(ListEntry {
                        node,
                        separator: None,
                    });
                }
            }

            if self.position() == begin {
                let token = self.take();
                let node = self.append_node(token, NodeKind::Error);

                statements.push(ListEntry {
                    node,
                    separator: None,
                });
            }
        }

        let statements = self.append_list(statements);

        self.append_node(start, NodeKind::Block { statements })
    }

    pub(super) fn block_end(&self) -> bool {
        matches!(
            self.current().kind,
            TokenKind::EndOfFile
                | TokenKind::Keyword(
                    Keyword::End | Keyword::Else | Keyword::ElseIf | Keyword::Until
                )
        )
    }

    /// Consumes erroneous syntax up to a recovery boundary.
    pub fn recover(&mut self, start: TokenIndex, stops: &[TokenKind]) -> NodeIndex {
        debug_assert!(start.0 <= self.cursor);

        while !self.at(TokenKind::EndOfFile) && !stops.contains(&self.current().kind) {
            if self.consume(TokenKind::Symbol(Symbol::Semicolon)).is_some() {
                break;
            }

            if self.cursor > start.0 {
                let gap = &self.source[self.end..self.current().span.start];

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

        self.append_node(start, NodeKind::Error)
    }

    /// Wraps a completed block in a root and returns the syntax tree.
    pub fn finish(mut self, block: NodeIndex) -> Tree<'source> {
        debug_assert!(self.at(TokenKind::EndOfFile));
        debug_assert!(matches!(self.node(block).kind, NodeKind::Block { .. }));
        let root = NodeIndex(self.nodes.len());

        self.nodes.push(Node {
            kind: NodeKind::Root {
                block,
                end_of_file: self.position(),
            },
            span: Span {
                start: 0,
                end: self.source.len(),
            },
            tokens: TokenIndex(0)..TokenIndex(self.tokens.len()),
        });

        Tree {
            source: self.source,
            tokens: self.tokens,
            nodes: self.nodes,
            lists: self.lists,
            root,
            diagnostics: self.diagnostics,
        }
    }

    fn skip_trivia(&mut self) {
        while is_trivia(self.current().kind) {
            self.cursor += 1;
        }
    }
}

/// Parses source bytes, retaining malformed syntax and reporting diagnostics.
pub fn parse(source: &[u8]) -> Tree<'_> {
    let mut parser = Parser::new(source);
    let block = parser.block(&[]);

    parser.finish(block)
}

fn is_trivia(kind: TokenKind) -> bool {
    matches!(
        kind,
        TokenKind::Whitespace | TokenKind::Comment | TokenKind::BlockComment
    )
}
