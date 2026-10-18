//! Type parser depth tests.

use std::thread;

use vermis::parser::Parser;
use vermis::token::TokenKind;

use crate::support::check;

#[test]
fn deeply_nested_types_report_limit() {
    thread::Builder::new()
        .stack_size(1024 * 1024)
        .spawn(|| {
            for annotation in [
                format!("{}number{}", "{field: ".repeat(1000), "}".repeat(1000)),
                format!("{}number{}", "(".repeat(1000), ")".repeat(1000)),
                format!("{}number{}", "Box<".repeat(1000), ">".repeat(1000)),
                format!("{}number", "() -> ".repeat(1000)),
            ] {
                let source = format!("type Value = {annotation}");
                let tree = check(source.as_bytes());

                assert!(
                    tree.diagnostics.iter().any(|diagnostic| {
                        diagnostic.message == "syntax nesting limit exceeded"
                    })
                );

                assert_eq!(tree.text(tree.root), source.as_bytes());

                let mut parser = Parser::new(annotation.as_bytes());
                parser.annotation();

                while !parser.at(TokenKind::EndOfFile) {
                    let position = parser.position();
                    parser.recover(position, &[]);
                    assert!(parser.position().get() > position.get());
                }
            }
        })
        .expect("expected type parser thread to spawn")
        .join()
        .expect("expected type parser thread to complete");
}
