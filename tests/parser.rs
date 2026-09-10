use bstr::BStr;
use vermis::{Kind, Node, Tree, parse};

fn children<'tree>(tree: &'tree Tree<'_>, node: &Node) -> &'tree [usize] {
    &tree.children[node.children.clone()]
}

fn check(source: &[u8]) -> Tree<'_> {
    let tree = parse(BStr::new(source));

    let restored: Vec<_> = tree
        .tokens
        .iter()
        .flat_map(|token| token.bytes(tree.source).iter().copied())
        .collect();

    assert_eq!(restored, source);
    assert_eq!(tree.text(tree.root), source);

    let mut parents = vec![0; tree.nodes.len()];
    let mut end = 0;

    for (index, node) in tree.nodes.iter().enumerate() {
        assert!(node.span.start <= node.span.end && node.span.end <= source.len());
        assert_eq!(node.children.start, end);
        assert!(node.children.end <= tree.children.len());
        end = node.children.end;
        let mut end = node.span.start;

        for child in children(&tree, node) {
            assert!(*child < index);
            let span = tree.nodes[*child].span;
            assert!(span.start >= end && span.end <= node.span.end);
            end = span.end;
            parents[*child] += 1;
        }
    }

    assert_eq!(end, tree.children.len());
    assert_eq!(parents[tree.root], 0);

    assert!(
        parents
            .iter()
            .enumerate()
            .all(|(index, count)| index == tree.root || *count == 1)
    );

    for error in &tree.diagnostics {
        assert!(error.span.start <= error.span.end && error.span.end <= source.len());
    }

    tree
}

fn accepted(source: &str) -> Tree<'_> {
    let tree = check(source.as_bytes());

    assert!(
        tree.diagnostics.is_empty(),
        "{source:?}: {:?}",
        tree.diagnostics
    );

    tree
}

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

        let binary = tree
            .nodes
            .iter()
            .rfind(|node| node.kind == Kind::Binary)
            .unwrap();

        assert_eq!(tree.text(children(&tree, binary)[0]), left.as_bytes());
        assert_eq!(tree.text(children(&tree, binary)[1]), operator.as_bytes());
        assert_eq!(tree.text(children(&tree, binary)[2]), right.as_bytes());
    }

    let tree = accepted("return -a ^ 2");

    let unary = tree
        .nodes
        .iter()
        .find(|node| node.kind == Kind::Unary)
        .unwrap();

    assert_eq!(tree.nodes[children(&tree, unary)[1]].kind, Kind::Binary);
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
        for (keyword, kind) in [("export", Kind::Export), ("const", Kind::LocalFunction)] {
            let source = format!(
                "{attributes} {keyword} function identity<T>(value: T): T return value end"
            );

            let tree = accepted(&source);
            let block = &tree.nodes[children(&tree, &tree.nodes[tree.root])[0]];
            let statement = children(&tree, block)[0];
            let declaration = &tree.nodes[statement];

            assert_eq!(declaration.kind, kind);
            assert_eq!(tree.text(statement), source.as_bytes());

            assert_eq!(
                tree.nodes[children(&tree, declaration)[0]].kind,
                Kind::Attributes
            );

            assert_eq!(
                tree.text(children(&tree, declaration)[0]),
                attributes.as_bytes()
            );

            let function = if kind == Kind::Export {
                &tree.nodes[children(&tree, declaration)[1]]
            } else {
                declaration
            };

            for kind in [Kind::Generics, Kind::Parameters, Kind::Returns, Kind::Block] {
                assert!(
                    children(&tree, function)
                        .iter()
                        .any(|child| tree.nodes[*child].kind == kind)
                );
            }
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
    for (reference, kind) in [
        ("Parent", Kind::Name),
        ("classes.Parent", Kind::Field),
        ("classes['Parent']", Kind::Index),
        ("classes[select(name)]", Kind::Index),
    ] {
        for prefix in ["class", "open class", "export class", "export open class"] {
            let source = format!("{prefix} Child extends {reference} end");
            let tree = accepted(&source);

            let extends = tree
                .nodes
                .iter()
                .find(|node| node.kind == Kind::Extends)
                .unwrap();

            let superclass = children(&tree, extends)[0];

            assert_eq!(tree.nodes[superclass].kind, kind);
            assert_eq!(tree.text(superclass), reference.as_bytes());
        }
    }

    for attributes in ["@checked", "@[checked, deprecated({reason = 'old'})]"] {
        let source = format!(
            "declare extern type Box extends Parent with {attributes} function get(self, key: string): number end"
        );

        let tree = accepted(&source);

        let method = tree
            .nodes
            .iter()
            .find(|node| node.kind == Kind::Method)
            .unwrap();

        assert_eq!(
            tree.nodes[children(&tree, method)[0]].kind,
            Kind::Attributes
        );

        assert_eq!(tree.text(children(&tree, method)[0]), attributes.as_bytes());
        assert_eq!(tree.text(children(&tree, method)[1]), b"get");
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

        let table = tree
            .nodes
            .iter()
            .find(|node| node.kind == Kind::TypeTable)
            .unwrap();

        assert_eq!(tree.nodes[children(&tree, table)[0]].kind, Kind::Operator);
        assert_eq!(tree.text(children(&tree, table)[0]), access.as_bytes());
        assert_eq!(tree.text(children(&tree, table)[1]), b"number");

        for (field, kind) in [
            ("value: number", Kind::TypeField),
            ("['value']: number", Kind::TypeField),
            ("[string]: number", Kind::TypeIndexer),
        ] {
            let mut sources = vec![format!("type Object = {{{access} {field}}}")];

            if field == "value: number" {
                sources.push(format!(
                    "declare extern type Object with {access} {field} end"
                ));
            }

            for source in sources {
                let tree = accepted(&source);
                let field = tree.nodes.iter().find(|node| node.kind == kind).unwrap();

                assert_eq!(tree.nodes[children(&tree, field)[0]].kind, Kind::Operator);
                assert_eq!(tree.text(children(&tree, field)[0]), access.as_bytes());
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

        let arguments = tree
            .nodes
            .iter()
            .position(|node| node.kind == Kind::TypeArguments)
            .unwrap();

        assert!(tree.nodes[arguments].children.is_empty());
        assert_eq!(tree.text(arguments), b"<>");
    }
}

#[test]
fn exported_functions_are_structured() {
    for source in [
        "export function identity<T>(value: T): T return value end",
        "@native export function identity<T>(value: T): T return value end",
    ] {
        let tree = accepted(source);

        let export = tree
            .nodes
            .iter()
            .find(|node| node.kind == Kind::Export)
            .unwrap();

        let function = &tree.nodes[*children(&tree, export).last().unwrap()];

        assert_eq!(function.kind, Kind::Function);
        assert_eq!(tree.nodes[children(&tree, function)[0]].kind, Kind::Name);
        assert_eq!(tree.text(children(&tree, function)[0]), b"identity");
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

    for kind in [
        Kind::TypeAlias,
        Kind::Generics,
        Kind::TypeTable,
        Kind::TypeField,
        Kind::TypeOptional,
        Kind::TypeFunctionExpression,
        Kind::TypePack,
    ] {
        assert!(
            tree.nodes.iter().any(|node| node.kind == kind),
            "missing {kind:?}"
        );
    }

    let tree = accepted("local value = callback<<number>>(input)");

    let call = tree
        .nodes
        .iter()
        .find(|node| node.kind == Kind::Call)
        .unwrap();

    assert_eq!(tree.nodes[children(&tree, call)[0]].kind, Kind::Instantiate);
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
    assert!(!tree.diagnostics.is_empty());

    assert!(
        tree.nodes.iter().any(|node| node.kind == Kind::Local
            && node.span.bytes(tree.source) == b"local valid = '\xff'")
    );

    assert!(tree.nodes.iter().any(|node| node.kind == Kind::Return));

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
    assert!(!check(deep.as_bytes()).diagnostics.is_empty());

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
        assert!(!check(source.as_bytes()).diagnostics.is_empty());
    }

    accepted(&format!("return value{}", ".field".repeat(1000)));
}
