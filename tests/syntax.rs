//! Named syntax field tests.

pub mod support;

use support::{check, first};
use vermis::tree::NodeKind;

#[test]
fn named_statements_and_expressions() {
    let tree = check(b"@native export function identity<T>(value: T): T local copy = value + 1 copy += 2 return copy end");
    assert!(tree.diagnostics.is_empty(), "{:?}", tree.diagnostics);

    let NodeKind::Root { block, .. } = tree.node(tree.root).kind else {
        panic!()
    };

    let NodeKind::Block { statements } = &tree.node(block).kind else {
        panic!()
    };

    assert_eq!(tree.list(statements).len(), 1);

    let NodeKind::Export {
        attributes,
        declaration,
        ..
    } = tree.node(tree.list(statements)[0].node).kind
    else {
        panic!()
    };

    assert_eq!(tree.text(attributes.unwrap()), b"@native");

    let NodeKind::Function {
        name,
        generics,
        parameters,
        returns,
        body,
        ..
    } = tree.node(declaration).kind
    else {
        panic!()
    };

    assert_eq!(tree.text(name.unwrap()), b"identity");
    assert_eq!(tree.text(generics.unwrap()), b"<T>");
    assert_eq!(tree.text(parameters), b"(value: T)");

    let NodeKind::Returns { annotation, .. } = tree.node(returns.unwrap()).kind else {
        panic!()
    };

    assert_eq!(tree.text(annotation), b"T");

    let NodeKind::Block { statements } = &tree.node(body.unwrap()).kind else {
        panic!()
    };

    assert_eq!(tree.list(statements).len(), 3);
    let local = first(&tree, |kind| matches!(kind, NodeKind::Local { .. }));

    let NodeKind::Local {
        bindings, values, ..
    } = &tree.node(local).kind
    else {
        panic!()
    };

    assert_eq!(tree.list(bindings).len(), 1);

    let NodeKind::Binding {
        name, annotation, ..
    } = tree.node(tree.list(bindings)[0].node).kind
    else {
        panic!()
    };

    assert_eq!(tree.text(name), b"copy");
    assert!(annotation.is_none());
    assert_eq!(tree.list(values).len(), 1);

    let NodeKind::Binary {
        left,
        operator,
        right,
    } = tree.node(tree.list(values)[0].node).kind
    else {
        panic!()
    };

    assert_eq!(tree.text(left), b"value");
    assert_eq!(tree.token(operator).bytes(tree.source), b"+");
    assert_eq!(tree.text(right), b"1");

    let assignment = first(&tree, |kind| {
        matches!(kind, NodeKind::CompoundAssignment { .. })
    });

    let NodeKind::CompoundAssignment {
        target,
        operator,
        value,
    } = tree.node(assignment).kind
    else {
        panic!()
    };

    assert_eq!(tree.text(target), b"copy");
    assert_eq!(tree.token(operator).bytes(tree.source), b"+=");
    assert_eq!(tree.text(value), b"2");
}

#[test]
fn named_types_and_calls() {
    let tree = check(b"type Result<T = number, Values... = ()> = {read value: T?, callback: (T) -> (T, Values...)} local result = object:method<<namespace.Result<>>>(1) :: Result<number>");
    assert!(tree.diagnostics.is_empty(), "{:?}", tree.diagnostics);
    let alias = first(&tree, |kind| matches!(kind, NodeKind::TypeAlias { .. }));

    let NodeKind::TypeAlias {
        name,
        generics,
        annotation,
        ..
    } = tree.node(alias).kind
    else {
        panic!()
    };

    assert_eq!(tree.text(name), b"Result");

    let NodeKind::Generics { parameters, .. } = &tree.node(generics.unwrap()).kind else {
        panic!()
    };

    assert_eq!(tree.list(parameters).len(), 2);

    let NodeKind::TypeTable {
        access,
        element,
        fields,
        ..
    } = &tree.node(annotation).kind
    else {
        panic!()
    };

    assert!(access.is_none() && element.is_none());
    let fields = tree.list(fields);
    assert_eq!(fields.len(), 2);

    let NodeKind::TypeField {
        access,
        key,
        annotation,
        ..
    } = tree.node(fields[0].node).kind
    else {
        panic!()
    };

    assert_eq!(tree.token(access.unwrap()).bytes(tree.source), b"read");
    assert_eq!(tree.text(key), b"value");

    let NodeKind::TypeOptional { annotation, .. } = tree.node(annotation).kind else {
        panic!()
    };

    assert_eq!(tree.text(annotation), b"T");

    let NodeKind::TypeField { annotation, .. } = tree.node(fields[1].node).kind else {
        panic!()
    };

    let NodeKind::TypeFunction {
        parameters,
        returns,
        ..
    } = tree.node(annotation).kind
    else {
        panic!()
    };

    assert_eq!(tree.text(parameters), b"(T)");
    assert_eq!(tree.text(returns), b"(T, Values...)");
    let call = first(&tree, |kind| matches!(kind, NodeKind::MethodCall { .. }));

    let NodeKind::MethodCall {
        receiver,
        method,
        instantiation,
        arguments,
        ..
    } = tree.node(call).kind
    else {
        panic!()
    };

    assert_eq!(tree.text(receiver), b"object");
    assert_eq!(tree.text(method), b"method");
    assert_eq!(tree.text(arguments), b"(1)");

    let NodeKind::InstantiationArguments { arguments, .. } = tree.node(instantiation.unwrap()).kind
    else {
        panic!()
    };

    let NodeKind::TypeArguments { arguments, .. } = &tree.node(arguments).kind else {
        panic!()
    };

    let NodeKind::TypeName {
        namespace,
        name,
        arguments,
        ..
    } = tree.node(tree.list(arguments)[0].node).kind
    else {
        panic!()
    };

    assert_eq!(tree.text(namespace.unwrap()), b"namespace");
    assert_eq!(tree.text(name), b"Result");

    let NodeKind::TypeArguments { arguments, .. } = &tree.node(arguments.unwrap()).kind else {
        panic!()
    };

    assert_eq!(tree.list(arguments), []);
}

const SOURCES: &[&str] = &[
    "local first, second: number = 1, 2 first, second = second, first first += 1",
    "@native function namespace.object:method<T>(value: T, ...: string): T return value end",
    "local function local_function() end const function constant_function() end",
    "if local value = input then call() elseif other then call() else call() end",
    "while ready do break end repeat call() until ready for index = 1, 2, 1 do continue end",
    "for key, value in pairs(values) do do call() end end",
    "export const answer = 42 export function identity() end export type Value = number",
    "type function identity(value) return value end declare function identity(value: number): number",
    "declare value: @checked <T>(T) -> T declare extern type Object with @checked function get(self): number end",
    "open class Object extends namespace.Parent public value: number public function get(self) return self.value end end",
    "type Value<T = number, Rest... = (string)> = {read value: T, [string]: (T) -> (T, Rest...)}",
    "type Value = <T>(value: T) -> ...T type Union = | number | string type Intersection = & First & Second",
    "type Value = {read number} type Group = (number)? type Reference = typeof(object.field)",
    "local value = (identity<<number>>(1) :: number) + -other local item = object[key]",
    "local value = if ready then true else false local missing = nil local tail = ...",
    "local message = `value {value}` local items = {name = value, [key] = value, value}",
    "local identity = @[deprecated {reason = 'old'}] function() end object:method<<number>>(1)",
];

#[test]
fn every_kind_has_named_fields() {
    let mut seen = Vec::new();

    for source in SOURCES {
        let tree = check(source.as_bytes());

        assert!(
            tree.diagnostics.is_empty(),
            "{source}: {:?}",
            tree.diagnostics
        );

        seen.extend(tree.nodes.into_iter().map(|node| node.kind));
    }

    for source in [b"local value =".as_slice(), b"local value =\n!"] {
        seen.extend(check(source).nodes.into_iter().map(|node| node.kind));
    }

    macro_rules! covered {
        ($($variant:ident),+) => {
            $(assert!(seen.iter().any(|kind| matches!(kind, NodeKind::$variant { .. })), "missing {} coverage", stringify!($variant));)+
        };
    }

    covered!(
        Root,
        Block,
        Missing,
        Name,
        Number,
        String,
        Boolean,
        Nil,
        Variadic,
        Local,
        Constant,
        Assignment,
        CompoundAssignment,
        CallStatement,
        Function,
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
        Declaration,
        Class,
        Property,
        Extends,
        Attributes,
        AttributeGroup,
        Attribute,
        Arguments,
        Generics,
        Generic,
        Unary,
        Binary,
        Group,
        Call,
        MethodCall,
        Field,
        Index,
        Instantiate,
        InstantiationArguments,
        Assertion,
        Conditional,
        Interpolation,
        Table,
        TableField,
        TypeName,
        TypeTable,
        TypeField,
        TypeIndexer,
        TypeFunction,
        TypeGroup,
        TypePack,
        GenericPack,
        VariadicType,
        TypeParameter,
        TypeArguments,
        TypeUnion,
        TypeIntersection,
        TypeOptional,
        TypeOf,
        Error
    );
}

#[test]
fn corpus_and_recovery() {
    for entry in std::fs::read_dir("vendor/luau/tests/conformance").unwrap() {
        let path = entry.unwrap().path();

        if path
            .extension()
            .is_some_and(|extension| extension == "lua" || extension == "luau")
        {
            let source = std::fs::read(path).unwrap();
            let tree = check(&source);
            assert!(tree.diagnostics.is_empty(), "{:?}", tree.diagnostics);
        }
    }

    for source in [
        b"local = 1\nreturn 2".as_slice(),
        b"return '\xff'",
        b"function unfinished()",
        b"\0\xff\xfe",
    ] {
        check(source);
    }
}

#[test]
fn borrowed_lists_and_recovery() {
    let tree = check(b"return first, second, third");
    let statement = first(&tree, |kind| matches!(kind, NodeKind::Return { .. }));

    let NodeKind::Return { values, .. } = &tree.node(statement).kind else {
        panic!()
    };

    let values = tree.list(values);
    assert_eq!(values.len(), 3);

    assert_eq!(
        values
            .iter()
            .map(|entry| tree.text(entry.node))
            .collect::<Vec<_>>(),
        [
            b"first".as_slice(),
            b"second".as_slice(),
            b"third".as_slice()
        ]
    );

    assert!(
        values[..2]
            .iter()
            .all(|entry| tree.token(entry.separator.unwrap()).bytes(tree.source) == b",")
    );

    assert!(values[2].separator.is_none());

    for source in [
        b"return first +".as_slice(),
        b"local broken = )\nlocal valid = 2\nreturn valid",
    ] {
        assert_ne!(check(source).diagnostics, []);
    }
}
