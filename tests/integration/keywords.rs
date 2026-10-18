//! Keyword classification tests.

use vermis::token::{Keyword, TokenKind};

use crate::support::check;

#[test]
fn classifies_reserved_keywords() {
    let names: Vec<_> = include_bytes!("../fixtures/keywords/reserved.luau")
        .split(u8::is_ascii_whitespace)
        .filter(|name| !name.is_empty())
        .collect();

    let keywords = [
        Keyword::And,
        Keyword::Break,
        Keyword::Do,
        Keyword::Else,
        Keyword::ElseIf,
        Keyword::End,
        Keyword::False,
        Keyword::For,
        Keyword::Function,
        Keyword::If,
        Keyword::In,
        Keyword::Local,
        Keyword::Nil,
        Keyword::Not,
        Keyword::Or,
        Keyword::Repeat,
        Keyword::Return,
        Keyword::Then,
        Keyword::True,
        Keyword::Until,
        Keyword::While,
    ];

    assert_eq!(names.len(), keywords.len());

    for (name, keyword) in names.into_iter().zip(keywords) {
        assert_eq!(first_kind(name), TokenKind::Keyword(keyword));
        assert_eq!(first_kind(&name.to_ascii_uppercase()), TokenKind::Name);

        for suffix in *b"x_0" {
            let mut extended = name.to_vec();
            extended.push(suffix);

            assert_eq!(first_kind(&extended), TokenKind::Name);
        }
    }
}

#[test]
fn keeps_contextual_keywords_as_names() {
    let names: Vec<_> = include_bytes!("../fixtures/keywords/contextual.luau")
        .split(u8::is_ascii_whitespace)
        .filter(|name| !name.is_empty())
        .collect();

    assert_eq!(names.len(), 14);

    for name in names {
        assert_eq!(first_kind(name), TokenKind::Name);
    }
}

fn first_kind(source: &[u8]) -> TokenKind {
    check(source).tokens[0].kind
}
