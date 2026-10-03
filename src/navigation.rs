use std::{fmt, iter::FusedIterator, ops::Range, ptr};

use crate::{Children, Span, Token, TokenKind, Tree, View, tree::Syntax};

#[derive(Clone, Copy)]
pub struct TokenView<'tree> {
    tree: &'tree Tree,
    index: usize,
}

#[derive(Clone)]
pub struct Tokens<'tree> {
    tree: &'tree Tree,
    indices: Range<usize>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Element<'tree> {
    Node(View<'tree>),
    Token(TokenView<'tree>),
}

#[derive(Clone)]
pub struct Elements<'tree> {
    children: std::iter::Peekable<Children<'tree>>,
    tokens: Tokens<'tree>,
}

#[derive(Clone, Debug)]
pub struct Descendants<'tree> {
    tree: &'tree Tree,
    occurrences: crate::tree::Occurrences<'tree>,
}

impl Tree {
    #[must_use]
    pub fn root(&self) -> View<'_> {
        self.view(0)
    }

    #[must_use]
    pub fn tokens(&self) -> Tokens<'_> {
        Tokens {
            tree: self,
            indices: 0..self.tokens.len(),
        }
    }

    #[must_use]
    pub fn token_at(&self, offset: usize) -> Option<TokenView<'_>> {
        if offset > self.source.len() {
            return None;
        }

        let index = if offset == self.source.len() {
            self.tokens.len().checked_sub(1)?
        } else {
            self.tokens.select(offset, |measure| measure.width)?.0
        };

        Some(TokenView { tree: self, index })
    }

    #[must_use]
    pub fn node_at(&self, offset: usize) -> Option<View<'_>> {
        if offset > self.source.len() {
            return None;
        }

        if offset == self.source.len() {
            return Some(self.root());
        }

        self.covering(Span {
            start: offset,
            end: offset + 1,
        })
    }

    #[must_use]
    pub fn covering(&self, span: Span) -> Option<View<'_>> {
        if span.start > span.end || span.end > self.source.len() {
            return None;
        }

        if span.is_empty() {
            return self.node_at(span.start);
        }

        Some(self.node_covering(span))
    }

    fn node_covering(&self, span: Span) -> View<'_> {
        let mut syntax = &self.syntax;
        let mut ordinal = 0;
        let mut start = 0;

        loop {
            let Some((_, Syntax::Node(child), prefix)) = syntax
                .edges
                .select(span.start - start, |measure| measure.width)
            else {
                return self.view(ordinal);
            };

            let begin = start + prefix.width;

            if begin + child.width < span.end {
                return self.view(ordinal);
            }

            ordinal += 1 + prefix.nodes;
            start = begin;
            syntax = child;
        }
    }
}

impl<'tree> View<'tree> {
    #[must_use]
    pub fn parent(self) -> Option<Self> {
        self.node().parent.map(|index| self.tree.view(index))
    }

    #[must_use]
    pub fn previous_sibling(self) -> Option<Self> {
        let occurrence = self.node();
        let parent = self.tree.view(occurrence.parent?);

        parent.children().get(occurrence.position.checked_sub(1)?)
    }

    #[must_use]
    pub fn next_sibling(self) -> Option<Self> {
        let occurrence = self.node();
        let parent = self.tree.view(occurrence.parent?);

        parent.children().get(occurrence.position + 1)
    }

    pub fn ancestors(self) -> impl Iterator<Item = Self> + Clone {
        std::iter::successors(Some(self), |node| node.parent())
    }

    #[must_use]
    pub fn descendants(self) -> Descendants<'tree> {
        Descendants {
            tree: self.tree,
            occurrences: if self.index == 0 {
                self.tree.occurrences()
            } else {
                crate::tree::Occurrences::new(self.node())
            },
        }
    }

    #[must_use]
    pub fn tokens(self) -> Tokens<'tree> {
        let occurrence = self.node();

        Tokens {
            tree: self.tree,
            indices: occurrence.token_start
                ..occurrence.token_start + occurrence.syntax.tokens.len(),
        }
    }

    #[must_use]
    pub fn elements(self) -> Elements<'tree> {
        Elements {
            children: self.children().peekable(),
            tokens: self.tokens(),
        }
    }
}

impl<'tree> TokenView<'tree> {
    /// # Panics
    ///
    /// Panics if the snapshot's token storage is inconsistent.
    #[must_use]
    pub fn data(self) -> Token {
        self.tree.token(self.index).expect("token ordinal exists").0
    }

    /// # Panics
    ///
    /// Panics if the snapshot's token storage is inconsistent.
    #[must_use]
    pub fn kind(self) -> TokenKind {
        self.tree
            .tokens
            .get(self.index)
            .expect("token ordinal exists")
            .kind
    }

    #[must_use]
    pub fn span(self) -> Span {
        self.data().span
    }

    /// # Panics
    ///
    /// Panics if the snapshot's token storage is inconsistent.
    #[must_use]
    pub fn text(self) -> &'tree [u8] {
        self.tree
            .tokens
            .get(self.index)
            .expect("token ordinal exists")
            .source
            .bytes()
    }

    #[must_use]
    pub fn parent(self) -> View<'tree> {
        if self.kind() == TokenKind::Eof {
            self.tree.root()
        } else {
            self.tree.node_covering(self.span())
        }
    }

    #[must_use]
    pub fn previous(self) -> Option<Self> {
        self.index.checked_sub(1).map(|index| Self {
            tree: self.tree,
            index,
        })
    }

    #[must_use]
    pub fn next(self) -> Option<Self> {
        let index = self.index + 1;

        (index < self.tree.tokens.len()).then_some(Self {
            tree: self.tree,
            index,
        })
    }
}

impl PartialEq for View<'_> {
    fn eq(&self, other: &Self) -> bool {
        ptr::eq(self.tree, other.tree) && self.index == other.index
    }
}

impl Eq for View<'_> {}

impl PartialEq for TokenView<'_> {
    fn eq(&self, other: &Self) -> bool {
        ptr::eq(self.tree, other.tree) && self.index == other.index
    }
}

impl Eq for TokenView<'_> {}

impl fmt::Debug for TokenView<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("TokenView")
            .field("kind", &self.kind())
            .field("span", &self.span())
            .finish()
    }
}

impl fmt::Debug for Tokens<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_list().entries(self.clone()).finish()
    }
}

impl fmt::Debug for Elements<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_list().entries(self.clone()).finish()
    }
}

impl<'tree> Iterator for Tokens<'tree> {
    type Item = TokenView<'tree>;

    fn next(&mut self) -> Option<Self::Item> {
        self.indices.next().map(|index| TokenView {
            tree: self.tree,
            index,
        })
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.indices.size_hint()
    }
}

impl DoubleEndedIterator for Tokens<'_> {
    fn next_back(&mut self) -> Option<Self::Item> {
        self.indices.next_back().map(|index| TokenView {
            tree: self.tree,
            index,
        })
    }
}

impl ExactSizeIterator for Tokens<'_> {}
impl FusedIterator for Tokens<'_> {}

impl<'tree> Iterator for Elements<'tree> {
    type Item = Element<'tree>;

    fn next(&mut self) -> Option<Self::Item> {
        if let Some(child) = self.children.peek().copied()
            && self
                .tokens
                .clone()
                .next()
                .is_none_or(|token| child.span().start <= token.span().start)
        {
            self.children.next();
            let occurrence = child.node();
            self.tokens.indices.start = occurrence.token_start + occurrence.syntax.tokens.len();

            return Some(Element::Node(child));
        }

        self.tokens.next().map(Element::Token)
    }
}

impl FusedIterator for Elements<'_> {}

impl<'tree> Iterator for Descendants<'tree> {
    type Item = View<'tree>;

    fn next(&mut self) -> Option<Self::Item> {
        self.occurrences
            .next()
            .map(|occurrence| self.tree.view(occurrence.ordinal))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.occurrences.size_hint()
    }
}

impl FusedIterator for Descendants<'_> {}
