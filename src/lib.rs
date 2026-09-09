mod ast;
mod lexer;
mod parser;
mod syntax;

pub use ast::{
    Attribute, BinaryOperator, Binding, Block, Chunk, ClassMember, ClassMemberKind, Expression,
    ExpressionKind, Function, FunctionName, FunctionSignature, GenericParameter, IfBranch,
    IfCondition, Statement, TableField, TableKey, TypeArgument, TypeExpression, TypeExpressionKind,
    TypeField, TypeIndexer, TypePack, TypePackTail, TypeParameter, UnaryOperator,
};
pub use lexer::{Lexer, classify_name, tokenize};
pub use parser::{ParseError, ParseErrorKind, Parser, parse, parse_expression, parse_type};
pub use syntax::{InterpolatedKind, Keyword, LexError, Operator, Span, Token, TokenKind};
