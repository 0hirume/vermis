mod expression;
mod markup;
mod statement;
mod types;

use crate::{
    Diagnostic, InterpolatedKind, Keyword, Kind, Node, Operator, Span, Token, TokenKind, Tree,
    tokenize,
};
use bstr::{BStr, ByteSlice};

type Parsed = Result<usize, Diagnostic>;

struct Parser<'source, const MARKUP: bool> {
    tree: Tree<'source>,
    cursor: usize,
    end: usize,
    depth: usize,
    lexer: Option<crate::Lexer<'source>>,
}

#[must_use]
pub fn parse(source: &BStr) -> Tree<'_> {
    parse_source::<false>(source)
}

#[must_use]
pub fn parse_luaux(source: &BStr) -> Tree<'_> {
    parse_source::<true>(source)
}

fn parse_source<const MARKUP: bool>(source: &BStr) -> Tree<'_> {
    let mut parser = Parser::<MARKUP> {
        tree: Tree {
            source,
            tokens: if MARKUP { Vec::new() } else { tokenize(source) },
            nodes: Vec::new(),
            children: Vec::new(),
            root: 0,
            diagnostics: Vec::new(),
        },
        cursor: 0,
        end: 0,
        depth: 0,
        lexer: if MARKUP {
            Some(crate::Lexer::new(source))
        } else {
            None
        },
    };

    parser.skip_trivia();
    let block = parser.block(&[]);

    parser.end = source.len();
    parser.tree.root = parser.node(Kind::Root, 0, [block]);

    parser.tree
}

impl<const MARKUP: bool> Parser<'_, MARKUP> {
    fn current(&self) -> Token {
        self.tree.tokens[self.cursor]
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
        self.at(TokenKind::Name) && self.current().bytes(self.tree.source).as_bytes() == name
    }

    fn next(&self) -> TokenKind {
        if MARKUP {
            return self
                .lexer
                .as_ref()
                .expect("markup token stream")
                .clone()
                .find(|token| !trivia(token.kind))
                .map_or(TokenKind::Eof, |token| token.kind);
        }

        self.tree.tokens[self.cursor + usize::from(!self.at(TokenKind::Eof))..]
            .iter()
            .find(|token| !trivia(token.kind))
            .expect("token stream ends in EOF")
            .kind
    }

    fn skip_trivia(&mut self) {
        if !MARKUP {
            while trivia(self.current().kind) {
                self.cursor += 1;
            }

            return;
        }

        loop {
            if self.cursor == self.tree.tokens.len() {
                self.tree.tokens.push(
                    self.lexer
                        .as_mut()
                        .expect("lazy token stream")
                        .next()
                        .expect("token stream ends in EOF"),
                );
            }

            if !trivia(self.current().kind) {
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

    fn expect(&mut self, kind: TokenKind, message: &'static str) -> Result<(), Diagnostic> {
        if self.consume(kind) {
            Ok(())
        } else {
            Err(self.error(message))
        }
    }

    fn close(&mut self, keyword: Keyword) {
        if let Err(error) = self.expect(TokenKind::Keyword(keyword), "missing block terminator") {
            self.tree.diagnostics.push(error);
        }
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
        let index = self.tree.nodes.len();
        let begin = self.tree.children.len();
        self.tree.children.extend(children);
        let children = begin..self.tree.children.len();

        self.tree.nodes.push(Node {
            kind,
            span: Span {
                start,
                end: self.end.max(start).max(
                    self.tree.children[children.clone()]
                        .last()
                        .map_or(start, |child| self.tree.nodes[*child].span.end),
                ),
            },
            children,
        });

        index
    }

    fn prepend(&mut self, node: usize, child: usize) {
        let children = &mut self.tree.nodes[node].children;
        assert_eq!(children.end, self.tree.children.len());
        self.tree.children.insert(children.start, child);
        children.end += 1;
    }

    fn leaf(&mut self, kind: Kind) -> usize {
        let token = self.take();

        self.node(kind, token.span.start, [])
    }

    fn name(&mut self) -> Parsed {
        if self.at(TokenKind::Name) {
            Ok(self.leaf(Kind::Name))
        } else {
            Err(self.error("expected name"))
        }
    }

    fn nested(&mut self, parse: impl FnOnce(&mut Self) -> Parsed) -> Parsed {
        if self.depth >= 256 {
            return Err(self.error("syntax nesting limit exceeded"));
        }

        self.depth += 1;
        let result = parse(self);
        self.depth -= 1;

        result
    }

    fn block(&mut self, stops: &[Keyword]) -> usize {
        let start = self.current().span.start;
        let mut statements: Vec<usize> = Vec::new();

        while !self.at(TokenKind::Eof) && !stops.iter().any(|stop| self.keyword(*stop)) {
            if statements.last().is_some_and(|statement| {
                matches!(
                    self.tree.nodes[*statement].kind,
                    Kind::Return | Kind::Break | Kind::Continue
                )
            }) {
                self.tree
                    .diagnostics
                    .push(self.error("statement follows a block-ending statement"));
            }

            let cursor = self.cursor;
            let checkpoint = self.tree.nodes.len();
            let child_checkpoint = self.tree.children.len();
            let begin = self.current().span.start;

            match self.nested(Self::statement) {
                Ok(statement) => {
                    statements.push(statement);
                    self.consume(TokenKind::Byte(b';'));
                }

                Err(error) => {
                    self.tree.nodes.truncate(checkpoint);
                    self.tree.children.truncate(child_checkpoint);
                    self.tree.diagnostics.push(error);
                    self.recover(cursor, stops);
                    statements.push(self.node(Kind::Error, begin, []));
                }
            }
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

                let gap = &self.tree.source.as_bytes()[self.end..self.current().span.start];

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

    fn binding(&mut self) -> Parsed {
        let start = self.current().span.start;
        let name = self.name()?;

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
