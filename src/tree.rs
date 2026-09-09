use bstr::BStr;

use crate::{Span, Token};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Root,
    Block,
    Error,

    Name,
    Number,
    String,
    Boolean,
    Nil,
    Variadic,
    Operator,

    Local,
    Constant,
    Assignment,
    CompoundAssignment,
    CallStatement,

    Function,
    LocalFunction,
    FunctionName,
    Parameters,
    Binding,
    Returns,

    If,
    Branch,
    Else,
    While,
    Repeat,
    NumericFor,
    GenericFor,
    Do,
    Return,
    Break,
    Continue,

    Export,
    TypeAlias,
    TypeFunction,
    Declaration,

    Class,
    Property,
    Method,
    Extends,

    Attributes,
    Attribute,
    Arguments,
    Generics,
    Generic,
    GenericPack,

    Unary,
    Binary,
    Group,
    Call,
    MethodCall,
    Field,
    Index,
    Instantiate,
    Assertion,
    Conditional,
    Interpolation,
    Table,
    TableField,

    TypeName,
    TypeTable,
    TypeField,
    TypeIndexer,
    TypeFunctionExpression,
    TypeGroup,
    TypePack,
    VariadicType,
    TypeParameter,
    TypeArguments,
    TypeUnion,
    TypeIntersection,
    TypeOptional,
    Typeof,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Node {
    pub kind: Kind,
    pub span: Span,
    pub children: Vec<usize>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    pub span: Span,
    pub message: &'static str,
}

#[derive(Debug)]
pub struct Tree<'source> {
    pub source: &'source BStr,
    pub tokens: Vec<Token>,
    pub nodes: Vec<Node>,
    pub root: usize,
    pub diagnostics: Vec<Diagnostic>,
}

impl Tree<'_> {
    #[must_use]
    pub fn text(&self, node: usize) -> &BStr {
        self.nodes[node].span.bytes(self.source)
    }
}
