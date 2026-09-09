use bstr::BStr;
use vermis::{Keyword, TokenKind, tokenize};

fn first(source: &[u8]) -> TokenKind {
    tokenize(BStr::new(source))[0].kind
}

#[test]
fn classifies_reserved_keywords() {
    let cases = [
        (b"and".as_slice(), Keyword::And),
        (b"break".as_slice(), Keyword::Break),
        (b"do".as_slice(), Keyword::Do),
        (b"else".as_slice(), Keyword::Else),
        (b"elseif".as_slice(), Keyword::ElseIf),
        (b"end".as_slice(), Keyword::End),
        (b"false".as_slice(), Keyword::False),
        (b"for".as_slice(), Keyword::For),
        (b"function".as_slice(), Keyword::Function),
        (b"if".as_slice(), Keyword::If),
        (b"in".as_slice(), Keyword::In),
        (b"local".as_slice(), Keyword::Local),
        (b"nil".as_slice(), Keyword::Nil),
        (b"not".as_slice(), Keyword::Not),
        (b"or".as_slice(), Keyword::Or),
        (b"repeat".as_slice(), Keyword::Repeat),
        (b"return".as_slice(), Keyword::Return),
        (b"then".as_slice(), Keyword::Then),
        (b"true".as_slice(), Keyword::True),
        (b"until".as_slice(), Keyword::Until),
        (b"while".as_slice(), Keyword::While),
    ];

    for (name, keyword) in cases {
        assert_eq!(first(name), TokenKind::Keyword(keyword));
        assert_eq!(first(&name.to_ascii_uppercase()), TokenKind::Name);

        for suffix in *b"x_0" {
            let mut extended = name.to_vec();
            extended.push(suffix);

            assert_eq!(first(&extended), TokenKind::Name);
        }
    }
}

#[test]
fn keeps_contextual_keywords_as_names() {
    for name in [
        b"continue".as_slice(),
        b"type".as_slice(),
        b"export".as_slice(),
        b"typeof".as_slice(),
        b"const".as_slice(),
        b"read".as_slice(),
        b"write".as_slice(),
        b"declare".as_slice(),
        b"extern".as_slice(),
        b"extends".as_slice(),
        b"with".as_slice(),
        b"class".as_slice(),
        b"open".as_slice(),
        b"public".as_slice(),
    ] {
        assert_eq!(first(name), TokenKind::Name);
    }
}
