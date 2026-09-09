#![no_main]

use bstr::{BStr, ByteSlice};
use libfuzzer_sys::fuzz_target;
use vermis::{Lexer, TokenKind};

fuzz_target!(|data: &[u8]| {
    let source = BStr::new(data);
    let mut lexer = Lexer::new(source);
    let mut reconstructed = Vec::new();
    let mut end = 0;

    loop {
        let token = lexer.next().expect("lexer must emit EOF");

        assert_eq!(token.span.start, end);
        assert!(token.span.start <= token.span.end);
        assert!(token.span.end <= data.len());

        let diagnostic = token.diagnostic_span();

        assert!(diagnostic.start >= token.span.start);
        assert!(diagnostic.start <= diagnostic.end);
        assert!(diagnostic.end <= token.span.end);

        let bytes = token.bytes(source).as_bytes();

        assert_eq!(token.utf8(source), std::str::from_utf8(bytes));

        if token.kind == TokenKind::Eof {
            assert!(token.span.is_empty());
            assert_eq!(end, data.len());
            break;
        }

        assert!(!token.span.is_empty());

        reconstructed.extend_from_slice(bytes);
        end = token.span.end;
    }

    assert_eq!(reconstructed, data);
    assert_eq!(lexer.next(), None);
    assert_eq!(lexer.next(), None);
});
