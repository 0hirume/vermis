use bstr::{BStr, ByteSlice};
use vermis::{InterpolatedKind, Keyword, LexError, Operator, Token, TokenKind, tokenize};

fn syntax(source: &BStr) -> Vec<TokenKind> {
    tokenize(source)
        .into_iter()
        .filter_map(|token| match token.kind {
            TokenKind::Eof | TokenKind::Whitespace => None,
            kind => Some(kind),
        })
        .collect()
}

#[test]
fn spans_partition_arbitrary_bytes() {
    let source = BStr::new(b"-- \xff\nlocal type = \"\xff\"\0tail");
    let tokens = tokenize(source);
    let mut reconstructed = Vec::new();
    let mut end = 0;

    for token in &tokens {
        if token.kind == TokenKind::Eof {
            assert_eq!(token.span.start, source.len());
            assert!(token.span.is_empty());
            continue;
        }

        assert_eq!(token.span.start, end);
        assert!(token.span.end > token.span.start);

        reconstructed.extend_from_slice(token.bytes(source).as_bytes());
        end = token.span.end;
    }

    assert_eq!(end, source.len());
    assert_eq!(reconstructed, source.as_bytes());
}

#[test]
fn recognizes_longest_operators() {
    let source = BStr::new(b"== <= >= ~= .. ... -> :: // += -= *= /= //= %= ^= ..=");

    assert_eq!(
        syntax(source),
        [
            TokenKind::Operator(Operator::Equal),
            TokenKind::Operator(Operator::LessEqual),
            TokenKind::Operator(Operator::GreaterEqual),
            TokenKind::Operator(Operator::NotEqual),
            TokenKind::Operator(Operator::Concat),
            TokenKind::Operator(Operator::Ellipsis),
            TokenKind::Operator(Operator::Arrow),
            TokenKind::Operator(Operator::DoubleColon),
            TokenKind::Operator(Operator::FloorDivide),
            TokenKind::Operator(Operator::AddAssign),
            TokenKind::Operator(Operator::SubtractAssign),
            TokenKind::Operator(Operator::MultiplyAssign),
            TokenKind::Operator(Operator::DivideAssign),
            TokenKind::Operator(Operator::FloorDivideAssign),
            TokenKind::Operator(Operator::ModuloAssign),
            TokenKind::Operator(Operator::PowerAssign),
            TokenKind::Operator(Operator::ConcatAssign),
        ]
    );
}

#[test]
fn recognizes_comments_and_strings() {
    let source = BStr::new(b"-- line\n--[=[block]=]\n[==[raw]==] 'quoted' \"double\"");

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
    let source = BStr::new(b"`plain` `a{x}b` `a{{bad}}b`");

    assert_eq!(
        syntax(source),
        [
            TokenKind::Interpolated(InterpolatedKind::Simple),
            TokenKind::Interpolated(InterpolatedKind::Begin),
            TokenKind::Name,
            TokenKind::Interpolated(InterpolatedKind::End),
            TokenKind::Error(LexError::BrokenInterpolatedDoubleBrace),
            TokenKind::Name,
            TokenKind::Interpolated(InterpolatedKind::End),
        ]
    );
}

#[test]
fn handles_unicode_without_requiring_utf8() {
    let source = BStr::new(b"\xff\xfe\xe2!\xe2\x98\x83 \"\xff\"");
    let tokens = tokenize(source);

    assert_eq!(
        syntax(source),
        [
            TokenKind::Error(LexError::BrokenUnicode { codepoint: 0 }),
            TokenKind::Error(LexError::BrokenUnicode { codepoint: 0 }),
            TokenKind::Error(LexError::BrokenUnicode { codepoint: 0 }),
            TokenKind::Byte(b'!'),
            TokenKind::Error(LexError::BrokenUnicode { codepoint: 0x2603 }),
            TokenKind::QuotedString,
        ]
    );

    assert!(tokens[0].utf8(source).is_err());
    assert_eq!(tokens[4].utf8(source), Ok("☃"));
}

#[test]
fn scans_names_and_number_like_sequences() {
    let source = BStr::new(b"local foo_1 = .5 0xABC 1e+2");

    assert_eq!(
        syntax(source),
        [
            TokenKind::Keyword(Keyword::Local),
            TokenKind::Name,
            TokenKind::Byte(b'='),
            TokenKind::Number,
            TokenKind::Number,
            TokenKind::Number,
        ]
    );
}

fn check(source: &[u8], expected: &[(TokenKind, &[u8])]) {
    let source = BStr::new(source);
    let mut lexer = vermis::Lexer::new(source);
    let mut offset = 0;

    for &(kind, bytes) in expected {
        let token = lexer.next().expect("expected token");

        assert_eq!(token.kind, kind);
        assert_eq!(token.span.start, offset);
        assert_eq!(token.span.end, offset + bytes.len());
        assert_eq!(token.bytes(source).as_bytes(), bytes);

        offset = token.span.end;
    }

    let eof = lexer.next().expect("expected eof");

    assert_eq!(offset, source.len());
    assert_eq!(eof.kind, TokenKind::Eof);
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
        &[
            (TokenKind::Error(LexError::BrokenString), b"[="),
            (TokenKind::Name, b"x"),
        ],
    );

    check(b"--[=x", &[(TokenKind::Comment, b"--[=x")]);
    check(
        b"--[[",
        &[(TokenKind::Error(LexError::BrokenComment), b"--[[")],
    );
    check(
        b"[=[x]]",
        &[(TokenKind::Error(LexError::BrokenString), b"[=[x]]")],
    );
    check(b"[=[x]]=]", &[(TokenKind::RawString, b"[=[x]]=]")]);
    check(
        b"[x]",
        &[
            (TokenKind::Byte(b'['), b"["),
            (TokenKind::Name, b"x"),
            (TokenKind::Byte(b']'), b"]"),
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
        check(
            source,
            &[(TokenKind::Error(LexError::BrokenString), source)],
        );
    }

    check(
        b"'x\ny",
        &[
            (TokenKind::Error(LexError::BrokenString), b"'x"),
            (TokenKind::Whitespace, b"\n"),
            (TokenKind::Name, b"y"),
        ],
    );
}

#[test]
fn interpolation_modes() {
    use InterpolatedKind::{Begin, End, Middle, Simple};
    use TokenKind::{Byte, Interpolated, Name};

    check(
        b"`{a}{b}`",
        &[
            (Interpolated(Begin), b"`{"),
            (Name, b"a"),
            (Interpolated(Middle), b"}{"),
            (Name, b"b"),
            (Interpolated(End), b"}`"),
        ],
    );

    check(
        b"`{ {x} }`",
        &[
            (Interpolated(Begin), b"`{"),
            (TokenKind::Whitespace, b" "),
            (Byte(b'{'), b"{"),
            (Name, b"x"),
            (Byte(b'}'), b"}"),
            (TokenKind::Whitespace, b" "),
            (Interpolated(End), b"}`"),
        ],
    );

    check(
        b"`{`{x}`}`",
        &[
            (Interpolated(Begin), b"`{"),
            (Interpolated(Begin), b"`{"),
            (Name, b"x"),
            (Interpolated(End), b"}`"),
            (Interpolated(End), b"}`"),
        ],
    );

    check(
        b"`\\u{2603}\\{x`",
        &[(Interpolated(Simple), b"`\\u{2603}\\{x`")],
    );
}

#[test]
fn broken_braces_have_separate_diagnostic_span() {
    let source = BStr::new(b"`{{x}`");
    let token = tokenize(source)[0];

    assert_eq!(
        token.kind,
        TokenKind::Error(LexError::BrokenInterpolatedDoubleBrace)
    );
    assert_eq!(token.bytes(source), BStr::new(b"`{{"));
    assert_eq!(token.diagnostic_span().bytes(source), BStr::new(b"`"));
}

#[test]
fn attributes() {
    check(
        b"@native@[x]@1",
        &[
            (TokenKind::Attribute, b"@native"),
            (TokenKind::AttributeOpen, b"@["),
            (TokenKind::Name, b"x"),
            (TokenKind::Byte(b']'), b"]"),
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
        check(
            source,
            &[(
                TokenKind::Error(LexError::BrokenUnicode { codepoint }),
                source,
            )],
        );
    }
}

#[test]
fn nul_ends_lexical_bodies_but_is_preserved() {
    for (prefix, kind) in [
        (b"'x".as_slice(), TokenKind::Error(LexError::BrokenString)),
        (b"[[x".as_slice(), TokenKind::Error(LexError::BrokenString)),
        (b"`x".as_slice(), TokenKind::Error(LexError::BrokenString)),
        (b"--x".as_slice(), TokenKind::Comment),
        (
            b"--[[x".as_slice(),
            TokenKind::Error(LexError::BrokenComment),
        ),
    ] {
        let mut source = prefix.to_vec();
        source.extend_from_slice(b"\0tail");

        check(
            &source,
            &[
                (kind, prefix),
                (TokenKind::Byte(0), b"\0"),
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
            let source = BStr::new(&bytes);
            let mut lexer = vermis::Lexer::new(source);
            let mut end = 0;

            for _ in 0..=source.len() {
                let token = lexer.next().expect("eof must be emitted");

                assert_eq!(token.span.start, end);
                assert!(token.span.end <= source.len());

                if token.kind == TokenKind::Eof {
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
    let source = BStr::new(b"a\0b");
    let tokens: Vec<Token> = tokenize(source);

    assert_eq!(
        tokens.iter().map(|token| token.kind).collect::<Vec<_>>(),
        [
            TokenKind::Name,
            TokenKind::Byte(0),
            TokenKind::Name,
            TokenKind::Eof,
        ]
    );
}
