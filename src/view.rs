use std::{fmt, iter::FusedIterator, ops::Range};

use crate::{Kind, Span, Tree, tree::Occurrence};

#[derive(Clone, Copy)]
pub struct View<'tree> {
    pub(crate) tree: &'tree Tree,
    pub(crate) index: usize,
}

#[derive(Clone)]
pub struct Children<'tree> {
    tree: &'tree Tree,
    syntax: &'tree crate::tree::Node,
    ordinal: usize,
    indices: Range<usize>,
}

#[derive(Clone, Debug)]
pub enum Parts<'tree> {
    Leaf,

    Root {
        block: View<'tree>,
    },

    Block {
        statements: Children<'tree>,
    },

    Local {
        bindings: Children<'tree>,
        values: Children<'tree>,
    },

    Assignment {
        targets: Children<'tree>,
        operator: View<'tree>,
        values: Children<'tree>,
    },

    CallStatement {
        call: View<'tree>,
    },

    Function {
        attributes: Option<View<'tree>>,
        name: Option<View<'tree>>,
        generics: Option<View<'tree>>,
        parameters: View<'tree>,
        returns: Option<View<'tree>>,
        body: Option<View<'tree>>,
    },

    FunctionName {
        path: Children<'tree>,
        method: Option<View<'tree>>,
    },

    Parameters {
        parameters: Children<'tree>,
    },

    Binding {
        name: View<'tree>,
        annotation: Option<View<'tree>>,
    },

    Returns {
        annotation: View<'tree>,
    },

    If {
        branches: Children<'tree>,
        otherwise: Option<View<'tree>>,
    },

    Branch {
        condition: View<'tree>,
        body: View<'tree>,
    },

    Body {
        body: View<'tree>,
    },

    While {
        condition: View<'tree>,
        body: View<'tree>,
    },

    Repeat {
        body: View<'tree>,
        condition: View<'tree>,
    },

    NumericFor {
        binding: View<'tree>,
        start: View<'tree>,
        end: View<'tree>,
        step: Option<View<'tree>>,
        body: View<'tree>,
    },

    GenericFor {
        bindings: Children<'tree>,
        values: Children<'tree>,
        body: View<'tree>,
    },

    Return {
        values: Children<'tree>,
    },

    Export {
        attributes: Option<View<'tree>>,
        declaration: View<'tree>,
    },

    TypeAlias {
        name: View<'tree>,
        generics: Option<View<'tree>>,
        annotation: View<'tree>,
    },

    Declaration {
        name: View<'tree>,
        annotation: View<'tree>,
    },

    ClassDeclaration {
        class: View<'tree>,
    },

    Class {
        name: View<'tree>,
        extends: Option<View<'tree>>,
        members: Children<'tree>,
    },

    Property {
        binding: View<'tree>,
    },

    Extends {
        superclass: View<'tree>,
    },

    Attributes {
        attributes: Children<'tree>,
    },

    Attribute {
        name: &'tree [u8],
        arguments: Option<View<'tree>>,
    },

    Arguments {
        values: Children<'tree>,
    },

    Generics {
        parameters: Children<'tree>,
    },

    Generic {
        name: View<'tree>,
        default: Option<View<'tree>>,
    },

    Variadic {
        annotation: Option<View<'tree>>,
    },

    Unary {
        operator: View<'tree>,
        operand: View<'tree>,
    },

    Binary {
        left: View<'tree>,
        operator: View<'tree>,
        right: View<'tree>,
    },

    Group {
        expression: View<'tree>,
    },

    Call {
        callee: View<'tree>,
        arguments: View<'tree>,
    },

    MethodCall {
        receiver: View<'tree>,
        method: View<'tree>,
        types: Option<View<'tree>>,
        arguments: View<'tree>,
    },

    Field {
        receiver: View<'tree>,
        name: View<'tree>,
    },

    Index {
        receiver: View<'tree>,
        key: View<'tree>,
    },

    Instantiate {
        expression: View<'tree>,
        arguments: View<'tree>,
    },

    Assertion {
        expression: View<'tree>,
        annotation: View<'tree>,
    },

    Conditional {
        condition: View<'tree>,
        truthy: View<'tree>,
        falsy: View<'tree>,
    },

    Interpolation {
        segments: Children<'tree>,
    },

    Table {
        fields: Children<'tree>,
    },

    TableField {
        key: Option<View<'tree>>,
        value: View<'tree>,
        indexed: bool,
    },

    TypeName {
        namespace: Option<View<'tree>>,
        name: View<'tree>,
        arguments: Option<View<'tree>>,
    },

    TypeTable {
        access: Option<View<'tree>>,
        element: Option<View<'tree>>,
        fields: Children<'tree>,
    },

    TypeField {
        access: Option<View<'tree>>,
        key: View<'tree>,
        annotation: View<'tree>,
    },

    TypeFunction {
        attributes: Option<View<'tree>>,
        generics: Option<View<'tree>>,
        parameters: View<'tree>,
        returns: View<'tree>,
    },

    TypeGroup {
        annotation: View<'tree>,
    },

    TypePack {
        types: Children<'tree>,
    },

    VariadicType {
        annotation: View<'tree>,
    },

    TypeParameter {
        name: View<'tree>,
        annotation: View<'tree>,
    },

    TypeArguments {
        types: Children<'tree>,
    },

    TypeUnion {
        types: Children<'tree>,
    },

    TypeIntersection {
        types: Children<'tree>,
    },

    TypeOptional {
        annotation: View<'tree>,
    },

    TypeOf {
        name: View<'tree>,
        expression: View<'tree>,
    },
}

impl Tree {
    pub(crate) fn view(&self, index: usize) -> View<'_> {
        View { tree: self, index }
    }
}

impl<'tree> View<'tree> {
    #[must_use]
    pub(crate) fn node(self) -> Occurrence<'tree> {
        self.tree.occurrence(self.index)
    }

    #[must_use]
    pub fn kind(self) -> Kind {
        self.node().syntax.kind
    }

    #[must_use]
    pub fn span(self) -> Span {
        self.node().span
    }

    #[must_use]
    pub fn text(self) -> &'tree [u8] {
        self.node().syntax.text()
    }

    pub fn recovery(
        self,
    ) -> impl Iterator<Item = crate::parser::context::Expectation> + Clone + 'tree {
        let occurrence = self.node();

        occurrence
            .syntax
            .recovery
            .iter()
            .map(move |mut expectation| {
                expectation.span.start += occurrence.span.start;
                expectation.span.end += occurrence.span.start;

                expectation
            })
    }

    #[must_use]
    pub fn identity(self) -> crate::Identity {
        crate::Identity::new(std::sync::Arc::clone(&self.node().syntax.identity))
    }

    #[must_use]
    pub fn children(self) -> Children<'tree> {
        let node = self.node();

        Children {
            tree: self.tree,
            syntax: node.syntax,
            ordinal: self.index + 1,
            indices: 0..node.syntax.edges.measure().children,
        }
    }

    #[must_use]
    pub fn parts(self) -> Option<Parts<'tree>> {
        match self.kind() {
            Kind::Root
            | Kind::Block
            | Kind::Local
            | Kind::Constant
            | Kind::Assignment
            | Kind::CompoundAssignment
            | Kind::CallStatement
            | Kind::If
            | Kind::Branch
            | Kind::Else
            | Kind::While
            | Kind::Repeat
            | Kind::NumericFor
            | Kind::GenericFor
            | Kind::Do
            | Kind::Return
            | Kind::Export
            | Kind::Class
            | Kind::Property
            | Kind::Extends => self.statement(),

            Kind::Unary
            | Kind::Binary
            | Kind::Group
            | Kind::Call
            | Kind::MethodCall
            | Kind::Field
            | Kind::Index
            | Kind::Instantiate
            | Kind::Assertion
            | Kind::Conditional
            | Kind::Interpolation
            | Kind::Table
            | Kind::TableField
            | Kind::Attribute => self.expression(),

            Kind::TypeAlias
            | Kind::TypeName
            | Kind::TypeTable
            | Kind::TypeField
            | Kind::TypeIndexer
            | Kind::TypeFunctionExpression
            | Kind::TypeGroup
            | Kind::TypePack
            | Kind::VariadicType
            | Kind::TypeParameter
            | Kind::TypeArguments
            | Kind::TypeUnion
            | Kind::TypeIntersection
            | Kind::TypeOptional
            | Kind::TypeOf
            | Kind::Generic
            | Kind::GenericPack => self.annotation(),

            Kind::Function
            | Kind::LocalFunction
            | Kind::TypeFunction
            | Kind::Method
            | Kind::Declaration
            | Kind::FunctionName
            | Kind::Parameters
            | Kind::Binding
            | Kind::Returns
            | Kind::Attributes
            | Kind::Arguments
            | Kind::Generics
            | Kind::Variadic => self.signature(),

            Kind::Error
            | Kind::Missing
            | Kind::Name
            | Kind::Number
            | Kind::String
            | Kind::Boolean
            | Kind::Nil
            | Kind::Operator
            | Kind::Break
            | Kind::Continue => self.children().is_empty().then_some(Parts::Leaf),
        }
    }

    fn statement(self) -> Option<Parts<'tree>> {
        let mut children = self.children();

        let parts = match self.kind() {
            Kind::Root => Parts::Root {
                block: children.next()?,
            },

            Kind::Block => {
                return Some(Parts::Block {
                    statements: children,
                });
            }

            Kind::Local | Kind::Constant => {
                let bindings = children.prefix(Kind::Binding);

                return Some(Parts::Local {
                    bindings,
                    values: children,
                });
            }

            Kind::Assignment | Kind::CompoundAssignment => {
                let separator = children
                    .clone()
                    .position(|child| child.kind() == Kind::Operator)?;

                let targets = children.take_front(separator);
                let operator = children.next()?;

                return Some(Parts::Assignment {
                    targets,
                    operator,
                    values: children,
                });
            }

            Kind::CallStatement => Parts::CallStatement {
                call: children.next()?,
            },

            Kind::If => {
                let branches = children.prefix(Kind::Branch);

                Parts::If {
                    branches,
                    otherwise: children.optional(Kind::Else),
                }
            }

            Kind::Branch => Parts::Branch {
                condition: children.next()?,
                body: children.next()?,
            },

            Kind::Else | Kind::Do => Parts::Body {
                body: children.next()?,
            },

            Kind::While => Parts::While {
                condition: children.next()?,
                body: children.next()?,
            },

            Kind::Repeat => Parts::Repeat {
                body: children.next()?,
                condition: children.next()?,
            },

            Kind::NumericFor => {
                let binding = children.next()?;
                let start = children.next()?;
                let end = children.next()?;
                let body = children.next_back()?;

                Parts::NumericFor {
                    binding,
                    start,
                    end,
                    step: children.next(),
                    body,
                }
            }

            Kind::GenericFor => {
                let bindings = children.prefix(Kind::Binding);
                let body = children.next_back()?;

                return Some(Parts::GenericFor {
                    bindings,
                    values: children,
                    body,
                });
            }

            Kind::Return => return Some(Parts::Return { values: children }),

            Kind::Export => Parts::Export {
                attributes: children.optional(Kind::Attributes),
                declaration: children.next()?,
            },

            Kind::Class => {
                let name = children.next()?;
                let extends = children.optional(Kind::Extends);

                return Some(Parts::Class {
                    name,
                    extends,
                    members: children,
                });
            }

            Kind::Property => Parts::Property {
                binding: children.next()?,
            },

            Kind::Extends => Parts::Extends {
                superclass: children.next()?,
            },

            _ => return None,
        };

        children.finish(parts)
    }

    fn expression(self) -> Option<Parts<'tree>> {
        let mut children = self.children();

        let parts = match self.kind() {
            Kind::Unary => Parts::Unary {
                operator: children.next()?,
                operand: children.next()?,
            },

            Kind::Binary => Parts::Binary {
                left: children.next()?,
                operator: children.next()?,
                right: children.next()?,
            },

            Kind::Group => Parts::Group {
                expression: children.next()?,
            },

            Kind::Call => Parts::Call {
                callee: children.next()?,
                arguments: children.next()?,
            },

            Kind::MethodCall => Parts::MethodCall {
                receiver: children.next()?,
                method: children.next()?,
                types: children.optional(Kind::TypeArguments),
                arguments: children.next()?,
            },

            Kind::Field => Parts::Field {
                receiver: children.next()?,
                name: children.next()?,
            },

            Kind::Index => Parts::Index {
                receiver: children.next()?,
                key: children.next()?,
            },

            Kind::Instantiate => Parts::Instantiate {
                expression: children.next()?,
                arguments: children.next()?,
            },

            Kind::Assertion => Parts::Assertion {
                expression: children.next()?,
                annotation: children.next()?,
            },

            Kind::Conditional => Parts::Conditional {
                condition: children.next()?,
                truthy: children.next()?,
                falsy: children.next()?,
            },

            Kind::Interpolation => return Some(Parts::Interpolation { segments: children }),
            Kind::Table => return Some(Parts::Table { fields: children }),

            Kind::TableField => {
                let value = children.next_back()?;
                let key = children.next();

                Parts::TableField {
                    key,
                    value,
                    indexed: key.is_some() && self.text().starts_with(b"["),
                }
            }

            Kind::Attribute => {
                let name = children.optional(Kind::Name).map_or_else(
                    || self.text().strip_prefix(b"@").unwrap_or(self.text()),
                    Self::text,
                );

                Parts::Attribute {
                    name,
                    arguments: children.optional(Kind::Arguments),
                }
            }

            _ => return None,
        };

        children.finish(parts)
    }

    fn annotation(self) -> Option<Parts<'tree>> {
        let mut children = self.children();

        let parts = match self.kind() {
            Kind::TypeAlias => Parts::TypeAlias {
                name: children.next()?,
                generics: children.optional(Kind::Generics),
                annotation: children.next()?,
            },

            Kind::Generic | Kind::GenericPack => Parts::Generic {
                name: children.next()?,
                default: children.next(),
            },

            Kind::TypeName => {
                let first = children.next()?;
                let second = children.optional(Kind::Name);

                Parts::TypeName {
                    namespace: second.map(|_| first),
                    name: second.unwrap_or(first),
                    arguments: children.optional(Kind::TypeArguments),
                }
            }

            Kind::TypeTable => {
                let access = children.optional(Kind::Operator);

                let element = match children.clone().next() {
                    Some(child) if !matches!(child.kind(), Kind::TypeField | Kind::TypeIndexer) => {
                        children.next()
                    }

                    _ => None,
                };

                return Some(Parts::TypeTable {
                    access,
                    element,
                    fields: children,
                });
            }

            Kind::TypeField | Kind::TypeIndexer => Parts::TypeField {
                access: children.optional(Kind::Operator),
                key: children.next()?,
                annotation: children.next()?,
            },

            Kind::TypeFunctionExpression => Parts::TypeFunction {
                attributes: children.optional(Kind::Attributes),
                generics: children.optional(Kind::Generics),
                parameters: children.next()?,
                returns: children.next()?,
            },

            Kind::TypeGroup => Parts::TypeGroup {
                annotation: children.next()?,
            },

            Kind::TypePack => return Some(Parts::TypePack { types: children }),

            Kind::VariadicType => Parts::VariadicType {
                annotation: children.next()?,
            },

            Kind::TypeParameter => Parts::TypeParameter {
                name: children.next()?,
                annotation: children.next()?,
            },

            Kind::TypeArguments => return Some(Parts::TypeArguments { types: children }),
            Kind::TypeUnion => return Some(Parts::TypeUnion { types: children }),
            Kind::TypeIntersection => return Some(Parts::TypeIntersection { types: children }),

            Kind::TypeOptional => Parts::TypeOptional {
                annotation: children.next()?,
            },

            Kind::TypeOf => Parts::TypeOf {
                name: children.next()?,
                expression: children.next()?,
            },

            _ => return None,
        };

        children.finish(parts)
    }

    fn signature(self) -> Option<Parts<'tree>> {
        let mut children = self.children();

        let parts = match self.kind() {
            Kind::Function
            | Kind::LocalFunction
            | Kind::TypeFunction
            | Kind::Method
            | Kind::Declaration => {
                let attributes = children.optional(Kind::Attributes);

                if self.kind() == Kind::Declaration {
                    if let Some(class) = children.optional(Kind::Class) {
                        return children.finish(Parts::ClassDeclaration { class });
                    }

                    if !children
                        .clone()
                        .any(|child| child.kind() == Kind::Parameters)
                    {
                        let name = children.next()?;
                        let annotation = children.next()?;

                        return children.finish(Parts::Declaration { name, annotation });
                    }
                }

                Parts::Function {
                    attributes,
                    name: children
                        .optional(Kind::Name)
                        .or_else(|| children.optional(Kind::FunctionName)),
                    generics: children.optional(Kind::Generics),
                    parameters: children.next()?,
                    returns: children.optional(Kind::Returns),
                    body: children.optional(Kind::Block),
                }
            }

            Kind::FunctionName => {
                let path = children.prefix(Kind::Name);

                let method = if children.optional(Kind::Operator).is_some() {
                    children.next()
                } else {
                    None
                };

                Parts::FunctionName { path, method }
            }

            Kind::Parameters => {
                return Some(Parts::Parameters {
                    parameters: children,
                });
            }

            Kind::Binding => Parts::Binding {
                name: children.next()?,
                annotation: children.next(),
            },

            Kind::Returns => Parts::Returns {
                annotation: children.next()?,
            },

            Kind::Attributes => {
                return Some(Parts::Attributes {
                    attributes: children,
                });
            }

            Kind::Arguments => return Some(Parts::Arguments { values: children }),

            Kind::Generics => {
                return Some(Parts::Generics {
                    parameters: children,
                });
            }

            Kind::Variadic => Parts::Variadic {
                annotation: children.next(),
            },

            _ => return None,
        };

        children.finish(parts)
    }
}

impl<'tree> Children<'tree> {
    fn optional(&mut self, kind: Kind) -> Option<View<'tree>> {
        self.clone().next().filter(|child| {
            child.kind() == kind || (kind == Kind::Name && child.kind() == Kind::Missing)
        })?;

        self.next()
    }

    fn prefix(&mut self, kind: Kind) -> Self {
        let count = self
            .clone()
            .take_while(|child| {
                child.kind() == kind || (kind == Kind::Name && child.kind() == Kind::Missing)
            })
            .count();

        self.take_front(count)
    }

    fn take_front(&mut self, count: usize) -> Self {
        assert!(count <= self.indices.len());
        let front = self.indices.start..self.indices.start + count;
        self.indices.start += count;

        Self {
            tree: self.tree,
            syntax: self.syntax,
            ordinal: self.ordinal,
            indices: front,
        }
    }

    pub(crate) fn get(&self, position: usize) -> Option<View<'tree>> {
        let (_, _, prefix) = self
            .syntax
            .edges
            .select(position, |measure| measure.children)?;

        Some(self.tree.view(self.ordinal + prefix.nodes))
    }

    fn is_empty(&self) -> bool {
        self.indices.is_empty()
    }

    fn finish(mut self, parts: Parts<'tree>) -> Option<Parts<'tree>> {
        self.next().is_none().then_some(parts)
    }
}

impl fmt::Debug for View<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("View")
            .field("kind", &self.kind())
            .field("span", &self.span())
            .finish()
    }
}

impl fmt::Debug for Children<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_list().entries(self.clone()).finish()
    }
}

impl<'tree> Iterator for Children<'tree> {
    type Item = View<'tree>;

    fn next(&mut self) -> Option<Self::Item> {
        let position = self.indices.next()?;

        self.get(position)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.indices.size_hint()
    }
}

impl DoubleEndedIterator for Children<'_> {
    fn next_back(&mut self) -> Option<Self::Item> {
        let position = self.indices.next_back()?;

        self.get(position)
    }
}

impl FusedIterator for Children<'_> {}

impl ExactSizeIterator for Children<'_> {}
