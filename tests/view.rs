use bstr::BStr;
use vermis::{Kind, Parts, Tree, View, parse};

fn check(source: &[u8]) -> Tree<'_> {
    let tree = parse(BStr::new(source));

    for index in 0..tree.nodes.len() {
        let view = tree.view(index).unwrap();
        assert_eq!(view.index(), index);
        assert_eq!(view.span(), tree.nodes[index].span);
        assert_eq!(view.text(), tree.text(index));

        assert!(
            view.parts().is_some(),
            "{:?}: {:?}",
            view.kind(),
            view.text()
        );

        assert_eq!(
            view.children().map(View::index).collect::<Vec<_>>(),
            tree.children[tree.nodes[index].children.clone()]
        );
    }

    tree
}

fn first<'tree, 'source>(tree: &'tree Tree<'source>, kind: Kind) -> View<'tree, 'source> {
    tree.view(
        tree.nodes
            .iter()
            .position(|node| node.kind == kind)
            .unwrap(),
    )
    .unwrap()
}

#[test]
fn named_statements_and_expressions() {
    let tree = check(b"@native export function identity<T>(value: T): T local copy = value + 1 copy += 2 return copy end");
    assert!(tree.diagnostics.is_empty(), "{:?}", tree.diagnostics);

    let Parts::Root { block } = tree.root_view().unwrap().parts().unwrap() else {
        panic!()
    };

    let Parts::Block { mut statements } = block.parts().unwrap() else {
        panic!()
    };

    let Parts::Export {
        attributes,
        declaration,
    } = statements.next().unwrap().parts().unwrap()
    else {
        panic!()
    };

    assert_eq!(attributes.unwrap().text(), b"@native");
    assert!(statements.next().is_none());

    let Parts::Function {
        name,
        generics,
        parameters,
        returns,
        body,
        ..
    } = declaration.parts().unwrap()
    else {
        panic!()
    };

    assert_eq!(name.unwrap().text(), b"identity");
    assert_eq!(generics.unwrap().text(), b"<T>");
    assert_eq!(parameters.text(), b"(value: T)");
    assert_eq!(returns.unwrap().text(), b"T");
    assert_eq!(body.unwrap().children().count(), 3);

    let Parts::Local {
        mut bindings,
        mut values,
    } = first(&tree, Kind::Local).parts().unwrap()
    else {
        panic!()
    };

    let Parts::Binding { name, annotation } = bindings.next().unwrap().parts().unwrap() else {
        panic!()
    };

    assert_eq!(name.text(), b"copy");
    assert!(annotation.is_none());

    let Parts::Binary {
        left,
        operator,
        right,
    } = values.next().unwrap().parts().unwrap()
    else {
        panic!()
    };

    assert_eq!(left.text(), b"value");
    assert_eq!(operator.text(), b"+");
    assert_eq!(right.text(), b"1");
    assert!(values.next().is_none());

    let Parts::Assignment {
        mut targets,
        operator,
        mut values,
    } = first(&tree, Kind::CompoundAssignment).parts().unwrap()
    else {
        panic!()
    };

    assert_eq!(targets.next().unwrap().text(), b"copy");
    assert_eq!(operator.text(), b"+=");
    assert_eq!(values.next().unwrap().text(), b"2");
}

#[test]
fn named_types_and_calls() {
    let tree = check(b"type Result<T = number, Values... = ()> = {read value: T?, callback: (T) -> (T, Values...)} local result = object:method<<namespace.Result<>>>(1) :: Result<number>");
    assert!(tree.diagnostics.is_empty(), "{:?}", tree.diagnostics);

    let Parts::TypeAlias {
        name,
        generics,
        annotation,
    } = first(&tree, Kind::TypeAlias).parts().unwrap()
    else {
        panic!()
    };

    assert_eq!(name.text(), b"Result");
    assert_eq!(generics.unwrap().children().count(), 2);

    let Parts::TypeTable {
        access,
        element,
        mut fields,
    } = annotation.parts().unwrap()
    else {
        panic!()
    };

    assert!(access.is_none() && element.is_none());

    let Parts::TypeField {
        access,
        key,
        annotation,
    } = fields.next().unwrap().parts().unwrap()
    else {
        panic!()
    };

    assert_eq!(access.unwrap().text(), b"read");
    assert_eq!(key.text(), b"value");

    let Parts::TypeOptional { annotation } = annotation.parts().unwrap() else {
        panic!()
    };

    assert_eq!(annotation.text(), b"T");

    let Parts::TypeField { annotation, .. } = fields.next().unwrap().parts().unwrap() else {
        panic!()
    };

    let Parts::TypeFunction {
        parameters,
        returns,
        ..
    } = annotation.parts().unwrap()
    else {
        panic!()
    };

    assert_eq!(parameters.text(), b"(T)");
    assert_eq!(returns.text(), b"(T, Values...)");

    let Parts::MethodCall {
        receiver,
        method,
        types,
        arguments,
    } = first(&tree, Kind::MethodCall).parts().unwrap()
    else {
        panic!()
    };

    assert_eq!(receiver.text(), b"object");
    assert_eq!(method.text(), b"method");
    assert_eq!(arguments.text(), b"(1)");
    let reference = types.unwrap().children().next().unwrap();

    let Parts::TypeName {
        namespace,
        name,
        arguments,
    } = reference.parts().unwrap()
    else {
        panic!()
    };

    assert_eq!(namespace.unwrap().text(), b"namespace");
    assert_eq!(name.text(), b"Result");
    assert_eq!(arguments.unwrap().children().count(), 0);
}

#[test]
fn every_kind_has_a_view() {
    let sources = [
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

    let mut seen = Vec::new();

    for source in sources {
        let tree = check(source.as_bytes());

        assert!(
            tree.diagnostics.is_empty(),
            "{source}: {:?}",
            tree.diagnostics
        );

        seen.extend(tree.nodes.iter().map(|node| node.kind));
    }

    seen.extend(check(b"local =").nodes.iter().map(|node| node.kind));

    for kind in KINDS {
        assert!(seen.contains(kind), "missing {kind:?} coverage");
    }
}

const KINDS: &[Kind] = &[
    Kind::Root,
    Kind::Block,
    Kind::Error,
    Kind::Name,
    Kind::Number,
    Kind::String,
    Kind::Boolean,
    Kind::Nil,
    Kind::Variadic,
    Kind::Operator,
    Kind::Local,
    Kind::Constant,
    Kind::Assignment,
    Kind::CompoundAssignment,
    Kind::CallStatement,
    Kind::Function,
    Kind::LocalFunction,
    Kind::FunctionName,
    Kind::Parameters,
    Kind::Binding,
    Kind::Returns,
    Kind::If,
    Kind::Branch,
    Kind::Else,
    Kind::While,
    Kind::Repeat,
    Kind::NumericFor,
    Kind::GenericFor,
    Kind::Do,
    Kind::Return,
    Kind::Break,
    Kind::Continue,
    Kind::Export,
    Kind::TypeAlias,
    Kind::TypeFunction,
    Kind::Declaration,
    Kind::Class,
    Kind::Property,
    Kind::Method,
    Kind::Extends,
    Kind::Attributes,
    Kind::Attribute,
    Kind::Arguments,
    Kind::Generics,
    Kind::Generic,
    Kind::GenericPack,
    Kind::Unary,
    Kind::Binary,
    Kind::Group,
    Kind::Call,
    Kind::MethodCall,
    Kind::Field,
    Kind::Index,
    Kind::Instantiate,
    Kind::Assertion,
    Kind::Conditional,
    Kind::Interpolation,
    Kind::Table,
    Kind::TableField,
    Kind::TypeName,
    Kind::TypeTable,
    Kind::TypeField,
    Kind::TypeIndexer,
    Kind::TypeFunctionExpression,
    Kind::TypeGroup,
    Kind::TypePack,
    Kind::VariadicType,
    Kind::TypeParameter,
    Kind::TypeArguments,
    Kind::TypeUnion,
    Kind::TypeIntersection,
    Kind::TypeOptional,
    Kind::TypeOf,
];

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
fn borrowed_children_and_invalid_indices() {
    let mut tree = check(b"return first, second, third");
    let mut children = first(&tree, Kind::Return).children();
    let saved = children.clone();
    assert_eq!(children.next().unwrap().text(), b"first");
    assert_eq!(children.next_back().unwrap().text(), b"third");
    assert_eq!(children.next().unwrap().text(), b"second");
    assert!(children.next().is_none() && children.next_back().is_none());
    assert_eq!(saved.count(), 3);
    assert!(tree.view(usize::MAX).is_none());

    let start = tree.nodes[tree.root].children.start;
    tree.children.insert(start, usize::MAX);
    tree.nodes[tree.root].children.end += 1;
    let root = tree.root_view().unwrap();
    assert!(root.parts().is_none());
    assert_eq!(root.children().count(), 1);

    tree.nodes[tree.root].children.end = usize::MAX;
    assert!(tree.root_view().unwrap().parts().is_none());
    assert_eq!(tree.root_view().unwrap().children().count(), 0);

    tree.root = usize::MAX;
    assert!(tree.root_view().is_none());

    let mut tree = parse(BStr::new("return first + second"));
    let binary = first(&tree, Kind::Binary).index();
    tree.nodes[binary].children.end -= 1;
    assert!(tree.view(binary).unwrap().parts().is_none());
}
