//! Shared syntax assertions.

use vermis::parse;
use vermis::token::TokenKind;
use vermis::tree::{NodeIndex, NodeKind, TokenIndex, Tree};

/// Parses source and validates its storage invariants.
///
/// # Panics
/// Panics if the tree violates a storage invariant.
pub fn check(source: &[u8]) -> Tree<'_> {
    let tree = parse(source);
    validate(&tree);

    tree
}

/// Parses source that must be valid.
///
/// # Panics
/// Panics on diagnostics or invalid storage.
pub fn accepted(source: &str) -> Tree<'_> {
    let tree = check(source.as_bytes());

    assert!(
        tree.diagnostics.is_empty(),
        "{source:?}: {:?}",
        tree.diagnostics
    );

    tree
}

/// Returns nodes in preorder, including the given root.
pub fn descendants(tree: &Tree<'_>, root: NodeIndex) -> Vec<NodeIndex> {
    let mut result = Vec::new();
    let mut pending = vec![root];

    while let Some(node) = pending.pop() {
        result.push(node);
        pending.extend(relations(tree, node).0.into_iter().rev());
    }

    result
}

/// Finds the first matching node in preorder.
///
/// # Panics
/// Panics if no node matches.
pub fn first(tree: &Tree<'_>, predicate: impl Fn(&NodeKind) -> bool) -> NodeIndex {
    descendants(tree, tree.root)
        .into_iter()
        .find(|index| predicate(&tree.node(*index).kind))
        .expect("expected syntax node")
}

/// Checks source coverage and indexed syntax relationships.
///
/// # Panics
/// Panics if any storage invariant is violated.
pub fn validate(tree: &Tree<'_>) {
    let source = tree.source;
    assert_eq!(tree.text(tree.root), source);
    let mut end = 0;

    for (index, token) in tree.tokens.iter().enumerate() {
        assert_eq!(token.span.start, end);
        assert!(token.span.end <= source.len());

        if token.kind == TokenKind::EndOfFile {
            assert_eq!(index, tree.tokens.len() - 1);
            assert_eq!(token.span.start, source.len());
            assert!(token.span.is_empty());
        } else {
            assert!(token.span.end > token.span.start);
        }

        end = token.span.end;
    }

    assert_eq!(end, source.len());
    assert_eq!(tree.tokens.last().unwrap().kind, TokenKind::EndOfFile);

    assert_eq!(
        tree.tokens
            .iter()
            .flat_map(|token| token.bytes(source))
            .copied()
            .collect::<Vec<_>>(),
        source
    );

    let mut seen = vec![false; tree.nodes.len()];
    let mut pending = vec![tree.root];

    while let Some(index) = pending.pop() {
        assert!(!seen[index.0], "shared child {index:?}");
        seen[index.0] = true;
        let node = tree.node(index);

        assert!(
            node.span.start <= node.span.end && node.span.end <= source.len(),
            "{node:?}"
        );

        assert!(node.tokens.start.0 <= node.tokens.end.0 && node.tokens.end.0 <= tree.tokens.len());
        assert_eq!(tree.text(index), node.span.bytes(source));

        if matches!(node.kind, NodeKind::Missing { .. }) {
            assert!(node.span.is_empty());
            assert_eq!(node.tokens.start, node.tokens.end);
        }

        let (children, punctuation) = relations(tree, index);
        let mut end = node.span.start;
        let mut token_end = node.tokens.start.0;

        for child in children {
            assert!(child.0 < index.0, "non-postorder child {child:?}: {node:?}");
            let child_node = tree.node(child);
            let span = child_node.span;

            assert!(
                span.start >= end && span.end <= node.span.end,
                "unordered children: {node:?}, child {child_node:?}, previous end {end}; {:?}",
                String::from_utf8_lossy(source)
            );

            assert!(
                child_node.tokens.start.0 >= token_end
                    && child_node.tokens.end.0 <= node.tokens.end.0
            );

            end = span.end;
            token_end = child_node.tokens.end.0;
            pending.push(child);
        }

        for token in punctuation {
            assert!(
                token.0 >= node.tokens.start.0 && token.0 < node.tokens.end.0,
                "punctuation outside node: {node:?}, {token:?}"
            );

            let span = tree.token(token).span;
            assert!(span.start >= node.span.start && span.end <= node.span.end);
        }

        let bytes: Vec<_> = tree.tokens[node.tokens.start.0..node.tokens.end.0]
            .iter()
            .flat_map(|token| token.bytes(source))
            .copied()
            .collect();

        assert_eq!(
            bytes,
            tree.text(index),
            "token range differs from source span: {node:?}"
        );
    }

    assert!(
        seen.iter().all(|seen| *seen),
        "unreachable node in {:?}",
        String::from_utf8_lossy(source)
    );

    for diagnostic in &tree.diagnostics {
        assert!(
            diagnostic.span.start <= diagnostic.span.end && diagnostic.span.end <= source.len()
        );
    }
}

/// Returns direct child nodes and punctuation references.
pub fn relations(tree: &Tree<'_>, index: NodeIndex) -> (Vec<NodeIndex>, Vec<TokenIndex>) {
    let mut children = Vec::new();
    let mut punctuation = Vec::new();
    let kind = &tree.node(index).kind;

    let visit: fn(&Tree<'_>, &NodeKind, &mut Vec<NodeIndex>, &mut Vec<TokenIndex>) = match kind {
        NodeKind::Root { .. }
        | NodeKind::Block { .. }
        | NodeKind::Missing { .. }
        | NodeKind::Error
        | NodeKind::Name { .. }
        | NodeKind::Number { .. }
        | NodeKind::String { .. }
        | NodeKind::Boolean { .. }
        | NodeKind::Nil { .. }
        | NodeKind::Variadic { .. }
        | NodeKind::Break { .. }
        | NodeKind::Continue { .. } => leaves,

        NodeKind::Local { .. }
        | NodeKind::Constant { .. }
        | NodeKind::Assignment { .. }
        | NodeKind::CompoundAssignment { .. }
        | NodeKind::CallStatement { .. }
        | NodeKind::Return { .. } => assignments,

        NodeKind::Function { .. }
        | NodeKind::FunctionName { .. }
        | NodeKind::Parameters { .. }
        | NodeKind::Binding { .. }
        | NodeKind::Returns { .. } => functions,

        NodeKind::If { .. }
        | NodeKind::Branch { .. }
        | NodeKind::Else { .. }
        | NodeKind::While { .. }
        | NodeKind::Repeat { .. }
        | NodeKind::Do { .. } => conditions,

        NodeKind::NumericFor { .. } | NodeKind::GenericFor { .. } => loops,

        NodeKind::Export { .. }
        | NodeKind::TypeAlias { .. }
        | NodeKind::Declaration { .. }
        | NodeKind::Class { .. }
        | NodeKind::Property { .. }
        | NodeKind::Extends { .. } => declarations,

        NodeKind::Attributes { .. }
        | NodeKind::AttributeGroup { .. }
        | NodeKind::Attribute { .. }
        | NodeKind::Arguments { .. }
        | NodeKind::Generics { .. }
        | NodeKind::Generic { .. } => attributes,

        NodeKind::Unary { .. }
        | NodeKind::Binary { .. }
        | NodeKind::Group { .. }
        | NodeKind::Call { .. }
        | NodeKind::MethodCall { .. }
        | NodeKind::Field { .. }
        | NodeKind::Index { .. } => expressions,

        NodeKind::Instantiate { .. }
        | NodeKind::InstantiationArguments { .. }
        | NodeKind::Assertion { .. }
        | NodeKind::Conditional { .. }
        | NodeKind::Interpolation { .. }
        | NodeKind::Table { .. }
        | NodeKind::TableField { .. } => constructors,

        NodeKind::TypeName { .. }
        | NodeKind::TypeTable { .. }
        | NodeKind::TypeField { .. }
        | NodeKind::TypeIndexer { .. }
        | NodeKind::TypeFunction { .. } => references,

        NodeKind::TypeGroup { .. }
        | NodeKind::TypePack { .. }
        | NodeKind::GenericPack { .. }
        | NodeKind::VariadicType { .. }
        | NodeKind::TypeParameter { .. }
        | NodeKind::TypeArguments { .. }
        | NodeKind::TypeUnion { .. }
        | NodeKind::TypeIntersection { .. }
        | NodeKind::TypeOptional { .. }
        | NodeKind::TypeOf { .. } => annotations,
    };

    visit(tree, kind, &mut children, &mut punctuation);

    (children, punctuation)
}

fn leaves(
    tree: &Tree<'_>,
    kind: &NodeKind,
    children: &mut Vec<NodeIndex>,
    punctuation: &mut Vec<TokenIndex>,
) {
    match kind {
        NodeKind::Root {
            block, end_of_file, ..
        } => {
            children.push(*block);
            punctuation.push(*end_of_file);
        }

        NodeKind::Block { statements, .. } => {
            for entry in tree.list(statements) {
                children.push(entry.node);
                punctuation.extend(entry.separator);
            }
        }

        NodeKind::Missing { .. } | NodeKind::Error => {}

        NodeKind::Name { token, .. }
        | NodeKind::Number { token, .. }
        | NodeKind::String { token, .. }
        | NodeKind::Boolean { token, .. }
        | NodeKind::Nil { token, .. } => {
            punctuation.push(*token);
        }

        NodeKind::Variadic {
            ellipsis,
            colon,
            annotation,
            ..
        } => {
            punctuation.push(*ellipsis);
            punctuation.extend(*colon);
            children.extend(*annotation);
        }

        NodeKind::Break { keyword, .. } | NodeKind::Continue { keyword, .. } => {
            punctuation.push(*keyword);
        }

        _ => unreachable!("unexpected syntax family"),
    }
}

fn assignments(
    tree: &Tree<'_>,
    kind: &NodeKind,
    children: &mut Vec<NodeIndex>,
    punctuation: &mut Vec<TokenIndex>,
) {
    match kind {
        NodeKind::Local {
            keyword,
            bindings,
            assignment,
            values,
            ..
        }
        | NodeKind::Constant {
            keyword,
            bindings,
            assignment,
            values,
            ..
        } => {
            punctuation.push(*keyword);

            for entry in tree.list(bindings) {
                children.push(entry.node);
                punctuation.extend(entry.separator);
            }

            punctuation.extend(*assignment);

            for entry in tree.list(values) {
                children.push(entry.node);
                punctuation.extend(entry.separator);
            }
        }

        NodeKind::Assignment {
            targets,
            assignment,
            values,
            ..
        } => {
            for entry in tree.list(targets) {
                children.push(entry.node);
                punctuation.extend(entry.separator);
            }

            punctuation.extend(*assignment);

            for entry in tree.list(values) {
                children.push(entry.node);
                punctuation.extend(entry.separator);
            }
        }

        NodeKind::CompoundAssignment {
            target,
            operator,
            value,
            ..
        } => {
            children.push(*target);
            punctuation.push(*operator);
            children.push(*value);
        }

        NodeKind::CallStatement { call, .. } => {
            children.push(*call);
        }

        NodeKind::Return {
            keyword, values, ..
        } => {
            punctuation.push(*keyword);

            for entry in tree.list(values) {
                children.push(entry.node);
                punctuation.extend(entry.separator);
            }
        }

        _ => unreachable!("unexpected syntax family"),
    }
}

fn functions(
    tree: &Tree<'_>,
    kind: &NodeKind,
    children: &mut Vec<NodeIndex>,
    punctuation: &mut Vec<TokenIndex>,
) {
    match kind {
        NodeKind::Function {
            attributes,
            prefix,
            keyword,
            name,
            generics,
            parameters,
            returns,
            body,
            end,
            ..
        } => {
            children.extend(*attributes);
            punctuation.extend(*prefix);
            punctuation.extend(*keyword);
            children.extend(*name);
            children.extend(*generics);
            children.push(*parameters);
            children.extend(*returns);
            children.extend(*body);
            punctuation.extend(*end);
        }

        NodeKind::FunctionName {
            path,
            colon,
            method,
            ..
        } => {
            for entry in tree.list(path) {
                children.push(entry.node);
                punctuation.extend(entry.separator);
            }

            punctuation.extend(*colon);
            children.extend(*method);
        }

        NodeKind::Parameters {
            opening,
            parameters,
            closing,
            ..
        } => {
            punctuation.extend(*opening);

            for entry in tree.list(parameters) {
                children.push(entry.node);
                punctuation.extend(entry.separator);
            }

            punctuation.extend(*closing);
        }

        NodeKind::Binding {
            name,
            colon,
            annotation,
            ..
        } => {
            children.push(*name);
            punctuation.extend(*colon);
            children.extend(*annotation);
        }

        NodeKind::Returns {
            colon, annotation, ..
        } => {
            punctuation.push(*colon);
            children.push(*annotation);
        }

        _ => unreachable!("unexpected syntax family"),
    }
}

fn conditions(
    tree: &Tree<'_>,
    kind: &NodeKind,
    children: &mut Vec<NodeIndex>,
    punctuation: &mut Vec<TokenIndex>,
) {
    match kind {
        NodeKind::If {
            branches,
            otherwise,
            end,
            ..
        } => {
            for entry in tree.list(branches) {
                children.push(entry.node);
                punctuation.extend(entry.separator);
            }

            children.extend(*otherwise);
            punctuation.extend(*end);
        }

        NodeKind::Branch {
            keyword,
            condition,
            then,
            body,
            ..
        } => {
            punctuation.push(*keyword);
            children.push(*condition);
            punctuation.extend(*then);
            children.push(*body);
        }

        NodeKind::Else { keyword, body, .. } => {
            punctuation.push(*keyword);
            children.push(*body);
        }

        NodeKind::While {
            keyword,
            condition,
            do_keyword,
            body,
            end,
            ..
        } => {
            punctuation.push(*keyword);
            children.push(*condition);
            punctuation.extend(*do_keyword);
            children.push(*body);
            punctuation.extend(*end);
        }

        NodeKind::Repeat {
            keyword,
            body,
            until,
            condition,
            ..
        } => {
            punctuation.push(*keyword);
            children.push(*body);
            punctuation.extend(*until);
            children.push(*condition);
        }

        NodeKind::Do {
            keyword, body, end, ..
        } => {
            punctuation.push(*keyword);
            children.push(*body);
            punctuation.extend(*end);
        }

        _ => unreachable!("unexpected syntax family"),
    }
}

fn loops(
    tree: &Tree<'_>,
    kind: &NodeKind,
    children: &mut Vec<NodeIndex>,
    punctuation: &mut Vec<TokenIndex>,
) {
    match kind {
        NodeKind::NumericFor {
            keyword,
            binding,
            assignment,
            start,
            range_separator,
            end,
            step_separator,
            step,
            do_keyword,
            body,
            end_keyword,
            ..
        } => {
            punctuation.push(*keyword);
            children.push(*binding);
            punctuation.extend(*assignment);
            children.push(*start);
            punctuation.extend(*range_separator);
            children.push(*end);
            punctuation.extend(*step_separator);
            children.extend(*step);
            punctuation.extend(*do_keyword);
            children.push(*body);
            punctuation.extend(*end_keyword);
        }

        NodeKind::GenericFor {
            keyword,
            bindings,
            in_keyword,
            values,
            do_keyword,
            body,
            end,
            ..
        } => {
            punctuation.push(*keyword);

            for entry in tree.list(bindings) {
                children.push(entry.node);
                punctuation.extend(entry.separator);
            }

            punctuation.extend(*in_keyword);

            for entry in tree.list(values) {
                children.push(entry.node);
                punctuation.extend(entry.separator);
            }

            punctuation.extend(*do_keyword);
            children.push(*body);
            punctuation.extend(*end);
        }

        _ => unreachable!("unexpected syntax family"),
    }
}

fn declarations(
    tree: &Tree<'_>,
    kind: &NodeKind,
    children: &mut Vec<NodeIndex>,
    punctuation: &mut Vec<TokenIndex>,
) {
    match kind {
        NodeKind::Export {
            attributes,
            keyword,
            declaration,
            ..
        } => {
            children.extend(*attributes);
            punctuation.push(*keyword);
            children.push(*declaration);
        }

        NodeKind::TypeAlias {
            keyword,
            name,
            generics,
            assignment,
            annotation,
            ..
        } => {
            punctuation.push(*keyword);
            children.push(*name);
            children.extend(*generics);
            punctuation.extend(*assignment);
            children.push(*annotation);
        }

        NodeKind::Declaration {
            keyword,
            external,
            declaration,
            ..
        } => {
            punctuation.push(*keyword);
            punctuation.extend(*external);
            children.push(*declaration);
        }

        NodeKind::Class {
            open,
            keyword,
            name,
            extends,
            with,
            members,
            end,
            ..
        } => {
            punctuation.extend(*open);
            punctuation.extend(*keyword);
            children.push(*name);
            children.extend(*extends);
            punctuation.extend(*with);

            for entry in tree.list(members) {
                children.push(entry.node);
                punctuation.extend(entry.separator);
            }

            punctuation.extend(*end);
        }

        NodeKind::Property {
            public, binding, ..
        } => {
            punctuation.push(*public);
            children.push(*binding);
        }

        NodeKind::Extends {
            keyword,
            superclass,
            ..
        } => {
            punctuation.push(*keyword);
            children.push(*superclass);
        }

        _ => unreachable!("unexpected syntax family"),
    }
}

fn attributes(
    tree: &Tree<'_>,
    kind: &NodeKind,
    children: &mut Vec<NodeIndex>,
    punctuation: &mut Vec<TokenIndex>,
) {
    match kind {
        NodeKind::Attributes { attributes, .. } => {
            for entry in tree.list(attributes) {
                children.push(entry.node);
                punctuation.extend(entry.separator);
            }
        }

        NodeKind::AttributeGroup {
            opening,
            attributes,
            closing,
            ..
        } => {
            punctuation.push(*opening);

            for entry in tree.list(attributes) {
                children.push(entry.node);
                punctuation.extend(entry.separator);
            }

            punctuation.extend(*closing);
        }

        NodeKind::Attribute {
            name, arguments, ..
        } => {
            children.push(*name);
            children.extend(*arguments);
        }

        NodeKind::Arguments {
            opening,
            values,
            closing,
            ..
        } => {
            punctuation.extend(*opening);

            for entry in tree.list(values) {
                children.push(entry.node);
                punctuation.extend(entry.separator);
            }

            punctuation.extend(*closing);
        }

        NodeKind::Generics {
            opening,
            parameters,
            closing,
            ..
        } => {
            punctuation.push(*opening);

            for entry in tree.list(parameters) {
                children.push(entry.node);
                punctuation.extend(entry.separator);
            }

            punctuation.extend(*closing);
        }

        NodeKind::Generic {
            name,
            ellipsis,
            assignment,
            default,
            ..
        } => {
            children.push(*name);
            punctuation.extend(*ellipsis);
            punctuation.extend(*assignment);
            children.extend(*default);
        }

        _ => unreachable!("unexpected syntax family"),
    }
}

fn expressions(
    _tree: &Tree<'_>,
    kind: &NodeKind,
    children: &mut Vec<NodeIndex>,
    punctuation: &mut Vec<TokenIndex>,
) {
    match kind {
        NodeKind::Unary {
            operator, operand, ..
        } => {
            punctuation.push(*operator);
            children.push(*operand);
        }

        NodeKind::Binary {
            left,
            operator,
            right,
            ..
        } => {
            children.push(*left);
            punctuation.push(*operator);
            children.push(*right);
        }

        NodeKind::Group {
            opening,
            expression,
            closing,
            ..
        } => {
            punctuation.push(*opening);
            children.push(*expression);
            punctuation.extend(*closing);
        }

        NodeKind::Call {
            callee, arguments, ..
        } => {
            children.push(*callee);
            children.push(*arguments);
        }

        NodeKind::MethodCall {
            receiver,
            colon,
            method,
            instantiation,
            arguments,
            ..
        } => {
            children.push(*receiver);
            punctuation.push(*colon);
            children.push(*method);
            children.extend(*instantiation);
            children.push(*arguments);
        }

        NodeKind::Field {
            receiver,
            dot,
            name,
            ..
        } => {
            children.push(*receiver);
            punctuation.push(*dot);
            children.push(*name);
        }

        NodeKind::Index {
            receiver,
            opening,
            key,
            closing,
            ..
        } => {
            children.push(*receiver);
            punctuation.push(*opening);
            children.push(*key);
            punctuation.extend(*closing);
        }

        _ => unreachable!("unexpected syntax family"),
    }
}

fn constructors(
    tree: &Tree<'_>,
    kind: &NodeKind,
    children: &mut Vec<NodeIndex>,
    punctuation: &mut Vec<TokenIndex>,
) {
    match kind {
        NodeKind::Instantiate {
            expression,
            arguments,
            ..
        } => {
            children.push(*expression);
            children.push(*arguments);
        }

        NodeKind::InstantiationArguments {
            opening,
            arguments,
            closing,
            ..
        } => {
            punctuation.push(*opening);
            children.push(*arguments);
            punctuation.extend(*closing);
        }

        NodeKind::Assertion {
            expression,
            operator,
            annotation,
            ..
        } => {
            children.push(*expression);
            punctuation.push(*operator);
            children.push(*annotation);
        }

        NodeKind::Conditional {
            keyword,
            condition,
            then,
            truthy,
            else_keyword,
            falsy,
            ..
        } => {
            punctuation.push(*keyword);
            children.push(*condition);
            punctuation.extend(*then);
            children.push(*truthy);
            punctuation.extend(*else_keyword);
            children.push(*falsy);
        }

        NodeKind::Interpolation { segments, .. } => {
            for entry in tree.list(segments) {
                children.push(entry.node);
                punctuation.extend(entry.separator);
            }
        }

        NodeKind::Table {
            opening,
            fields,
            closing,
            ..
        } => {
            punctuation.push(*opening);

            for entry in tree.list(fields) {
                children.push(entry.node);
                punctuation.extend(entry.separator);
            }

            punctuation.extend(*closing);
        }

        NodeKind::TableField {
            opening,
            key,
            closing,
            assignment,
            value,
            ..
        } => {
            punctuation.extend(*opening);
            children.extend(*key);
            punctuation.extend(*closing);
            punctuation.extend(*assignment);
            children.push(*value);
        }

        _ => unreachable!("unexpected syntax family"),
    }
}

fn references(
    tree: &Tree<'_>,
    kind: &NodeKind,
    children: &mut Vec<NodeIndex>,
    punctuation: &mut Vec<TokenIndex>,
) {
    match kind {
        NodeKind::TypeName {
            namespace,
            dot,
            name,
            arguments,
            ..
        } => {
            children.extend(*namespace);
            punctuation.extend(*dot);
            children.push(*name);
            children.extend(*arguments);
        }

        NodeKind::TypeTable {
            opening,
            access,
            element,
            fields,
            closing,
            ..
        } => {
            punctuation.push(*opening);
            punctuation.extend(*access);
            children.extend(*element);

            for entry in tree.list(fields) {
                children.push(entry.node);
                punctuation.extend(entry.separator);
            }

            punctuation.extend(*closing);
        }

        NodeKind::TypeField {
            access,
            opening,
            key,
            closing,
            colon,
            annotation,
            ..
        } => {
            punctuation.extend(*access);
            punctuation.extend(*opening);
            children.push(*key);
            punctuation.extend(*closing);
            punctuation.extend(*colon);
            children.push(*annotation);
        }

        NodeKind::TypeIndexer {
            access,
            opening,
            key,
            closing,
            colon,
            annotation,
            ..
        } => {
            punctuation.extend(*access);
            punctuation.push(*opening);
            children.push(*key);
            punctuation.extend(*closing);
            punctuation.extend(*colon);
            children.push(*annotation);
        }

        NodeKind::TypeFunction {
            attributes,
            generics,
            parameters,
            arrow,
            returns,
            ..
        } => {
            children.extend(*attributes);
            children.extend(*generics);
            children.push(*parameters);
            punctuation.extend(*arrow);
            children.push(*returns);
        }

        _ => unreachable!("unexpected syntax family"),
    }
}

fn annotations(
    tree: &Tree<'_>,
    kind: &NodeKind,
    children: &mut Vec<NodeIndex>,
    punctuation: &mut Vec<TokenIndex>,
) {
    match kind {
        NodeKind::TypeGroup {
            opening,
            annotation,
            closing,
            ..
        } => {
            punctuation.push(*opening);
            children.push(*annotation);
            punctuation.extend(*closing);
        }

        NodeKind::TypePack {
            opening,
            types,
            closing,
            ..
        } => {
            punctuation.extend(*opening);

            for entry in tree.list(types) {
                children.push(entry.node);
                punctuation.extend(entry.separator);
            }

            punctuation.extend(*closing);
        }

        NodeKind::GenericPack { name, ellipsis, .. } => {
            children.push(*name);
            punctuation.extend(*ellipsis);
        }

        NodeKind::VariadicType {
            ellipsis,
            annotation,
            ..
        } => {
            punctuation.push(*ellipsis);
            children.push(*annotation);
        }

        NodeKind::TypeParameter {
            name,
            colon,
            annotation,
            ..
        } => {
            children.push(*name);
            punctuation.push(*colon);
            children.push(*annotation);
        }

        NodeKind::TypeArguments {
            opening,
            arguments,
            closing,
            ..
        } => {
            punctuation.push(*opening);

            for entry in tree.list(arguments) {
                children.push(entry.node);
                punctuation.extend(entry.separator);
            }

            punctuation.extend(*closing);
        }

        NodeKind::TypeUnion {
            left,
            operator,
            right,
            ..
        }
        | NodeKind::TypeIntersection {
            left,
            operator,
            right,
            ..
        } => {
            children.extend(*left);
            punctuation.push(*operator);
            children.push(*right);
        }

        NodeKind::TypeOptional {
            annotation,
            question_mark,
            ..
        } => {
            children.push(*annotation);
            punctuation.push(*question_mark);
        }

        NodeKind::TypeOf {
            keyword,
            opening,
            expression,
            closing,
            ..
        } => {
            punctuation.push(*keyword);
            punctuation.extend(*opening);
            children.push(*expression);
            punctuation.extend(*closing);
        }

        _ => unreachable!("unexpected syntax family"),
    }
}
