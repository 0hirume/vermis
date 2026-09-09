use bstr::BStr;
use vermis::{Kind, Tree, parse};

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

    for (index, node) in tree.nodes.iter().enumerate() {
        assert!(node.span.start <= node.span.end && node.span.end <= source.len());
        let mut end = node.span.start;

        for child in &node.children {
            assert!(*child < index);
            let span = tree.nodes[*child].span;
            assert!(span.start >= end && span.end <= node.span.end);
            end = span.end;
            parents[*child] += 1;
        }
    }

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
        assert_eq!(tree.text(binary.children[0]), left.as_bytes());
        assert_eq!(tree.text(binary.children[1]), operator.as_bytes());
        assert_eq!(tree.text(binary.children[2]), right.as_bytes());
    }

    let tree = accepted("return -a ^ 2");
    let unary = tree
        .nodes
        .iter()
        .find(|node| node.kind == Kind::Unary)
        .unwrap();
    assert_eq!(tree.nodes[unary.children[1]].kind, Kind::Binary);
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
    assert_eq!(tree.nodes[call.children[0]].kind, Kind::Instantiate);
}

#[test]
fn malformed_syntax() {
    for source in [
        "local = 1",
        "local value =",
        "local value = f(,)",
        "function f(a,) end",
        "local value = {key = }",
        "type Value = (number, string)",
        "type Value = number | string & boolean",
        "type Value = number & string?",
        "type Value = number? & string",
        "type Value = Box<>",
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
