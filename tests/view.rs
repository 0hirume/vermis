use vermis::{Kind, Parts, Tree, View, parse, parse_luaux};

fn check(source: &[u8]) -> Tree {
    let tree = parse(source);
    assert_eq!(tree.source(), source);
    assert_eq!(tree.root().text(), source);
    assert_eq!(tree.root().parent(), None);

    for view in tree.root().descendants() {
        let span = view.span();
        assert!(span.start <= span.end && span.end <= source.len());
        assert_eq!(view.text(), span.bytes(tree.source()));

        assert!(
            view.parts().is_some(),
            "{:?}: {:?}",
            view.kind(),
            view.text()
        );

        if view != tree.root() {
            let parent = view.parent().unwrap();
            assert_eq!(parent.children().filter(|child| *child == view).count(), 1);
        }

        let mut end = span.start;

        for child in view.children() {
            assert_eq!(child.parent(), Some(view));
            assert!(child.span().start >= end && child.span().end <= span.end);
            end = child.span().end;
        }
    }

    tree
}

fn first(tree: &Tree, kind: Kind) -> View<'_> {
    tree.root()
        .descendants()
        .find(|node| node.kind() == kind)
        .unwrap()
}

#[test]
fn markup_views() {
    let tree = parse_luaux(b"return <Components.Frame Enabled Text='literal' Size={size} {props} ={props.Name}>before<>{child + offset}<Button/><!-- note -->{--[[ note ]]}</>after</Components.Frame>");
    assert!(tree.diagnostics().is_empty(), "{:?}", tree.diagnostics());

    for kind in [
        Kind::Element,
        Kind::Fragment,
        Kind::Opening,
        Kind::Closing,
        Kind::MarkupName,
        Kind::MarkupAttributes,
        Kind::MarkupAttribute,
        Kind::MarkupSpread,
        Kind::MarkupInferred,
        Kind::MarkupChildren,
        Kind::MarkupExpression,
        Kind::MarkupText,
        Kind::MarkupComment,
    ] {
        assert!(first(&tree, kind).parts().is_some(), "{kind:?}");
    }

    let element = first(&tree, Kind::Element);

    let Parts::Markup {
        opening,
        children,
        closing,
    } = element.parts().unwrap()
    else {
        panic!()
    };

    assert!(children.is_some());
    assert_eq!(closing.unwrap().text(), b"</Components.Frame>");

    let Parts::Tag { name, attributes } = opening.parts().unwrap() else {
        panic!()
    };

    let Parts::MarkupName { segments } = name.unwrap().parts().unwrap() else {
        panic!()
    };

    assert_eq!(
        segments.map(View::text).collect::<Vec<_>>(),
        [b"Components".as_slice(), b"Frame".as_slice()]
    );

    let Parts::MarkupAttributes { mut attributes } = attributes.unwrap().parts().unwrap() else {
        panic!()
    };

    let Parts::MarkupAttribute { name, value } = attributes.next().unwrap().parts().unwrap() else {
        panic!()
    };

    assert_eq!(name.text(), b"Enabled");
    assert!(value.is_none());

    let Parts::MarkupAttribute { value, .. } = attributes.next().unwrap().parts().unwrap() else {
        panic!()
    };

    assert_eq!(value.unwrap().text(), b"'literal'");

    let Parts::MarkupExpression { expression } =
        first(&tree, Kind::MarkupExpression).parts().unwrap()
    else {
        panic!()
    };

    assert_eq!(expression.kind(), Kind::Name);

    let button = tree
        .root()
        .descendants()
        .find(|node| node.kind() == Kind::Element && node.text() == b"<Button/>")
        .unwrap();

    let Parts::Markup {
        children, closing, ..
    } = button.parts().unwrap()
    else {
        panic!()
    };

    assert!(children.is_none());
    assert!(closing.is_none());
}

#[test]
fn named_statements_and_expressions() {
    let tree = check(b"@native export function identity<T>(value: T): T local copy = value + 1 copy += 2 return copy end");
    assert!(tree.diagnostics().is_empty(), "{:?}", tree.diagnostics());

    let Parts::Root { block } = tree.root().parts().unwrap() else {
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
    assert!(tree.diagnostics().is_empty(), "{:?}", tree.diagnostics());

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
            tree.diagnostics().is_empty(),
            "{source}: {:?}",
            tree.diagnostics()
        );

        seen.extend(tree.root().descendants().map(View::kind));
    }

    seen.extend(
        check(b"local value =\n!")
            .root()
            .descendants()
            .map(View::kind),
    );

    for kind in KINDS {
        assert!(seen.contains(kind), "missing {kind:?} coverage");
    }
}

const KINDS: &[Kind] = &[
    Kind::Root,
    Kind::Block,
    Kind::Error,
    Kind::Missing,
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
            assert!(tree.diagnostics().is_empty(), "{:?}", tree.diagnostics());
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
fn borrowed_children_and_recovery() {
    let tree = check(b"return first, second, third");
    let statement = first(&tree, Kind::Return);
    let mut children = statement.children();
    let saved = children.clone();
    assert_eq!(children.len(), 3);
    assert_eq!(children.next().unwrap().text(), b"first");
    assert_eq!(children.len(), 2);
    assert_eq!(children.next_back().unwrap().text(), b"third");
    assert_eq!(children.next().unwrap().text(), b"second");
    assert!(children.next().is_none() && children.next_back().is_none());
    assert_eq!(saved.count(), 3);

    let children: Vec<_> = statement.children().collect();
    assert_eq!(children[0].previous_sibling(), None);
    assert_eq!(children[0].next_sibling(), Some(children[1]));
    assert_eq!(children[1].previous_sibling(), Some(children[0]));
    assert_eq!(children[1].next_sibling(), Some(children[2]));
    assert_eq!(children[2].previous_sibling(), Some(children[1]));
    assert_eq!(children[2].next_sibling(), None);

    assert!(
        children
            .iter()
            .all(|child| child.parent() == Some(statement))
    );

    assert_eq!(
        statement.descendants().collect::<Vec<_>>(),
        [statement, children[0], children[1], children[2]]
    );

    assert_eq!(children[1].ancestors().next(), Some(children[1]));
    assert_eq!(children[1].ancestors().last(), Some(tree.root()));

    for source in [
        b"return first +".as_slice(),
        b"local broken = )\nlocal valid = 2\nreturn valid",
    ] {
        let tree = check(source);
        assert_ne!(tree.diagnostics(), []);

        assert_eq!(
            tree.root()
                .tokens()
                .flat_map(|token| token.text().iter().copied())
                .collect::<Vec<_>>(),
            source
        );
    }
}
