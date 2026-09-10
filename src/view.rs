use std::{fmt, iter::FusedIterator, slice};

use bstr::{BStr, ByteSlice};

use crate::{Kind, Node, Span, Tree};

#[derive(Clone, Copy)]
pub struct View<'tree, 'source> {
    tree: &'tree Tree<'source>,
    index: usize,
}

#[derive(Clone)]
pub struct Children<'tree, 'source> {
    tree: &'tree Tree<'source>,
    indices: slice::Iter<'tree, usize>,
}

#[derive(Clone, Debug)]
pub enum Parts<'tree, 'source> {
    Leaf,
    Root {
        block: View<'tree, 'source>,
    },
    Block {
        statements: Children<'tree, 'source>,
    },
    Local {
        bindings: Children<'tree, 'source>,
        values: Children<'tree, 'source>,
    },
    Assignment {
        targets: Children<'tree, 'source>,
        operator: View<'tree, 'source>,
        values: Children<'tree, 'source>,
    },
    CallStatement {
        call: View<'tree, 'source>,
    },
    Function {
        attributes: Option<View<'tree, 'source>>,
        name: Option<View<'tree, 'source>>,
        generics: Option<View<'tree, 'source>>,
        parameters: View<'tree, 'source>,
        returns: Option<View<'tree, 'source>>,
        body: Option<View<'tree, 'source>>,
    },
    FunctionName {
        path: Children<'tree, 'source>,
        method: Option<View<'tree, 'source>>,
    },
    Parameters {
        parameters: Children<'tree, 'source>,
    },
    Binding {
        name: View<'tree, 'source>,
        annotation: Option<View<'tree, 'source>>,
    },
    Returns {
        annotation: View<'tree, 'source>,
    },
    If {
        branches: Children<'tree, 'source>,
        otherwise: Option<View<'tree, 'source>>,
    },
    Branch {
        condition: View<'tree, 'source>,
        body: View<'tree, 'source>,
    },
    Body {
        body: View<'tree, 'source>,
    },
    While {
        condition: View<'tree, 'source>,
        body: View<'tree, 'source>,
    },
    Repeat {
        body: View<'tree, 'source>,
        condition: View<'tree, 'source>,
    },
    NumericFor {
        binding: View<'tree, 'source>,
        start: View<'tree, 'source>,
        end: View<'tree, 'source>,
        step: Option<View<'tree, 'source>>,
        body: View<'tree, 'source>,
    },
    GenericFor {
        bindings: Children<'tree, 'source>,
        values: Children<'tree, 'source>,
        body: View<'tree, 'source>,
    },
    Return {
        values: Children<'tree, 'source>,
    },
    Export {
        attributes: Option<View<'tree, 'source>>,
        declaration: View<'tree, 'source>,
    },
    TypeAlias {
        name: View<'tree, 'source>,
        generics: Option<View<'tree, 'source>>,
        annotation: View<'tree, 'source>,
    },
    Declaration {
        name: View<'tree, 'source>,
        annotation: View<'tree, 'source>,
    },
    ClassDeclaration {
        class: View<'tree, 'source>,
    },
    Class {
        name: View<'tree, 'source>,
        extends: Option<View<'tree, 'source>>,
        members: Children<'tree, 'source>,
    },
    Property {
        binding: View<'tree, 'source>,
    },
    Extends {
        superclass: View<'tree, 'source>,
    },
    Attributes {
        attributes: Children<'tree, 'source>,
    },
    Attribute {
        name: &'source BStr,
        arguments: Option<View<'tree, 'source>>,
    },
    Arguments {
        values: Children<'tree, 'source>,
    },
    Generics {
        parameters: Children<'tree, 'source>,
    },
    Generic {
        name: View<'tree, 'source>,
        default: Option<View<'tree, 'source>>,
    },
    Variadic {
        annotation: Option<View<'tree, 'source>>,
    },
    Unary {
        operator: View<'tree, 'source>,
        operand: View<'tree, 'source>,
    },
    Binary {
        left: View<'tree, 'source>,
        operator: View<'tree, 'source>,
        right: View<'tree, 'source>,
    },
    Group {
        expression: View<'tree, 'source>,
    },
    Call {
        callee: View<'tree, 'source>,
        arguments: View<'tree, 'source>,
    },
    MethodCall {
        receiver: View<'tree, 'source>,
        method: View<'tree, 'source>,
        types: Option<View<'tree, 'source>>,
        arguments: View<'tree, 'source>,
    },
    Field {
        receiver: View<'tree, 'source>,
        name: View<'tree, 'source>,
    },
    Index {
        receiver: View<'tree, 'source>,
        key: View<'tree, 'source>,
    },
    Instantiate {
        expression: View<'tree, 'source>,
        arguments: View<'tree, 'source>,
    },
    Assertion {
        expression: View<'tree, 'source>,
        annotation: View<'tree, 'source>,
    },
    Conditional {
        condition: View<'tree, 'source>,
        truthy: View<'tree, 'source>,
        falsy: View<'tree, 'source>,
    },
    Interpolation {
        segments: Children<'tree, 'source>,
    },
    Table {
        fields: Children<'tree, 'source>,
    },
    TableField {
        key: Option<View<'tree, 'source>>,
        value: View<'tree, 'source>,
        indexed: bool,
    },
    TypeName {
        namespace: Option<View<'tree, 'source>>,
        name: View<'tree, 'source>,
        arguments: Option<View<'tree, 'source>>,
    },
    TypeTable {
        access: Option<View<'tree, 'source>>,
        element: Option<View<'tree, 'source>>,
        fields: Children<'tree, 'source>,
    },
    TypeField {
        access: Option<View<'tree, 'source>>,
        key: View<'tree, 'source>,
        annotation: View<'tree, 'source>,
    },
    TypeFunction {
        attributes: Option<View<'tree, 'source>>,
        generics: Option<View<'tree, 'source>>,
        parameters: View<'tree, 'source>,
        returns: View<'tree, 'source>,
    },
    TypeGroup {
        annotation: View<'tree, 'source>,
    },
    TypePack {
        types: Children<'tree, 'source>,
    },
    VariadicType {
        annotation: View<'tree, 'source>,
    },
    TypeParameter {
        name: View<'tree, 'source>,
        annotation: View<'tree, 'source>,
    },
    TypeArguments {
        types: Children<'tree, 'source>,
    },
    TypeUnion {
        types: Children<'tree, 'source>,
    },
    TypeIntersection {
        types: Children<'tree, 'source>,
    },
    TypeOptional {
        annotation: View<'tree, 'source>,
    },
    TypeOf {
        name: View<'tree, 'source>,
        expression: View<'tree, 'source>,
    },
}

impl<'source> Tree<'source> {
    #[must_use]
    pub fn view(&self, index: usize) -> Option<View<'_, 'source>> {
        self.nodes.get(index)?;
        Some(View { tree: self, index })
    }

    #[must_use]
    pub fn root_view(&self) -> Option<View<'_, 'source>> {
        self.view(self.root)
    }
}

impl<'tree, 'source> View<'tree, 'source> {
    #[must_use]
    pub fn index(self) -> usize {
        self.index
    }

    #[must_use]
    pub fn node(self) -> &'tree Node {
        &self.tree.nodes[self.index]
    }

    #[must_use]
    pub fn kind(self) -> Kind {
        self.node().kind
    }

    #[must_use]
    pub fn span(self) -> Span {
        self.node().span
    }

    #[must_use]
    pub fn text(self) -> &'source BStr {
        self.span().bytes(self.tree.source)
    }

    #[must_use]
    pub fn children(self) -> Children<'tree, 'source> {
        Children {
            tree: self.tree,
            indices: self
                .tree
                .children
                .get(self.node().children.clone())
                .unwrap_or(&[])
                .iter(),
        }
    }

    #[must_use]
    pub fn parts(self) -> Option<Parts<'tree, 'source>> {
        if self
            .tree
            .children
            .get(self.node().children.clone())?
            .iter()
            .any(|index| *index >= self.tree.nodes.len())
        {
            return None;
        }

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
            | Kind::Name
            | Kind::Number
            | Kind::String
            | Kind::Boolean
            | Kind::Nil
            | Kind::Operator
            | Kind::Break
            | Kind::Continue => self.node().children.is_empty().then_some(Parts::Leaf),
        }
    }

    fn statement(self) -> Option<Parts<'tree, 'source>> {
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

    fn expression(self) -> Option<Parts<'tree, 'source>> {
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
                    indexed: key.is_some() && self.text().as_bytes().starts_with(b"["),
                }
            }
            Kind::Attribute => {
                let name = children.optional(Kind::Name).map_or_else(
                    || {
                        BStr::new(
                            self.text()
                                .as_bytes()
                                .strip_prefix(b"@")
                                .unwrap_or(self.text().as_bytes()),
                        )
                    },
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

    fn annotation(self) -> Option<Parts<'tree, 'source>> {
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

    fn signature(self) -> Option<Parts<'tree, 'source>> {
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

impl<'tree, 'source> Children<'tree, 'source> {
    fn optional(&mut self, kind: Kind) -> Option<View<'tree, 'source>> {
        self.clone().next().filter(|child| child.kind() == kind)?;
        self.next()
    }

    fn prefix(&mut self, kind: Kind) -> Self {
        let count = self
            .clone()
            .take_while(|child| child.kind() == kind)
            .count();
        self.take_front(count)
    }

    fn take_front(&mut self, count: usize) -> Self {
        let (front, rest) = self.indices.as_slice().split_at(count);
        self.indices = rest.iter();
        Self {
            tree: self.tree,
            indices: front.iter(),
        }
    }

    fn finish(mut self, parts: Parts<'tree, 'source>) -> Option<Parts<'tree, 'source>> {
        self.next().is_none().then_some(parts)
    }
}

impl fmt::Debug for View<'_, '_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("View")
            .field("index", &self.index)
            .field("kind", &self.kind())
            .field("span", &self.span())
            .finish()
    }
}

impl fmt::Debug for Children<'_, '_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_list().entries(self.clone()).finish()
    }
}

impl<'tree, 'source> Iterator for Children<'tree, 'source> {
    type Item = View<'tree, 'source>;

    fn next(&mut self) -> Option<Self::Item> {
        self.indices.find_map(|index| self.tree.view(*index))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (0, Some(self.indices.len()))
    }
}

impl DoubleEndedIterator for Children<'_, '_> {
    fn next_back(&mut self) -> Option<Self::Item> {
        self.indices
            .by_ref()
            .rev()
            .find_map(|index| self.tree.view(*index))
    }
}

impl FusedIterator for Children<'_, '_> {}
