mod expression;
mod statement;
mod types;

use crate::{
    Diagnostic, InterpolatedKind, Keyword, Kind, Node, Operator, Span, Token, TokenKind, Tree,
    tokenize,
};
use bstr::{BStr, ByteSlice};

type Parsed = Result<usize, Diagnostic>;

struct Parser<'source> {
    tree: Tree<'source>,
    cursor: usize,
    end: usize,
    depth: usize,
}

#[must_use]
pub fn parse(source: &BStr) -> Tree<'_> {
    let mut parser = Parser {
        tree: Tree {
            source,
            tokens: tokenize(source),
            nodes: Vec::new(),
            root: 0,
            diagnostics: Vec::new(),
        },
        cursor: 0,
        end: 0,
        depth: 0,
    };

    parser.skip_trivia();
    let block = parser.block(&[]);

    parser.end = source.len();
    parser.tree.root = parser.node(Kind::Root, 0, vec![block]);

    parser.tree
}

impl Parser<'_> {
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
        self.tree.tokens[self.cursor + usize::from(!self.at(TokenKind::Eof))..]
            .iter()
            .find(|token| !trivia(token.kind))
            .expect("token stream ends in EOF")
            .kind
    }

    fn skip_trivia(&mut self) {
        while trivia(self.current().kind) {
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

    fn node(&mut self, kind: Kind, start: usize, children: Vec<usize>) -> usize {
        let index = self.tree.nodes.len();

        self.tree.nodes.push(Node {
            kind,
            span: Span {
                start,
                end: self.end.max(start).max(
                    children
                        .last()
                        .map_or(start, |child| self.tree.nodes[*child].span.end),
                ),
            },
            children,
        });

        index
    }

    fn leaf(&mut self, kind: Kind) -> usize {
        let token = self.take();
        self.node(kind, token.span.start, Vec::new())
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
            if self.consume(TokenKind::Byte(b';')) {
                continue;
            }

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
            let begin = self.current().span.start;

            match self.nested(Self::statement) {
                Ok(statement) => statements.push(statement),
                Err(error) => {
                    self.tree.nodes.truncate(checkpoint);
                    self.tree.diagnostics.push(error);
                    self.recover(cursor, stops);
                    statements.push(self.node(Kind::Error, begin, Vec::new()));
                }
            }
        }

        self.node(Kind::Block, start, statements)
    }

    fn recover(&mut self, start: usize, stops: &[Keyword]) {
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
        let mut children = vec![self.name()?];

        if self.consume(TokenKind::Byte(b':')) {
            children.push(self.annotation()?);
        }

        Ok(self.node(Kind::Binding, start, children))
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
