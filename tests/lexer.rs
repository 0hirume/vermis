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
            TokenKind::Error(LexError::BrokenUnicode),
            TokenKind::Error(LexError::BrokenUnicode),
            TokenKind::Error(LexError::BrokenUnicode),
            TokenKind::Byte(b'!'),
            TokenKind::Error(LexError::BrokenUnicode),
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
