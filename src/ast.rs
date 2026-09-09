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
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Binding {
    pub span: Span,
    pub name: Span,
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
    pub parameters: Vec<Binding>,
    pub variadic: bool,
    pub body: Block,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IfBranch {
    pub condition: Expression,
    pub body: Block,
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
        target: Expression,
        operator: Operator,
        value: Expression,
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
        from: Expression,
        to: Expression,
        step: Option<Expression>,
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

    Function(Function),
    Table(Vec<TableField>),
    Call {
        function: Box<Expression>,
        method: Option<Span>,
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
    Expression(Expression),
    Name(Span),
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
