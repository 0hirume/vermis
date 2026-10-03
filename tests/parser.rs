//! Parser grammar tests.

pub mod support;

use support::{accepted, check, first};
use vermis::tree::NodeKind;

#[test]
fn precedence_and_associativity() {
    for (source, operator, left, right) in [
        ("return a + b * c", "+", "a", "b * c"),
        ("return a - b - c", "-", "a - b", "c"),
        ("return a ^ b ^ c", "^", "a", "b ^ c"),
        ("return a .. b .. c", "..", "a", "b .. c"),
        ("return a or b and c", "or", "a", "b and c"),
    ] {
        let tree = accepted(source);
        let binary = first(&tree, |kind| matches!(kind, NodeKind::Binary { .. }));

        let NodeKind::Binary {
            left: actual_left,
            operator: actual_operator,
            right: actual_right,
        } = tree.node(binary).kind
        else {
            panic!()
        };

        assert_eq!(tree.text(actual_left), left.as_bytes());

        assert_eq!(
            tree.token(actual_operator).bytes(tree.source),
            operator.as_bytes()
        );

        assert_eq!(tree.text(actual_right), right.as_bytes());
    }

    let tree = accepted("return -a ^ 2");
    let unary = first(&tree, |kind| matches!(kind, NodeKind::Unary { .. }));

    let NodeKind::Unary { operand, .. } = tree.node(unary).kind else {
        panic!()
    };

    assert!(matches!(tree.node(operand).kind, NodeKind::Binary { .. }));
}

#[test]
fn condition_bindings() {
    for source in [
        "if local first: number = value then use(first) elseif const second = other then use(second) end",
        "return if local first: number = value then first elseif const second = other then second else fallback",
    ] {
        let tree = accepted(source);

        let bindings: Vec<_> = tree
            .nodes
            .iter()
            .filter_map(|node| {
                let (constant, bindings, values) = match &node.kind {
                    NodeKind::Local {
                        bindings, values, ..
                    } => (false, bindings, values),

                    NodeKind::Constant {
                        bindings, values, ..
                    } => (true, bindings, values),

                    _ => return None,
                };

                assert_eq!(tree.list(bindings).len(), 1);
                assert_eq!(tree.list(values).len(), 1);

                Some((
                    constant,
                    tree.text(tree.list(bindings)[0].node),
                    tree.text(tree.list(values)[0].node),
                ))
            })
            .collect();

        assert_eq!(
            bindings,
            [
                (false, b"first: number".as_slice(), b"value".as_slice()),
                (true, b"second".as_slice(), b"other".as_slice())
            ]
        );
    }

    for source in [
        "if const then use() elseif const.field then use() end",
        "return if const then const elseif const() then true else false",
    ] {
        let tree = accepted(source);

        assert!(
            tree.nodes
                .iter()
                .all(|node| !matches!(node.kind, NodeKind::Constant { .. }))
        );
    }
}

#[test]
fn grammar() {
    for source in [
        "if const ready = value then return ready elseif other then use() else fallback() end",
        "while ready do if stop then break end continue end",
        "repeat local ready = value until ready",
        "for index = 1, limit, step do use(index) end",
        "for key, value in pairs(values) do use(key, value) end",
        "do local first, second: string = 1, 'two' first += 1 end",
        "local function callback<Values...>(...: Values...): Values... return ... end",
        "local result = object.field[index](1, 2):method {name = 'value', [key] = value, true}",
        "local message = `hello {name}, {`nested {other}`}`",
        "local value = if ready then first elseif other then second else third",
        "local callback = @native function(value) return value end",
        "@[deprecated({reason = 'old'})] function old() end",
        "export const answer = 42",
        "export type Value<T> = {value: T}",
        "type function identity(value) return value end",
        "type Result<First = number, Rest... = (string)> = (First, Rest...) -> Rest...",
        "type Value = First<(number), (string)?, ...number>",
        "type Value = {read first: number, write second: string, [string]: number}",
        "type Value = {[\"key\"]: typeof(value.field)}",
        "type Value = <T>(value: T) -> (T, string)",
        "type Value = number | (string & boolean)",
        "type Value = {number}",
        "local value = callback<<number, (string, boolean)>>(input) :: number",
        "declare version: string declare function print(value: string): ()",
        "declare function collect(...: string)",
        "@checked declare function callback(value: string): string",
        "declare callback: @checked (value: string) -> string",
        "@native local function callback() end",
        "local value = object:method<<number>>(1)",
        "type Value = | number | string",
        "type Value = & First & Second",
        "declare extern type Box extends Parent with value: number function get(self): number end",
        "open class Box extends Parent public value: string function get(self): string return self.value end end",
        "local type, class, const, declare, export, continue = 1, 2, 3, 4, 5, 6",
        "type() class() const() declare() export() continue()",
    ] {
        accepted(source);
    }
}

#[test]
fn attributed_declarations() {
    for attributes in [
        "@native",
        "@[native, deprecated({reason = 'old'})]",
        "@[deprecated {reason = 'old'}]",
        "@[native 'message']",
        "@[native [[message]]]",
    ] {
        for keyword in ["export", "const"] {
            let source = format!(
                "{attributes} {keyword} function identity<T>(value: T): T return value end"
            );

            let tree = accepted(&source);
            let function = first(&tree, |kind| matches!(kind, NodeKind::Function { .. }));

            let NodeKind::Function {
                attributes: function_attributes,
                prefix,
                generics,
                parameters,
                returns,
                body,
                ..
            } = tree.node(function).kind
            else {
                panic!()
            };

            let attributes_node = if keyword == "export" {
                let export = first(&tree, |kind| matches!(kind, NodeKind::Export { .. }));

                let NodeKind::Export {
                    attributes,
                    declaration,
                    ..
                } = tree.node(export).kind
                else {
                    panic!()
                };

                assert_eq!(tree.text(export), source.as_bytes());
                assert_eq!(declaration, function);

                attributes.unwrap()
            } else {
                assert_eq!(tree.text(function), source.as_bytes());
                assert_eq!(tree.token(prefix.unwrap()).bytes(tree.source), b"const");

                function_attributes.unwrap()
            };

            assert!(matches!(
                tree.node(attributes_node).kind,
                NodeKind::Attributes { .. }
            ));

            assert_eq!(tree.text(attributes_node), attributes.as_bytes());

            assert!(matches!(
                tree.node(generics.unwrap()).kind,
                NodeKind::Generics { .. }
            ));

            assert!(matches!(
                tree.node(parameters).kind,
                NodeKind::Parameters { .. }
            ));

            assert!(matches!(
                tree.node(returns.unwrap()).kind,
                NodeKind::Returns { .. }
            ));

            assert!(matches!(
                tree.node(body.unwrap()).kind,
                NodeKind::Block { .. }
            ));
        }
    }

    accepted("const function identity<T>(value: T): T return value end");
    accepted("@native local function identity() end");
    accepted("@native function module.identity() end");
    accepted("@checked declare function identity(value: number): number");

    for source in [
        "@native export local value = 1",
        "@native export const value = 1",
        "@native export const function identity() end",
        "@native export local function identity() end",
        "@native export function module.identity() end",
        "@native const value = 1",
        "@native const function module.identity() end",
    ] {
        assert!(
            !check(source.as_bytes()).diagnostics.is_empty(),
            "accepted {source:?}"
        );
    }
}

#[test]
fn classes_are_structured() {
    for reference in [
        "Parent",
        "classes.Parent",
        "classes['Parent']",
        "classes[select(name)]",
    ] {
        for prefix in ["class", "open class", "export class", "export open class"] {
            let source = format!("{prefix} Child extends {reference} end");
            let tree = accepted(&source);
            let extends = first(&tree, |kind| matches!(kind, NodeKind::Extends { .. }));

            let NodeKind::Extends { superclass, .. } = tree.node(extends).kind else {
                panic!()
            };

            assert!(match reference {
                "Parent" => matches!(tree.node(superclass).kind, NodeKind::Name { .. }),
                "classes.Parent" => matches!(tree.node(superclass).kind, NodeKind::Field { .. }),
                _ => matches!(tree.node(superclass).kind, NodeKind::Index { .. }),
            });

            assert_eq!(tree.text(superclass), reference.as_bytes());
        }
    }

    for attributes in ["@checked", "@[checked, deprecated({reason = 'old'})]"] {
        let source = format!(
            "declare extern type Box extends Parent with {attributes} function get(self, key: string): number end"
        );

        let tree = accepted(&source);
        let method = first(&tree, |kind| matches!(kind, NodeKind::Function { .. }));

        let NodeKind::Function {
            attributes: Some(attributes_node),
            name: Some(name),
            ..
        } = tree.node(method).kind
        else {
            panic!()
        };

        assert!(matches!(
            tree.node(attributes_node).kind,
            NodeKind::Attributes { .. }
        ));

        assert_eq!(tree.text(attributes_node), attributes.as_bytes());
        assert_eq!(tree.text(name), b"get");
    }

    accepted("declare extern type Box with public: number read: string write: boolean end");

    for source in [
        "class Child extends classes['Parent' end",
        "class Child extends classes[] end",
        "declare extern type Box with @checked value: number end",
        "declare extern type Box with @checked end",
    ] {
        assert!(
            !check(source.as_bytes()).diagnostics.is_empty(),
            "accepted {source:?}"
        );
    }
}

#[test]
fn access_types_are_structured() {
    for access in ["read", "write"] {
        let source = format!("type Array = {{{access} number}}");
        let tree = accepted(&source);
        let table = first(&tree, |kind| matches!(kind, NodeKind::TypeTable { .. }));

        let NodeKind::TypeTable {
            access: Some(operator),
            element: Some(element),
            ..
        } = tree.node(table).kind
        else {
            panic!()
        };

        assert_eq!(tree.token(operator).bytes(tree.source), access.as_bytes());
        assert_eq!(tree.text(element), b"number");

        for field in ["value: number", "['value']: number", "[string]: number"] {
            let mut sources = vec![format!("type Object = {{{access} {field}}}")];

            if field == "value: number" {
                sources.push(format!(
                    "declare extern type Object with {access} {field} end"
                ));
            }

            for source in sources {
                let tree = accepted(&source);

                let field_node = first(&tree, |kind| {
                    if field == "[string]: number" {
                        matches!(kind, NodeKind::TypeIndexer { .. })
                    } else {
                        matches!(kind, NodeKind::TypeField { .. })
                    }
                });

                let (NodeKind::TypeField {
                    access: Some(operator),
                    ..
                }
                | NodeKind::TypeIndexer {
                    access: Some(operator),
                    ..
                }) = tree.node(field_node).kind
                else {
                    panic!()
                };

                assert_eq!(tree.token(operator).bytes(tree.source), access.as_bytes());
            }
        }
    }

    for source in [
        "type Array = {read (number | string)}",
        "type Array = {write {number}}",
        "type Object = {read: number, write: string}",
        "type Object = {read value: number, write [string]: string}",
        "type Object = {[ [[key]] ]: number}",
        "declare extern type Object with ['value']: number [string]: number end",
    ] {
        accepted(source);
    }
}

#[test]
fn enabled_syntax() {
    for source in [
        "@debugnoinline function identity(value) return value end",
        "local identity = @[deprecated {reason = 'old'}] function(value) return value end",
        "declare identity: @[checked 'message'] (number) -> number",
        "declare extern type Box with @[deprecated {reason = 'old'}] function get(self): number end",
        "export local first, second = 1, 2",
        "export const first, second = 1, 2",
        "export function identity<T>(value: T): T return value end",
        "export type function identity(value) return value end",
        "declare class: {new: (number) -> number}",
        "local namespace = {} type Value = namespace.Value<number>",
        "if local first: number = value then use(first) elseif const second = other then use(second) else fallback() end",
        "function identity(): (number)? return nil end",
        "function identity(): (number) | string return 1 end",
        "function identity(): (number, ...string) return 1 end",
        "function identity<Values...>(...: Values...): Values... return ... end",
        "declare functions: {identity: @checked <T>(value: T) -> T}",
        "local integers = {0i, 9_223_372_036_854_775_807i, 0xffffffffffffffffi, 0b1111i}",
        "local identity = source<<number, (string, boolean), ...number>>",
        "local value = source:identity<<number>>(1)",
    ] {
        accepted(source);
    }

    for source in ["return 0b0b1", "return 0b0B1i"] {
        assert!(
            !check(source.as_bytes()).diagnostics.is_empty(),
            "accepted {source:?}"
        );
    }
}

#[test]
fn empty_type_arguments() {
    for source in [
        "type Value = Box<>",
        "type Value = namespace.Box<>",
        "local value: Box<>",
        "identity<<>>()",
        "local identity = original<<>>",
        "object:identity<<>>()",
    ] {
        let tree = accepted(source);
        let arguments_node = first(&tree, |kind| matches!(kind, NodeKind::TypeArguments { .. }));

        let NodeKind::TypeArguments { arguments, .. } = &tree.node(arguments_node).kind else {
            panic!()
        };

        assert_eq!(tree.list(arguments), []);
        assert_eq!(tree.text(arguments_node), b"<>");
    }
}

#[test]
fn exported_functions_are_structured() {
    for source in [
        "export function identity<T>(value: T): T return value end",
        "@native export function identity<T>(value: T): T return value end",
    ] {
        let tree = accepted(source);
        let export = first(&tree, |kind| matches!(kind, NodeKind::Export { .. }));

        let NodeKind::Export { declaration, .. } = tree.node(export).kind else {
            panic!()
        };

        let NodeKind::Function {
            name: Some(name), ..
        } = tree.node(declaration).kind
        else {
            panic!()
        };

        assert!(matches!(tree.node(name).kind, NodeKind::Name { .. }));
        assert_eq!(tree.text(name), b"identity");
    }
}

#[test]
fn declaration_type_context() {
    for source in [
        "declare identity: @checked (number) -> number",
        "declare library: {nested: {identity: @checked (number) -> number}}",
        "declare library: {read identity: @checked (number) -> number}",
        "declare identity: @checked () -> () | nil",
    ] {
        accepted(source);
    }

    for source in [
        "type Identity = @checked () -> ()",
        "local identity: @checked () -> ()",
        "declare identity: nil | @checked () -> ()",
        "declare identity: | @checked () -> ()",
        "declare identity: (@checked () -> ())",
        "declare library: {[@checked () -> ()]: number}",
        "declare library: {['identity']: @checked () -> ()}",
        "declare library: {@checked () -> ()}",
        "declare library: Box<@checked () -> ()>",
    ] {
        assert!(
            !check(source.as_bytes()).diagnostics.is_empty(),
            "accepted {source:?}"
        );
    }
}

#[test]
fn types_are_structured() {
    let tree = accepted("type Value<T> = {read value: T?, callback: (T) -> (T, string)}");
    first(&tree, |kind| matches!(kind, NodeKind::TypeAlias { .. }));
    first(&tree, |kind| matches!(kind, NodeKind::Generics { .. }));
    first(&tree, |kind| matches!(kind, NodeKind::TypeTable { .. }));
    first(&tree, |kind| matches!(kind, NodeKind::TypeField { .. }));
    first(&tree, |kind| matches!(kind, NodeKind::TypeOptional { .. }));
    first(&tree, |kind| matches!(kind, NodeKind::TypeFunction { .. }));
    first(&tree, |kind| matches!(kind, NodeKind::TypePack { .. }));
    let tree = accepted("local value = callback<<number>>(input)");
    let call = first(&tree, |kind| matches!(kind, NodeKind::Call { .. }));

    let NodeKind::Call { callee, .. } = tree.node(call).kind else {
        panic!()
    };

    assert!(matches!(
        tree.node(callee).kind,
        NodeKind::Instantiate { .. }
    ));
}

#[test]
fn numeric_literals() {
    for literal in [
        "123",
        "1_2_3",
        "1.25",
        "1_2.5_0",
        "1e+3",
        "1_e+3",
        "0xff",
        "0x_f_f",
        "0b1010",
        "0b_10_10",
        "123i",
        "1_2_3i",
        "0xffffffffffffffffi",
        "0x_ffff_ffff_ffff_ffffi",
        "0b1010i",
        "0b_10_10i",
    ] {
        accepted(&format!("return {literal}"));
    }

    for literal in [
        "1e",
        "1_e",
        "0xg",
        "0x_g",
        "0b2",
        "0b_2",
        "1.0i",
        "1_._0i",
        "9223372036854775808i",
        "9_223_372_036_854_775_808i",
        "0x10000000000000000i",
        "0x1_0000_0000_0000_0000i",
    ] {
        let source = format!("return {literal}");

        assert!(
            !check(source.as_bytes()).diagnostics.is_empty(),
            "accepted {literal}"
        );
    }
}

#[test]
fn malformed_syntax() {
    for source in [
        ";",
        "identity();;identity()",
        "export function library.identity() end",
        "export function library:identity() end",
        "declare extern type Object with function identity<Value>(self) end",
        "declare extern type Object with @checked function identity<Value>(self) end",
        "type Value = {read write value: number}",
        "type Value = {first: number, read write second: number}",
        "type Value = Box<number,>",
        "identity<<number,>>()",
        "type Value<> = number",
        "function identity<>() end",
        "local = 1",
        "local value =",
        "local value = f(,)",
        "function f(a,) end",
        "local value = {key = }",
        "type Value = (number, string)",
        "type Value = number | string & boolean",
        "type Value = number & string?",
        "type Value = number? & string",
        "type Value<T..., U> = T",
        "type Value<Rest... = number> = number",
        "type Value = Box<(number, string) | boolean>",
        "type Value = {key?: string}",
        "return 0x",
        "return 0b2",
        "return 1e",
        "return 1.2i",
        "return '\\xgg'",
        "return '\\256'",
        "return '\\u{}'",
        "return '\\u{110000}'",
        "return (value",
        "if value then",
        "(1) = value",
        "return 1 local value = 2",
    ] {
        let tree = check(source.as_bytes());
        assert!(!tree.diagnostics.is_empty(), "accepted {source:?}");
    }
}

#[test]
fn nesting_boundary() {
    for depth in [255, 256] {
        accepted(&format!("{}{}", "do ".repeat(depth), "end ".repeat(depth)));
    }

    let source = format!("{}{}", "do ".repeat(257), "end ".repeat(257));
    let tree = check(source.as_bytes());

    assert!(
        tree.diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message == "syntax nesting limit exceeded")
    );
}

#[test]
fn recovery_and_bytes() {
    let tree = check(b"local broken = )\nlocal valid = '\xff' -- comment\nreturn valid");
    assert_ne!(tree.diagnostics, []);

    assert!(
        tree.nodes
            .iter()
            .any(|node| matches!(node.kind, NodeKind::Local { .. })
                && node.span.bytes(tree.source) == b"local valid = '\xff'")
    );

    first(&tree, |kind| matches!(kind, NodeKind::Return { .. }));

    for first in u8::MIN..=u8::MAX {
        for second in u8::MIN..=u8::MAX {
            check(&[first, second]);
        }
    }

    for source in [
        "if x then f() else g() end",
        "type Value<T> = (T) -> {value: T}",
        "return `hello {value}`",
    ] {
        for end in 0..=source.len() {
            check(&source.as_bytes()[..end]);
        }
    }

    let deep = format!("return {}value{}", "(".repeat(1000), ")".repeat(1000));
    assert_ne!(check(deep.as_bytes()).diagnostics, []);

    for source in [
        format!("{}{}", "do ".repeat(1001), "end ".repeat(1001)),
        format!(
            "type Value = {}number{}",
            "{field: ".repeat(1000),
            "}".repeat(1000)
        ),
        format!("return {}value", "if ready then value else ".repeat(1000)),
        format!(
            "return {}1{}",
            "function() return ".repeat(1000),
            " end".repeat(1000)
        ),
    ] {
        assert_ne!(check(source.as_bytes()).diagnostics, []);
    }

    accepted(&format!("return value{}", ".field".repeat(1000)));
}
