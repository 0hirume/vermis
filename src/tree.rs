use std::{num::NonZeroUsize, ops::Range};

use crate::token::{Span, Token};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(transparent)]
/// Index into a tree’s node storage.
pub struct NodeIndex(NonZeroUsize);

impl NodeIndex {
    /// Creates an index from a zero-based position.
    ///
    /// # Panics
    /// Panics if `index` is `usize::MAX`.
    pub fn new(index: usize) -> Self {
        Self(
            NonZeroUsize::MIN
                .checked_add(index)
                .expect("node index overflow"),
        )
    }

    /// Returns the zero-based position.
    pub fn get(self) -> usize {
        self.0.get() - 1
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(transparent)]
/// Index into a tree’s token storage.
pub struct TokenIndex(NonZeroUsize);

impl TokenIndex {
    /// Creates an index from a zero-based position.
    ///
    /// # Panics
    /// Panics if `index` is `usize::MAX`.
    pub fn new(index: usize) -> Self {
        Self(
            NonZeroUsize::MIN
                .checked_add(index)
                .expect("token index overflow"),
        )
    }

    /// Returns the zero-based position.
    pub fn get(self) -> usize {
        self.0.get() - 1
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
/// Half-open range in a tree’s list storage.
pub struct NodeList(
    /// List entry range.
    pub Range<usize>,
);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
/// Node and its trailing separator.
pub struct ListEntry {
    /// List element node.
    pub node: NodeIndex,

    /// Trailing separator token.
    pub separator: Option<TokenIndex>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
/// Syntax node with source and token ranges.
pub struct Node {
    /// Named syntax fields.
    pub kind: NodeKind,

    /// Source byte range.
    pub span: Span,

    /// Half-open token range.
    pub tokens: Range<TokenIndex>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
/// Named syntax fields; optional fields may be absent or missing during recovery.
pub enum NodeKind {
    /// Complete source file.
    Root {
        /// Top-level statement block.
        block: NodeIndex,
        /// End-of-input token.
        end_of_file: TokenIndex,
    },

    /// Ordered statements.
    Block {
        /// Statements in source order.
        statements: NodeList,
    },

    /// Source consumed during recovery.
    Error,

    /// Required syntax absent from the source.
    Missing {
        /// Expected syntax role.
        expected: &'static str,
    },

    /// Identifier.
    Name {
        /// Source token.
        token: TokenIndex,
    },

    /// Numeric literal.
    Number {
        /// Source token.
        token: TokenIndex,
    },

    /// String literal or interpolation segment.
    String {
        /// Source token.
        token: TokenIndex,
    },

    /// Boolean literal.
    Boolean {
        /// Source token.
        token: TokenIndex,
    },

    /// Nil literal.
    Nil {
        /// Source token.
        token: TokenIndex,
    },

    /// Variadic expression or parameter.
    Variadic {
        /// `...` token.
        ellipsis: TokenIndex,
        /// `:` token.
        colon: Option<TokenIndex>,
        /// Type annotation.
        annotation: Option<NodeIndex>,
    },

    /// Local variable declaration.
    Local {
        /// Leading keyword token.
        keyword: TokenIndex,
        /// Declared bindings.
        bindings: NodeList,
        /// `=` token.
        assignment: Option<TokenIndex>,
        /// Value expressions in source order.
        values: NodeList,
    },

    /// Constant declaration.
    Constant {
        /// Leading keyword token.
        keyword: TokenIndex,
        /// Declared bindings.
        bindings: NodeList,
        /// `=` token.
        assignment: Option<TokenIndex>,
        /// Value expressions in source order.
        values: NodeList,
    },

    /// Assignment statement.
    Assignment {
        /// Assignment targets.
        targets: NodeList,
        /// `=` token.
        assignment: Option<TokenIndex>,
        /// Value expressions in source order.
        values: NodeList,
    },

    /// Compound assignment statement.
    CompoundAssignment {
        /// Assignment target.
        target: NodeIndex,
        /// Operator token.
        operator: TokenIndex,
        /// Value expression.
        value: NodeIndex,
    },

    /// Call used as a statement.
    CallStatement {
        /// Call expression.
        call: NodeIndex,
    },

    /// Function declaration or expression.
    Function {
        /// Attributes in source order.
        attributes: Option<NodeIndex>,
        /// Declaration prefix, such as `local` or `declare`.
        prefix: Option<TokenIndex>,
        /// Leading keyword token.
        keyword: Option<TokenIndex>,
        /// Name node.
        name: Option<NodeIndex>,
        /// Generic parameter list.
        generics: Option<NodeIndex>,
        /// Function parameters.
        parameters: NodeIndex,
        /// Return type syntax.
        returns: Option<NodeIndex>,
        /// Statement body.
        body: Option<NodeIndex>,
        /// `end` token.
        end: Option<TokenIndex>,
    },

    /// Qualified function name.
    FunctionName {
        /// Name components separated by dots.
        path: NodeList,
        /// `:` token.
        colon: Option<TokenIndex>,
        /// Method name.
        method: Option<NodeIndex>,
    },

    /// Function parameter list.
    Parameters {
        /// Opening delimiter token.
        opening: Option<TokenIndex>,
        /// Function parameters.
        parameters: NodeList,
        /// Closing delimiter token.
        closing: Option<TokenIndex>,
    },

    /// Name with an optional type annotation.
    Binding {
        /// Name node.
        name: NodeIndex,
        /// `:` token.
        colon: Option<TokenIndex>,
        /// Type annotation.
        annotation: Option<NodeIndex>,
    },

    /// Return type annotation.
    Returns {
        /// `:` token.
        colon: TokenIndex,
        /// Type annotation.
        annotation: NodeIndex,
    },

    /// Conditional statement.
    If {
        /// If and elseif branches.
        branches: NodeList,
        /// Else branch.
        otherwise: Option<NodeIndex>,
        /// `end` token.
        end: Option<TokenIndex>,
    },

    /// If or elseif branch.
    Branch {
        /// Leading keyword token.
        keyword: TokenIndex,
        /// Condition expression or binding.
        condition: NodeIndex,
        /// `then` token.
        then: Option<TokenIndex>,
        /// Statement body.
        body: NodeIndex,
    },

    /// Else branch.
    Else {
        /// Leading keyword token.
        keyword: TokenIndex,
        /// Statement body.
        body: NodeIndex,
    },

    /// While loop.
    While {
        /// Leading keyword token.
        keyword: TokenIndex,
        /// Condition expression or binding.
        condition: NodeIndex,
        /// `do` token.
        do_keyword: Option<TokenIndex>,
        /// Statement body.
        body: NodeIndex,
        /// `end` token.
        end: Option<TokenIndex>,
    },

    /// Repeat-until loop.
    Repeat {
        /// Leading keyword token.
        keyword: TokenIndex,
        /// Statement body.
        body: NodeIndex,
        /// `until` token.
        until: Option<TokenIndex>,
        /// Condition expression or binding.
        condition: NodeIndex,
    },

    /// Numeric for loop.
    NumericFor {
        /// Leading keyword token.
        keyword: TokenIndex,
        /// Declared binding.
        binding: NodeIndex,
        /// `=` token.
        assignment: Option<TokenIndex>,
        /// Initial loop value.
        start: NodeIndex,
        /// Comma before the limit.
        range_separator: Option<TokenIndex>,
        /// Loop limit expression.
        end: NodeIndex,
        /// Comma before the step.
        step_separator: Option<TokenIndex>,
        /// Loop step expression.
        step: Option<NodeIndex>,
        /// `do` token.
        do_keyword: Option<TokenIndex>,
        /// Statement body.
        body: NodeIndex,
        /// `end` token.
        end_keyword: Option<TokenIndex>,
    },

    /// Generic for loop.
    GenericFor {
        /// Leading keyword token.
        keyword: TokenIndex,
        /// Declared bindings.
        bindings: NodeList,
        /// `in` token.
        in_keyword: Option<TokenIndex>,
        /// Value expressions in source order.
        values: NodeList,
        /// `do` token.
        do_keyword: Option<TokenIndex>,
        /// Statement body.
        body: NodeIndex,
        /// `end` token.
        end: Option<TokenIndex>,
    },

    /// Do block.
    Do {
        /// Leading keyword token.
        keyword: TokenIndex,
        /// Statement body.
        body: NodeIndex,
        /// `end` token.
        end: Option<TokenIndex>,
    },

    /// Return statement.
    Return {
        /// Leading keyword token.
        keyword: TokenIndex,
        /// Value expressions in source order.
        values: NodeList,
    },

    /// Break statement.
    Break {
        /// Leading keyword token.
        keyword: TokenIndex,
    },

    /// Continue statement.
    Continue {
        /// Leading keyword token.
        keyword: TokenIndex,
    },

    /// Exported declaration.
    Export {
        /// Attributes in source order.
        attributes: Option<NodeIndex>,
        /// Leading keyword token.
        keyword: TokenIndex,
        /// Declared syntax.
        declaration: NodeIndex,
    },

    /// Type alias declaration.
    TypeAlias {
        /// Leading keyword token.
        keyword: TokenIndex,
        /// Name node.
        name: NodeIndex,
        /// Generic parameter list.
        generics: Option<NodeIndex>,
        /// `=` token.
        assignment: Option<TokenIndex>,
        /// Type annotation.
        annotation: NodeIndex,
    },

    /// Ambient declaration.
    Declaration {
        /// Leading keyword token.
        keyword: TokenIndex,
        /// `extern` token.
        external: Option<TokenIndex>,
        /// Declared syntax.
        declaration: NodeIndex,
    },

    /// Class declaration.
    Class {
        /// `open` token.
        open: Option<TokenIndex>,
        /// Leading keyword token.
        keyword: Option<TokenIndex>,
        /// Name node.
        name: NodeIndex,
        /// Superclass clause.
        extends: Option<NodeIndex>,
        /// `with` token.
        with: Option<TokenIndex>,
        /// Class members in source order.
        members: NodeList,
        /// `end` token.
        end: Option<TokenIndex>,
    },

    /// Public class property.
    Property {
        /// `public` token.
        public: TokenIndex,
        /// Declared binding.
        binding: NodeIndex,
    },

    /// Superclass clause.
    Extends {
        /// Leading keyword token.
        keyword: TokenIndex,
        /// Superclass expression.
        superclass: NodeIndex,
    },

    /// Function attributes.
    Attributes {
        /// Attributes in source order.
        attributes: NodeList,
    },

    /// Bracketed attribute group.
    AttributeGroup {
        /// Opening delimiter token.
        opening: TokenIndex,
        /// Attributes in source order.
        attributes: NodeList,
        /// Closing delimiter token.
        closing: Option<TokenIndex>,
    },

    /// Single attribute.
    Attribute {
        /// Name node.
        name: NodeIndex,
        /// Argument syntax.
        arguments: Option<NodeIndex>,
    },

    /// Call arguments.
    Arguments {
        /// Opening delimiter token.
        opening: Option<TokenIndex>,
        /// Value expressions in source order.
        values: NodeList,
        /// Closing delimiter token.
        closing: Option<TokenIndex>,
    },

    /// Generic parameter list.
    Generics {
        /// Opening delimiter token.
        opening: TokenIndex,
        /// Function parameters.
        parameters: NodeList,
        /// Closing delimiter token.
        closing: Option<TokenIndex>,
    },

    /// Generic parameter with an optional default.
    Generic {
        /// Name node.
        name: NodeIndex,
        /// `...` token.
        ellipsis: Option<TokenIndex>,
        /// `=` token.
        assignment: Option<TokenIndex>,
        /// Default type or type pack.
        default: Option<NodeIndex>,
    },

    /// Unary expression.
    Unary {
        /// Operator token.
        operator: TokenIndex,
        /// Operand expression.
        operand: NodeIndex,
    },

    /// Binary expression.
    Binary {
        /// Left operand.
        left: NodeIndex,
        /// Operator token.
        operator: TokenIndex,
        /// Right operand.
        right: NodeIndex,
    },

    /// Parenthesized expression.
    Group {
        /// Opening delimiter token.
        opening: TokenIndex,
        /// Expression node.
        expression: NodeIndex,
        /// Closing delimiter token.
        closing: Option<TokenIndex>,
    },

    /// Function call.
    Call {
        /// Called expression.
        callee: NodeIndex,
        /// Argument syntax.
        arguments: NodeIndex,
    },

    /// Method call.
    MethodCall {
        /// Receiver expression.
        receiver: NodeIndex,
        /// `:` token.
        colon: TokenIndex,
        /// Method name.
        method: NodeIndex,
        /// Explicit type arguments.
        instantiation: Option<NodeIndex>,
        /// Argument syntax.
        arguments: NodeIndex,
    },

    /// Named field access.
    Field {
        /// Receiver expression.
        receiver: NodeIndex,
        /// `.` token.
        dot: TokenIndex,
        /// Name node.
        name: NodeIndex,
    },

    /// Indexed access.
    Index {
        /// Receiver expression.
        receiver: NodeIndex,
        /// Opening delimiter token.
        opening: TokenIndex,
        /// Field key.
        key: NodeIndex,
        /// Closing delimiter token.
        closing: Option<TokenIndex>,
    },

    /// Explicit type instantiation.
    Instantiate {
        /// Expression node.
        expression: NodeIndex,
        /// Argument syntax.
        arguments: NodeIndex,
    },

    /// Double-angle type argument wrapper.
    InstantiationArguments {
        /// Opening delimiter token.
        opening: TokenIndex,
        /// Argument syntax.
        arguments: NodeIndex,
        /// Closing delimiter token.
        closing: Option<TokenIndex>,
    },

    /// Type assertion.
    Assertion {
        /// Expression node.
        expression: NodeIndex,
        /// Operator token.
        operator: TokenIndex,
        /// Type annotation.
        annotation: NodeIndex,
    },

    /// If expression.
    Conditional {
        /// Leading keyword token.
        keyword: TokenIndex,
        /// Condition expression or binding.
        condition: NodeIndex,
        /// `then` token.
        then: Option<TokenIndex>,
        /// Value when the condition holds.
        truthy: NodeIndex,
        /// `else` or `elseif` token.
        else_keyword: Option<TokenIndex>,
        /// Value when the condition is false.
        falsy: NodeIndex,
    },

    /// Interpolated string.
    Interpolation {
        /// String segments and expressions in source order.
        segments: NodeList,
    },

    /// Table constructor.
    Table {
        /// Opening delimiter token.
        opening: TokenIndex,
        /// Table fields in source order.
        fields: NodeList,
        /// Closing delimiter token.
        closing: Option<TokenIndex>,
    },

    /// Table constructor field.
    TableField {
        /// Opening delimiter token.
        opening: Option<TokenIndex>,
        /// Field key.
        key: Option<NodeIndex>,
        /// Closing delimiter token.
        closing: Option<TokenIndex>,
        /// `=` token.
        assignment: Option<TokenIndex>,
        /// Value expression.
        value: NodeIndex,
    },

    /// Type reference.
    TypeName {
        /// Type namespace.
        namespace: Option<NodeIndex>,
        /// `.` token.
        dot: Option<TokenIndex>,
        /// Name node.
        name: NodeIndex,
        /// Argument syntax.
        arguments: Option<NodeIndex>,
    },

    /// Table or array type.
    TypeTable {
        /// Opening delimiter token.
        opening: TokenIndex,
        /// `read` or `write` token.
        access: Option<TokenIndex>,
        /// Array element type.
        element: Option<NodeIndex>,
        /// Table fields in source order.
        fields: NodeList,
        /// Closing delimiter token.
        closing: Option<TokenIndex>,
    },

    /// Named table type field.
    TypeField {
        /// `read` or `write` token.
        access: Option<TokenIndex>,
        /// Opening delimiter token.
        opening: Option<TokenIndex>,
        /// Field key.
        key: NodeIndex,
        /// Closing delimiter token.
        closing: Option<TokenIndex>,
        /// `:` token.
        colon: Option<TokenIndex>,
        /// Type annotation.
        annotation: NodeIndex,
    },

    /// Table type indexer.
    TypeIndexer {
        /// `read` or `write` token.
        access: Option<TokenIndex>,
        /// Opening delimiter token.
        opening: TokenIndex,
        /// Field key.
        key: NodeIndex,
        /// Closing delimiter token.
        closing: Option<TokenIndex>,
        /// `:` token.
        colon: Option<TokenIndex>,
        /// Type annotation.
        annotation: NodeIndex,
    },

    /// Function type.
    TypeFunction {
        /// Attributes in source order.
        attributes: Option<NodeIndex>,
        /// Generic parameter list.
        generics: Option<NodeIndex>,
        /// Function parameters.
        parameters: NodeIndex,
        /// `->` token.
        arrow: Option<TokenIndex>,
        /// Return type syntax.
        returns: NodeIndex,
    },

    /// Parenthesized type.
    TypeGroup {
        /// Opening delimiter token.
        opening: TokenIndex,
        /// Type annotation.
        annotation: NodeIndex,
        /// Closing delimiter token.
        closing: Option<TokenIndex>,
    },

    /// Type pack.
    TypePack {
        /// Opening delimiter token.
        opening: Option<TokenIndex>,
        /// Types in source order.
        types: NodeList,
        /// Closing delimiter token.
        closing: Option<TokenIndex>,
    },

    /// Named generic type pack.
    GenericPack {
        /// Name node.
        name: NodeIndex,
        /// `...` token.
        ellipsis: Option<TokenIndex>,
    },

    /// Variadic type pack.
    VariadicType {
        /// `...` token.
        ellipsis: TokenIndex,
        /// Type annotation.
        annotation: NodeIndex,
    },

    /// Named function type parameter.
    TypeParameter {
        /// Name node.
        name: NodeIndex,
        /// `:` token.
        colon: TokenIndex,
        /// Type annotation.
        annotation: NodeIndex,
    },

    /// Type argument list.
    TypeArguments {
        /// Opening delimiter token.
        opening: TokenIndex,
        /// Type arguments in source order.
        arguments: NodeList,
        /// Closing delimiter token.
        closing: Option<TokenIndex>,
    },

    /// Union type.
    TypeUnion {
        /// Left operand.
        left: Option<NodeIndex>,
        /// Operator token.
        operator: TokenIndex,
        /// Right operand.
        right: NodeIndex,
    },

    /// Intersection type.
    TypeIntersection {
        /// Left operand.
        left: Option<NodeIndex>,
        /// Operator token.
        operator: TokenIndex,
        /// Right operand.
        right: NodeIndex,
    },

    /// Optional type.
    TypeOptional {
        /// Type annotation.
        annotation: NodeIndex,
        /// `?` token.
        question_mark: TokenIndex,
    },

    /// Type of an expression.
    TypeOf {
        /// Leading keyword token.
        keyword: TokenIndex,
        /// Opening delimiter token.
        opening: Option<TokenIndex>,
        /// Expression node.
        expression: NodeIndex,
        /// Closing delimiter token.
        closing: Option<TokenIndex>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
/// Parse error and its source range.
pub struct Diagnostic {
    /// Source byte range.
    pub span: Span,

    /// Diagnostic message.
    pub message: &'static str,
}

#[derive(Debug)]
/// Lossless indexed syntax over borrowed source bytes.
pub struct Tree<'source> {
    /// Borrowed source bytes.
    pub source: &'source [u8],

    /// All source tokens, including trivia and end-of-input.
    pub tokens: Vec<Token>,

    /// Nodes in child-before-parent order.
    pub nodes: Vec<Node>,

    /// Storage for node lists.
    pub lists: Vec<ListEntry>,

    /// Root node index.
    pub root: NodeIndex,

    /// Diagnostics in emission order.
    pub diagnostics: Vec<Diagnostic>,
}

impl<'source> Tree<'source> {
    /// Returns a node by index.
    pub fn node(&self, index: NodeIndex) -> &Node {
        &self.nodes[index.get()]
    }

    /// Returns a token by index.
    pub fn token(&self, index: TokenIndex) -> &Token {
        &self.tokens[index.get()]
    }

    /// Returns the entries in a node list.
    pub fn list(&self, list: &NodeList) -> &[ListEntry] {
        &self.lists[list.0.clone()]
    }

    /// Returns a node’s source bytes.
    pub fn text(&self, index: NodeIndex) -> &'source [u8] {
        self.node(index).span.bytes(self.source)
    }
}
