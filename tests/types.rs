//! Type parser depth tests.

use vermis::parser::Parser;
use vermis::token::TokenKind;
use vermis::tree::{ListEntry, NodeKind};

#[test]
fn deeply_nested_types_report_limit() {
    std::thread::Builder::new()
        .stack_size(1024 * 1024)
        .spawn(|| {
            for source in [
                format!("{}number{}", "{field: ".repeat(1000), "}".repeat(1000)),
                format!("{}number{}", "(".repeat(1000), ")".repeat(1000)),
                format!("{}number{}", "Box<".repeat(1000), ">".repeat(1000)),
                format!("{}number", "() -> ".repeat(1000)),
            ] {
                let mut parser = Parser::new(source.as_bytes());
                let start = parser.position();
                let node = parser.annotation();

                let mut entries = vec![ListEntry {
                    node,
                    separator: None,
                }];

                while !parser.at(TokenKind::EndOfFile) {
                    let position = parser.position();
                    let node = parser.recover(position, &[]);
                    assert!(parser.position().0 > position.0);

                    entries.push(ListEntry {
                        node,
                        separator: None,
                    });
                }

                let statements = parser.append_list(entries);
                let block = parser.append_node(start, NodeKind::Block { statements });
                let tree = parser.finish(block);

                assert!(
                    tree.diagnostics.iter().any(|diagnostic| {
                        diagnostic.message == "syntax nesting limit exceeded"
                    })
                );

                assert_eq!(tree.text(tree.root), source.as_bytes());
            }
        })
        .unwrap()
        .join()
        .unwrap();
}
