use crate::syntax::{Operator, Span};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Chunk {
    pub span: Span,
    pub body: Vec<Statement>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Block {
    pub span: Span,
    pub body: Vec<Statement>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Attribute {
    pub span: Span,
    pub name: Option<Span>,
    pub arguments: Vec<Expression>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Binding {
    pub span: Span,
    pub name: Span,
    pub annotation: Option<TypeExpression>,
    pub is_const: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FunctionName {
    pub span: Span,
    pub parts: Vec<Span>,
    pub method: Option<Span>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Function {
    pub span: Span,
    pub attributes: Vec<Attribute>,
    pub generics: Vec<GenericParameter>,
    pub parameters: Vec<Binding>,
    pub variadic: bool,
    pub variadic_type: Option<TypePack>,
    pub return_types: Option<TypePack>,
    pub body: Block,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IfBranch {
    pub condition: IfCondition,
    pub body: Block,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IfCondition {
    Expression(Expression),
    Local { binding: Binding, value: Expression },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Statement {
    Empty {
        span: Span,
    },

    Local {
        span: Span,
        attributes: Vec<Attribute>,
        bindings: Vec<Binding>,
        values: Vec<Expression>,
        is_const: bool,
    },
    LocalFunction {
        span: Span,
        attributes: Vec<Attribute>,
        name: Span,
        function: Function,
    },

    Assignment {
        span: Span,
        targets: Vec<Expression>,
        values: Vec<Expression>,
    },
    CompoundAssignment {
        span: Span,
        target: Box<Expression>,
        operator: Operator,
        value: Box<Expression>,
    },
    Call {
        span: Span,
        expression: Expression,
    },
    Return {
        span: Span,
        values: Vec<Expression>,
    },
    Break {
        span: Span,
    },
    Continue {
        span: Span,
    },

    Do {
        span: Span,
        body: Block,
    },
    If {
        span: Span,
        branches: Vec<IfBranch>,
        else_body: Option<Block>,
    },
    While {
        span: Span,
        condition: Expression,
        body: Block,
    },
    Repeat {
        span: Span,
        body: Block,
        condition: Expression,
    },
    NumericFor {
        span: Span,
        binding: Binding,
        from: Box<Expression>,
        to: Box<Expression>,
        step: Option<Box<Expression>>,
        body: Block,
    },
    GenericFor {
        span: Span,
        bindings: Vec<Binding>,
        values: Vec<Expression>,
        body: Block,
    },

    Function {
        span: Span,
        attributes: Vec<Attribute>,
        name: FunctionName,
        function: Function,
    },

    TypeAlias {
        span: Span,
        exported: bool,
        name: Span,
        generics: Vec<GenericParameter>,
        value: TypeExpression,
    },
    TypeFunction {
        span: Span,
        exported: bool,
        name: Span,
        function: Function,
    },
    DeclareGlobal {
        span: Span,
        name: Span,
        annotation: TypeExpression,
    },
    DeclareFunction {
        span: Span,
        name: Span,
        signature: FunctionSignature,
    },
    Class {
        span: Span,
        exported: bool,
        open: bool,
        name: Span,
        superclass: Option<TypeExpression>,
        members: Vec<ClassMember>,
    },
    Export {
        span: Span,
        statement: Box<Statement>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FunctionSignature {
    pub span: Span,
    pub generics: Vec<GenericParameter>,
    pub parameters: Vec<TypeParameter>,
    pub variadic: Option<TypeExpression>,
    pub returns: TypePack,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClassMember {
    pub span: Span,
    pub name: Span,
    pub kind: ClassMemberKind,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ClassMemberKind {
    Property { annotation: Option<TypeExpression> },
    Method { function: Function },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Expression {
    pub span: Span,
    pub kind: ExpressionKind,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExpressionKind {
    Nil,
    Boolean(bool),
    Number,
    String,
    Interpolated(Vec<Expression>),
    Name,
    Vararg,

    Unary {
        operator: UnaryOperator,
        operand: Box<Expression>,
    },
    Binary {
        operator: BinaryOperator,
        left: Box<Expression>,
        right: Box<Expression>,
    },
    Group(Box<Expression>),
    IfElse {
        condition: Box<Expression>,
        then_expression: Box<Expression>,
        else_expression: Box<Expression>,
    },
    TypeAssertion {
        expression: Box<Expression>,
        annotation: TypeExpression,
    },

    Function(Function),
    Table(Vec<TableField>),
    Call {
        function: Box<Expression>,
        method: Option<Span>,
        type_arguments: Vec<TypeArgument>,
        type_arguments_span: Option<Span>,
        arguments: Vec<Expression>,
    },
    Index {
        object: Box<Expression>,
        index: Box<Expression>,
    },
    Field {
        object: Box<Expression>,
        name: Span,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TableField {
    pub span: Span,
    pub key: Option<TableKey>,
    pub value: Expression,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TableKey {
    Expression(Box<Expression>),
    Name(Span),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GenericParameter {
    pub span: Span,
    pub name: Span,
    pub is_pack: bool,
    pub default: Option<TypeExpression>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TypeParameter {
    pub span: Span,
    pub name: Option<Span>,
    pub annotation: TypeExpression,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TypeArgument {
    Type(TypeExpression),
    Pack(TypePack),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TypeExpression {
    pub span: Span,
    pub kind: TypeExpressionKind,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TypeExpressionKind {
    Name {
        path: Vec<Span>,
        arguments: Vec<TypeArgument>,
    },
    Nil,
    Boolean(bool),
    String,
    Number,
    Table {
        fields: Vec<TypeField>,
        indexer: Option<Box<TypeIndexer>>,
    },
    Function {
        generics: Vec<GenericParameter>,
        parameters: Vec<TypeParameter>,
        variadic: Option<Box<TypeExpression>>,
        returns: TypePack,
    },
    Typeof(Box<Expression>),
    Optional(Box<TypeExpression>),
    Union(Vec<TypeExpression>),
    Intersection(Vec<TypeExpression>),
    Group(Box<TypeExpression>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TypeField {
    pub span: Span,
    pub name: Option<Span>,
    pub key: Option<TypeExpression>,
    pub annotation: TypeExpression,
    pub optional: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TypeIndexer {
    pub span: Span,
    pub index: TypeExpression,
    pub result: TypeExpression,
    pub implicit: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TypePack {
    pub span: Span,
    pub types: Vec<TypeExpression>,
    pub tail: Option<TypePackTail>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TypePackTail {
    Variadic(Box<TypeExpression>),
    Generic(Span),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnaryOperator {
    Negate,
    Not,
    Length,
    BitNot,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BinaryOperator {
    Or,
    And,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
    Equal,
    NotEqual,
    BitOr,
    BitXor,
    BitAnd,
    ShiftLeft,
    ShiftRight,
    Concat,
    Add,
    Subtract,
    Multiply,
    Divide,
    FloorDivide,
    Modulo,
    Power,
}

impl BinaryOperator {
    #[must_use]
    pub const fn binding_power(self) -> (u8, u8) {
        match self {
            Self::Or => (1, 2),
            Self::And => (3, 4),

            Self::Less
            | Self::LessEqual
            | Self::Greater
            | Self::GreaterEqual
            | Self::Equal
            | Self::NotEqual => (5, 6),

            Self::BitOr => (7, 8),
            Self::BitXor => (9, 10),
            Self::BitAnd => (11, 12),
            Self::ShiftLeft | Self::ShiftRight => (13, 14),

            Self::Concat => (15, 15),
            Self::Add | Self::Subtract => (17, 18),
            Self::Multiply | Self::Divide | Self::FloorDivide | Self::Modulo => (19, 20),
            Self::Power => (23, 22),
        }
    }
}
