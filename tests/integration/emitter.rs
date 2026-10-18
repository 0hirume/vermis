//! Emitter regression tests.

use std::mem::{Discriminant, discriminant};

use vermis::emitter::{Emitter, LineEnding};
use vermis::lexer::Lexer;
use vermis::token::TokenKind;
use vermis::tree::{NodeIndex, NodeKind, TokenIndex, Tree};

use crate::support::check;

#[test]
fn generated_symbols_follow_interpolated_strings() {
    for source in [
        include_bytes!("../fixtures/emitter/interpolation/arguments.luau").as_slice(),
        include_bytes!("../fixtures/emitter/interpolation/call.luau").as_slice(),
        include_bytes!("../fixtures/emitter/interpolation/group.luau").as_slice(),
        include_bytes!("../fixtures/emitter/interpolation/index.luau").as_slice(),
        include_bytes!("../fixtures/emitter/interpolation/table.luau").as_slice(),
        include_bytes!("../fixtures/emitter/interpolation/text.luau").as_slice(),
        include_bytes!("../fixtures/emitter/interpolation/concatenation.luau").as_slice(),
        include_bytes!("../fixtures/emitter/interpolation/assertion.luau").as_slice(),
        include_bytes!("../fixtures/emitter/interpolation/nested.luau").as_slice(),
        include_bytes!("../fixtures/emitter/interpolation/plain.luau").as_slice(),
    ] {
        let output = emit(source, true);
        assert_eq!(output, source);
    }
}

#[test]
fn generated_symbols_follow_interpolation_middles() {
    for source in [
        include_bytes!("../fixtures/emitter/interpolation/parentheses.luau").as_slice(),
        include_bytes!("../fixtures/emitter/interpolation/negation.luau").as_slice(),
        include_bytes!("../fixtures/emitter/interpolation/minus.luau").as_slice(),
        include_bytes!("../fixtures/emitter/interpolation/length.luau").as_slice(),
    ] {
        let output = emit(source, true);
        assert_eq!(output, source);
    }
}

#[test]
fn token_separation_is_preserved() {
    for (source, expected) in [
        (
            include_bytes!("../fixtures/emitter/separation/local/source.luau").as_slice(),
            include_bytes!("../fixtures/emitter/separation/local/expected.luau").as_slice(),
        ),
        (
            include_bytes!("../fixtures/emitter/separation/conjunction/source.luau").as_slice(),
            include_bytes!("../fixtures/emitter/separation/conjunction/source.luau").as_slice(),
        ),
        (
            include_bytes!("../fixtures/emitter/separation/number/source.luau").as_slice(),
            include_bytes!("../fixtures/emitter/separation/number/source.luau").as_slice(),
        ),
        (
            include_bytes!("../fixtures/emitter/separation/subtraction/source.luau").as_slice(),
            include_bytes!("../fixtures/emitter/separation/subtraction/expected.luau").as_slice(),
        ),
        (
            include_bytes!("../fixtures/emitter/separation/index/source.luau").as_slice(),
            include_bytes!("../fixtures/emitter/separation/index/expected.luau").as_slice(),
        ),
        (
            include_bytes!("../fixtures/emitter/separation/table/source.luau").as_slice(),
            include_bytes!("../fixtures/emitter/separation/table/expected.luau").as_slice(),
        ),
        (
            include_bytes!("../fixtures/emitter/separation/interpolation/source.luau").as_slice(),
            include_bytes!("../fixtures/emitter/separation/interpolation/expected.luau").as_slice(),
        ),
        (
            include_bytes!("../fixtures/emitter/separation/comment/source.luau").as_slice(),
            include_bytes!("../fixtures/emitter/separation/comment/expected.luau").as_slice(),
        ),
    ] {
        let output = emit(source, false);
        assert_eq!(output, expected);
    }
}

#[test]
fn programs_preserve_syntax_through_emission() {
    let source = include_bytes!("../fixtures/emitter/program.luau");
    assert_eq!(emit(source, true), source);
    assert!(emit(source, false).len() < source.len());
}

fn emit(source: &[u8], preserve_whitespace: bool) -> Vec<u8> {
    let tree = check(source);

    assert!(
        tree.diagnostics.is_empty(),
        "{:?}: {:?}",
        tree.source,
        tree.diagnostics
    );

    let mut emitter = Emitter::new(LineEnding::LineFeed);

    for (index, token) in tree.tokens.iter().enumerate() {
        match token.kind {
            TokenKind::Keyword(keyword) => {
                emitter.keyword(keyword);
            }

            TokenKind::Symbol(symbol) => {
                emitter.symbol(symbol);
            }

            TokenKind::Whitespace if !preserve_whitespace => {}

            _ => {
                emitter.token(&tree, TokenIndex::new(index));
            }
        }
    }

    let output = emitter.finish().into_owned();
    let reparsed = check(&output);

    assert!(
        reparsed.diagnostics.is_empty(),
        "{:?}: {:?}",
        reparsed.source,
        reparsed.diagnostics
    );

    assert_eq!(tokens(&output), tokens(source));
    assert_eq!(structure(&reparsed), structure(&tree));

    output
}

fn tokens(source: &[u8]) -> Vec<(TokenKind, Vec<u8>)> {
    Lexer::new(source)
        .filter(|token| token.kind != TokenKind::Whitespace)
        .map(|token| (token.kind, token.bytes(source).to_vec()))
        .collect()
}

fn structure(tree: &Tree<'_>) -> Vec<(Discriminant<NodeKind>, Vec<NodeIndex>)> {
    tree.nodes
        .iter()
        .enumerate()
        .map(|(index, node)| {
            (
                discriminant(&node.kind),
                tree.children(NodeIndex::new(index)),
            )
        })
        .collect()
}
