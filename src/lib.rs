mod analysis;
mod diagnostics;
mod document;
mod lexer;
mod navigation;
mod parser;
mod sequence;
mod source;
mod syntax;
mod tree;
mod update;
mod view;

pub use analysis::{AnalysisError, Identity, Memo};
pub use diagnostics::{DiagnosticCode, Severity};
pub use document::{Document, DocumentError, Revision, Snapshot};
pub use lexer::{Lexer, classify_name};
pub use navigation::{Descendants, Element, Elements, TokenView, Tokens};
pub use parser::context::{Expectation, Expected};
pub use parser::control::{Control, Limits, ParseError, Resource};
pub use parser::parse;
pub use source::{CoordinateError, Position};
pub use syntax::{InterpolatedKind, Keyword, LexError, Operator, Span, Token, TokenKind};
pub use tree::{Diagnostic, Kind, Tree};
pub use update::{Edit, EditError};
pub use view::{Children, Parts, View};

/// # Errors
///
/// Returns an error if parsing is cancelled or exceeds a configured resource limit.
pub fn parse_with(source: &[u8], control: &Control) -> Result<Tree, ParseError> {
    parser::controlled(source, control)
}
