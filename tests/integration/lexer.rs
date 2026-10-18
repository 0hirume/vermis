//! Lexer tests.

use vermis::lexer::Lexer;
use vermis::token::{Keyword, Symbol, TokenKind};

use crate::support::check;

#[test]
fn spans_partition_arbitrary_bytes() {
    let source = include_bytes!("../fixtures/lexer/partition.luau");
    let tokens = check(source).tokens;
    let mut reconstructed = Vec::new();
    let mut end = 0;

    for token in &tokens {
        if token.kind == TokenKind::EndOfFile {
            assert_eq!(token.span.start, source.len());
            assert!(token.span.is_empty());
            continue;
        }

        assert_eq!(token.span.start, end);
        assert!(token.span.end > token.span.start);

        reconstructed.extend_from_slice(token.bytes(source));
        end = token.span.end;
    }

    assert_eq!(end, source.len());
    assert_eq!(reconstructed, source);
}

#[test]
fn recognizes_longest_operators() {
    let source = include_bytes!("../fixtures/lexer/operators.luau");

    assert_eq!(
        syntax(source),
        [
            TokenKind::Symbol(Symbol::Equal),
            TokenKind::Symbol(Symbol::LessThanOrEqual),
            TokenKind::Symbol(Symbol::GreaterThanOrEqual),
            TokenKind::Symbol(Symbol::NotEqual),
            TokenKind::Symbol(Symbol::Concatenate),
            TokenKind::Symbol(Symbol::Ellipsis),
            TokenKind::Symbol(Symbol::Arrow),
            TokenKind::Symbol(Symbol::DoubleColon),
            TokenKind::Symbol(Symbol::FloorDivide),
            TokenKind::Symbol(Symbol::AddAssignment),
            TokenKind::Symbol(Symbol::SubtractAssignment),
            TokenKind::Symbol(Symbol::MultiplyAssignment),
            TokenKind::Symbol(Symbol::DivideAssignment),
            TokenKind::Symbol(Symbol::FloorDivideAssignment),
            TokenKind::Symbol(Symbol::ModuloAssignment),
            TokenKind::Symbol(Symbol::PowerAssignment),
            TokenKind::Symbol(Symbol::ConcatenateAssignment),
        ]
    );
}

#[test]
fn recognizes_comments_and_strings() {
    let source = include_bytes!("../fixtures/lexer/comments.luau");

    assert_eq!(
        syntax(source),
        [
            TokenKind::Comment,
            TokenKind::BlockComment,
            TokenKind::RawString,
            TokenKind::QuotedString,
            TokenKind::QuotedString,
        ]
    );
}

#[test]
fn tracks_interpolated_braces() {
    let source = include_bytes!("../fixtures/lexer/braces.luau");

    assert_eq!(
        syntax(source),
        [
            TokenKind::InterpolatedStringSimple,
            TokenKind::InterpolatedStringStart,
            TokenKind::Name,
            TokenKind::InterpolatedStringEnd,
            TokenKind::InvalidInterpolationDoubleBrace,
            TokenKind::Name,
            TokenKind::InterpolatedStringEnd,
        ]
    );
}

#[test]
fn handles_unicode_without_requiring_utf8() {
    let source = include_bytes!("../fixtures/lexer/unicode/mixed.luau");
    let tokens = check(source).tokens;

    assert_eq!(
        syntax(source),
        [
            TokenKind::InvalidUnicode { codepoint: 0 },
            TokenKind::InvalidUnicode { codepoint: 0 },
            TokenKind::InvalidUnicode { codepoint: 0 },
            TokenKind::InvalidCharacter { byte: b'!' },
            TokenKind::InvalidUnicode { codepoint: 0x2603 },
            TokenKind::QuotedString,
        ]
    );

    let Err(_) = tokens[0].utf8(source) else {
        panic!("expected invalid UTF-8 error");
    };

    assert_eq!(tokens[4].utf8(source), Ok("☃"));
}

#[test]
fn scans_names_and_number_like_sequences() {
    let source = include_bytes!("../fixtures/lexer/names.luau");

    assert_eq!(
        syntax(source),
        [
            TokenKind::Keyword(Keyword::Local),
            TokenKind::Name,
            TokenKind::Symbol(Symbol::Assignment),
            TokenKind::Number,
            TokenKind::Number,
            TokenKind::Number,
        ]
    );
}

#[test]
fn empty_and_whitespace() {
    assert_tokens(include_bytes!("../fixtures/lexer/empty.luau"), &[]);

    assert_tokens(
        include_bytes!("../fixtures/lexer/whitespace.luau"),
        &[(TokenKind::Whitespace, b" \t\r\n\x0b\x0c")],
    );
}

#[test]
fn malformed_delimiters() {
    assert_tokens(
        include_bytes!("../fixtures/lexer/delimiters/opening.luau"),
        &[(TokenKind::MalformedString, b"[="), (TokenKind::Name, b"x")],
    );

    assert_tokens(
        include_bytes!("../fixtures/lexer/delimiters/comment.luau"),
        &[(TokenKind::Comment, b"--[=x")],
    );

    assert_tokens(
        include_bytes!("../fixtures/lexer/delimiters/unfinished.luau"),
        &[(TokenKind::MalformedComment, b"--[[")],
    );

    assert_tokens(
        include_bytes!("../fixtures/lexer/delimiters/mismatch.luau"),
        &[(TokenKind::MalformedString, b"[=[x]]")],
    );

    assert_tokens(
        include_bytes!("../fixtures/lexer/delimiters/closing.luau"),
        &[(TokenKind::RawString, b"[=[x]]=]")],
    );

    assert_tokens(
        include_bytes!("../fixtures/lexer/delimiters/index.luau"),
        &[
            (TokenKind::Symbol(Symbol::LeftBracket), b"["),
            (TokenKind::Name, b"x"),
            (TokenKind::Symbol(Symbol::RightBracket), b"]"),
        ],
    );
}

#[test]
fn escapes_and_unfinished_strings() {
    for source in [
        include_bytes!("../fixtures/lexer/strings/whitespace.luau").as_slice(),
        include_bytes!("../fixtures/lexer/strings/carriage.luau"),
        include_bytes!("../fixtures/lexer/strings/line.luau"),
        include_bytes!("../fixtures/lexer/strings/hexadecimal.luau"),
        include_bytes!("../fixtures/lexer/strings/unicode.luau"),
        include_bytes!("../fixtures/lexer/strings/decimal.luau"),
        include_bytes!("../fixtures/lexer/strings/double.luau"),
        include_bytes!("../fixtures/lexer/strings/single.luau"),
    ] {
        assert_tokens(source, &[(TokenKind::QuotedString, source)]);
    }

    for source in [
        include_bytes!("../fixtures/lexer/strings/empty.luau").as_slice(),
        include_bytes!("../fixtures/lexer/strings/escape.luau"),
        include_bytes!("../fixtures/lexer/strings/interpolation.luau"),
        include_bytes!("../fixtures/lexer/strings/raw.luau"),
    ] {
        assert_tokens(source, &[(TokenKind::MalformedString, source)]);
    }

    assert_tokens(
        include_bytes!("../fixtures/lexer/strings/newline.luau"),
        &[
            (TokenKind::MalformedString, b"'x"),
            (TokenKind::Whitespace, b"\n"),
            (TokenKind::Name, b"y"),
        ],
    );
}

#[test]
fn interpolation_modes() {
    assert_tokens(
        include_bytes!("../fixtures/lexer/interpolation/adjacent.luau"),
        &[
            (TokenKind::InterpolatedStringStart, b"`{"),
            (TokenKind::Name, b"a"),
            (TokenKind::InterpolatedStringMiddle, b"}{"),
            (TokenKind::Name, b"b"),
            (TokenKind::InterpolatedStringEnd, b"}`"),
        ],
    );

    assert_tokens(
        include_bytes!("../fixtures/lexer/interpolation/table.luau"),
        &[
            (TokenKind::InterpolatedStringStart, b"`{"),
            (TokenKind::Whitespace, b" "),
            (TokenKind::Symbol(Symbol::LeftBrace), b"{"),
            (TokenKind::Name, b"x"),
            (TokenKind::Symbol(Symbol::RightBrace), b"}"),
            (TokenKind::Whitespace, b" "),
            (TokenKind::InterpolatedStringEnd, b"}`"),
        ],
    );

    assert_tokens(
        include_bytes!("../fixtures/lexer/interpolation/nested.luau"),
        &[
            (TokenKind::InterpolatedStringStart, b"`{"),
            (TokenKind::InterpolatedStringStart, b"`{"),
            (TokenKind::Name, b"x"),
            (TokenKind::InterpolatedStringEnd, b"}`"),
            (TokenKind::InterpolatedStringEnd, b"}`"),
        ],
    );

    assert_tokens(
        include_bytes!("../fixtures/lexer/interpolation/escapes.luau"),
        &[(TokenKind::InterpolatedStringSimple, b"`\\u{2603}\\{x`")],
    );
}

#[test]
fn broken_braces_have_separate_diagnostic_span() {
    let source = include_bytes!("../fixtures/lexer/interpolation/broken.luau");
    let token = check(source).tokens[0];
    assert_eq!(token.kind, TokenKind::InvalidInterpolationDoubleBrace);

    assert_eq!(token.bytes(source), b"`{{");
    assert_eq!(token.diagnostic_span().bytes(source), b"`");
}

#[test]
fn attributes() {
    assert_tokens(
        include_bytes!("../fixtures/lexer/attributes.luau"),
        &[
            (TokenKind::Attribute, b"@native"),
            (TokenKind::Symbol(Symbol::AttributeOpen), b"@["),
            (TokenKind::Name, b"x"),
            (TokenKind::Symbol(Symbol::RightBracket), b"]"),
            (TokenKind::Attribute, b"@"),
            (TokenKind::Number, b"1"),
        ],
    );
}

#[test]
fn upstream_unicode_decoding_is_not_scalar_validation() {
    for (source, codepoint) in [
        (
            include_bytes!("../fixtures/lexer/unicode/zero.luau").as_slice(),
            0,
        ),
        (
            include_bytes!("../fixtures/lexer/unicode/control.luau").as_slice(),
            0x7f,
        ),
        (
            include_bytes!("../fixtures/lexer/unicode/surrogate.luau").as_slice(),
            0xd800,
        ),
        (
            include_bytes!("../fixtures/lexer/unicode/overflow.luau").as_slice(),
            0x0011_0000,
        ),
        (
            include_bytes!("../fixtures/lexer/unicode/maximum.luau").as_slice(),
            0x001f_ffff,
        ),
        (
            include_bytes!("../fixtures/lexer/unicode/emoji.luau").as_slice(),
            0x1f600,
        ),
        (
            include_bytes!("../fixtures/lexer/unicode/truncated.luau").as_slice(),
            0,
        ),
    ] {
        assert_tokens(source, &[(TokenKind::InvalidUnicode { codepoint }, source)]);
    }
}

#[test]
fn nul_ends_lexical_bodies_but_is_preserved() {
    for (source, prefix, kind) in [
        (
            include_bytes!("../fixtures/lexer/null/quoted.luau").as_slice(),
            b"'x".as_slice(),
            TokenKind::MalformedString,
        ),
        (
            include_bytes!("../fixtures/lexer/null/raw.luau").as_slice(),
            b"[[x".as_slice(),
            TokenKind::MalformedString,
        ),
        (
            include_bytes!("../fixtures/lexer/null/interpolation.luau").as_slice(),
            b"`x".as_slice(),
            TokenKind::MalformedString,
        ),
        (
            include_bytes!("../fixtures/lexer/null/comment.luau").as_slice(),
            b"--x".as_slice(),
            TokenKind::Comment,
        ),
        (
            include_bytes!("../fixtures/lexer/null/block.luau").as_slice(),
            b"--[[x".as_slice(),
            TokenKind::MalformedComment,
        ),
    ] {
        assert_tokens(
            source,
            &[
                (kind, prefix),
                (TokenKind::InvalidCharacter { byte: 0 }, b"\0"),
                (TokenKind::Name, b"tail"),
            ],
        );
    }
}

#[test]
fn every_byte_pair_terminates_and_round_trips() {
    for first in u8::MIN..=u8::MAX {
        for second in u8::MIN..=u8::MAX {
            let source = [first, second];
            let mut lexer = Lexer::new(&source);
            let mut end = 0;

            for _ in 0..=source.len() {
                let token = lexer.next().expect("expected end-of-file token");
                assert_eq!(token.span.start, end);
                assert!(token.span.end <= source.len());

                if token.kind == TokenKind::EndOfFile {
                    assert_eq!(end, source.len());
                    assert!(token.span.is_empty());
                    assert_eq!(lexer.next(), None);
                    break;
                }

                assert!(!token.span.is_empty());
                end = token.span.end;
            }

            assert_eq!(lexer.next(), None);
        }
    }
}

#[test]
fn preserves_embedded_nul() {
    let source = include_bytes!("../fixtures/lexer/null/embedded.luau");
    let tokens = check(source).tokens;

    let kinds: Vec<_> = tokens.iter().map(|token| token.kind).collect();

    assert_eq!(
        kinds,
        [
            TokenKind::Name,
            TokenKind::InvalidCharacter { byte: 0 },
            TokenKind::Name,
            TokenKind::EndOfFile,
        ]
    );
}

fn syntax(source: &[u8]) -> Vec<TokenKind> {
    check(source)
        .tokens
        .into_iter()
        .filter_map(|token| match token.kind {
            TokenKind::EndOfFile | TokenKind::Whitespace => None,
            kind => Some(kind),
        })
        .collect()
}

fn assert_tokens(source: &[u8], expected: &[(TokenKind, &[u8])]) {
    let tree = check(source);
    let mut lexer = Lexer::new(source);
    let actual: Vec<_> = lexer.by_ref().collect();
    assert_eq!(actual, tree.tokens);

    let mut tokens = tree.tokens.into_iter();
    let mut offset = 0;

    for (kind, bytes) in expected {
        let token = tokens.next().expect("expected token");
        assert_eq!(token.kind, *kind);
        assert_eq!(token.span.start, offset);
        assert_eq!(token.span.end, offset + bytes.len());
        assert_eq!(token.bytes(source), *bytes);

        offset = token.span.end;
    }

    let end_of_file = tokens.next().expect("expected end-of-file token");
    assert_eq!(offset, source.len());
    assert_eq!(end_of_file.kind, TokenKind::EndOfFile);
    assert_eq!(end_of_file.span.start, offset);
    assert_eq!(end_of_file.span.end, offset);
    assert_eq!(tokens.next(), None);
    assert_eq!(lexer.next(), None);
    assert_eq!(lexer.next(), None);
}
