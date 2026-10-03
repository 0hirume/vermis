//! Lexer tests.

use vermis::{
    lexer::Lexer,
    token::{Keyword, Symbol, Token, TokenKind},
};

fn syntax(source: &[u8]) -> Vec<TokenKind> {
    Lexer::new(source)
        .collect::<Vec<_>>()
        .into_iter()
        .filter_map(|token| match token.kind {
            TokenKind::EndOfFile | TokenKind::Whitespace => None,
            kind => Some(kind),
        })
        .collect()
}

#[test]
fn spans_partition_arbitrary_bytes() {
    let source = b"-- \xff\nlocal type = \"\xff\"\0tail";
    let tokens = Lexer::new(source).collect::<Vec<_>>();
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
    let source = b"== <= >= ~= .. ... -> :: // += -= *= /= //= %= ^= ..=";

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
    let source = b"-- line\n--[=[block]=]\n[==[raw]==] 'quoted' \"double\"";

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
    let source = b"`plain` `a{x}b` `a{{bad}}b`";

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
    let source = b"\xff\xfe\xe2!\xe2\x98\x83 \"\xff\"";
    let tokens = Lexer::new(source).collect::<Vec<_>>();

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

    assert!(tokens[0].utf8(source).is_err());
    assert_eq!(tokens[4].utf8(source), Ok("☃"));
}

#[test]
fn scans_names_and_number_like_sequences() {
    let source = b"local foo_1 = .5 0xABC 1e+2";

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

fn check(source: &[u8], expected: &[(TokenKind, &[u8])]) {
    let mut lexer = Lexer::new(source);
    let mut offset = 0;

    for &(kind, bytes) in expected {
        let token = lexer.next().expect("expected token");

        assert_eq!(token.kind, kind);
        assert_eq!(token.span.start, offset);
        assert_eq!(token.span.end, offset + bytes.len());
        assert_eq!(token.bytes(source), bytes);

        offset = token.span.end;
    }

    let eof = lexer.next().expect("expected eof");

    assert_eq!(offset, source.len());
    assert_eq!(eof.kind, TokenKind::EndOfFile);
    assert_eq!(eof.span.start, offset);
    assert_eq!(eof.span.end, offset);
    assert_eq!(lexer.next(), None);
    assert_eq!(lexer.next(), None);
}

#[test]
fn empty_and_whitespace() {
    check(b"", &[]);

    check(
        b" \t\r\n\x0b\x0c",
        &[(TokenKind::Whitespace, b" \t\r\n\x0b\x0c")],
    );
}

#[test]
fn malformed_delimiters() {
    check(
        b"[=x",
        &[(TokenKind::MalformedString, b"[="), (TokenKind::Name, b"x")],
    );

    check(b"--[=x", &[(TokenKind::Comment, b"--[=x")]);

    check(b"--[[", &[(TokenKind::MalformedComment, b"--[[")]);

    check(b"[=[x]]", &[(TokenKind::MalformedString, b"[=[x]]")]);

    check(b"[=[x]]=]", &[(TokenKind::RawString, b"[=[x]]=]")]);

    check(
        b"[x]",
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
        b"'\\z \t\r\n\x0b\x0cx'".as_slice(),
        b"'\\\r\nx'",
        b"'\\\nx'",
        b"'\\xQQ'",
        b"'\\u{no}'",
        b"'\\999'",
        b"'\\\"'",
        b"'\\\''",
    ] {
        check(source, &[(TokenKind::QuotedString, source)]);
    }

    for source in [b"'".as_slice(), b"'x\\", b"`x", b"[[x"] {
        check(source, &[(TokenKind::MalformedString, source)]);
    }

    check(
        b"'x\ny",
        &[
            (TokenKind::MalformedString, b"'x"),
            (TokenKind::Whitespace, b"\n"),
            (TokenKind::Name, b"y"),
        ],
    );
}

#[test]
fn interpolation_modes() {
    check(
        b"`{a}{b}`",
        &[
            (TokenKind::InterpolatedStringStart, b"`{"),
            (TokenKind::Name, b"a"),
            (TokenKind::InterpolatedStringMiddle, b"}{"),
            (TokenKind::Name, b"b"),
            (TokenKind::InterpolatedStringEnd, b"}`"),
        ],
    );

    check(
        b"`{ {x} }`",
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

    check(
        b"`{`{x}`}`",
        &[
            (TokenKind::InterpolatedStringStart, b"`{"),
            (TokenKind::InterpolatedStringStart, b"`{"),
            (TokenKind::Name, b"x"),
            (TokenKind::InterpolatedStringEnd, b"}`"),
            (TokenKind::InterpolatedStringEnd, b"}`"),
        ],
    );

    check(
        b"`\\u{2603}\\{x`",
        &[(TokenKind::InterpolatedStringSimple, b"`\\u{2603}\\{x`")],
    );
}

#[test]
fn broken_braces_have_separate_diagnostic_span() {
    let source = b"`{{x}`";
    let token = Lexer::new(source).collect::<Vec<_>>()[0];

    assert_eq!(token.kind, TokenKind::InvalidInterpolationDoubleBrace);

    assert_eq!(token.bytes(source), b"`{{");
    assert_eq!(token.diagnostic_span().bytes(source), b"`");
}

#[test]
fn attributes() {
    check(
        b"@native@[x]@1",
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
        (b"\xc0\x80".as_slice(), 0),
        (b"\xc1\xbf".as_slice(), 0x7f),
        (b"\xed\xa0\x80".as_slice(), 0xd800),
        (b"\xf4\x90\x80\x80".as_slice(), 0x0011_0000),
        (b"\xf7\xbf\xbf\xbf".as_slice(), 0x001f_ffff),
        (b"\xf0\x9f\x98\x80".as_slice(), 0x1f600),
        (b"\xe2\x98".as_slice(), 0),
    ] {
        check(source, &[(TokenKind::InvalidUnicode { codepoint }, source)]);
    }
}

#[test]
fn nul_ends_lexical_bodies_but_is_preserved() {
    for (prefix, kind) in [
        (b"'x".as_slice(), TokenKind::MalformedString),
        (b"[[x".as_slice(), TokenKind::MalformedString),
        (b"`x".as_slice(), TokenKind::MalformedString),
        (b"--x".as_slice(), TokenKind::Comment),
        (b"--[[x".as_slice(), TokenKind::MalformedComment),
    ] {
        let mut source = prefix.to_vec();
        source.extend_from_slice(b"\0tail");

        check(
            &source,
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
            let bytes = [first, second];
            let source = &bytes;
            let mut lexer = Lexer::new(source);
            let mut end = 0;

            for _ in 0..=source.len() {
                let token = lexer.next().expect("eof must be emitted");

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
    let source = b"a\0b";
    let tokens: Vec<Token> = Lexer::new(source).collect::<Vec<_>>();

    assert_eq!(
        tokens.iter().map(|token| token.kind).collect::<Vec<_>>(),
        [
            TokenKind::Name,
            TokenKind::InvalidCharacter { byte: 0 },
            TokenKind::Name,
            TokenKind::EndOfFile,
        ]
    );
}
