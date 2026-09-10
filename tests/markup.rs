use vermis::{Kind, TokenKind, Tree, parse, parse_luaux};

fn check(source: &[u8]) -> Tree<'_> {
    let tree = parse_luaux(source.into());
    let mut end = 0;

    for (index, token) in tree.tokens.iter().enumerate() {
        assert_eq!(token.span.start, end);
        assert!(token.span.end <= source.len());

        if token.kind == TokenKind::Eof {
            assert_eq!(index + 1, tree.tokens.len());
            assert_eq!(token.span.end, source.len());
        } else {
            assert!(token.span.end > token.span.start);
        }

        end = token.span.end;
    }

    assert_eq!(end, source.len());
    assert_eq!(tree.tokens.last().unwrap().kind, TokenKind::Eof);
    let mut parents = vec![0; tree.nodes.len()];
    let mut end = 0;

    for (index, node) in tree.nodes.iter().enumerate() {
        assert!(node.span.start <= node.span.end && node.span.end <= source.len());
        assert_eq!(node.children.start, end);
        end = node.children.end;
        let mut position = node.span.start;

        for child in &tree.children[node.children.clone()] {
            assert!(*child < index);
            let span = tree.nodes[*child].span;
            assert!(span.start >= position && span.end <= node.span.end);
            position = span.end;
            parents[*child] += 1;
        }

        assert!(tree.view(index).unwrap().parts().is_some(), "{node:?}");
    }

    assert_eq!(end, tree.children.len());

    assert!(
        parents
            .iter()
            .enumerate()
            .all(|(index, count)| *count == usize::from(index != tree.root))
    );

    assert_eq!(tree.text(tree.root), source);

    tree
}

#[test]
fn grammar() {
    for markup in [
        "<Frame/>",
        "<Components.Controls.Button />",
        "<Frame Name='name' Size={UDim2.new(1, 0)} Visible />",
        "<Frame ={props.Size} Name=\"name\" ={Visible} />",
        "<Frame ={render().value} />",
        "<Frame Name = \"name\" />",
        "<Frame Size= {props.Size} />",
        "<Frame {props} />",
        "<Frame><TextLabel/><TextButton/></Frame>",
        "<><Frame/><Frame/></>",
        "< > <Frame/> </ >",
        "<TextLabel>Name: {name}</TextLabel>",
        "<TextLabel>\n  Name: {name}\n</TextLabel>",
        "<Frame>\n<!-- note -->\n<TextLabel/>\n</Frame>",
        "<Frame><!-- brackets ]] and quotes ' ` { } --></Frame>",
        "<Frame>{--[[ note ]]}</Frame>",
        "<Frame>{-- note\n}</Frame>",
        "<Frame>{value --[[ note ]]}</Frame>",
        "<Frame Value={render(\"}\")} />",
        "<Frame Value={{nested = {value}}} />",
        "<Frame Value={`hello {name}`} />",
        "<Frame>{enabled and <Button/> or nil}</Frame>",
        "<Frame>{if enabled then <Button/> else <Label/>}</Frame>",
        "<Frame>{function() return <Button/> end}</Frame>",
        "<Frame>{`hello {<Label/>}`}</Frame>",
        "<Frame>don't lex this ' \" ` --[[ as Luau</Frame>",
        r"<Label>escaped \{brace} \` \\ \<angle</Label>",
        r#"<Label Text="line\nline" />"#,
        "<Frame>{[[text } <Frame/>]]}</Frame>",
        "<Frame>{--[=[ } <Frame/> ]=]\nvalue}</Frame>",
    ] {
        let source = format!("local element = {markup}\nreturn element");
        let tree = check(source.as_bytes());

        assert!(
            tree.diagnostics.is_empty(),
            "{source}: {:?}",
            tree.diagnostics
        );

        assert!(
            !parse(source.as_bytes().into()).diagnostics.is_empty(),
            "standard Luau accepted {source}"
        );
    }

    for source in [
        "return `<Frame/>`",
        "return `element {<Frame/>} after {value}`",
        "local element = render(<Frame/>, <Label/>)\nreturn element",
        "return {<Frame/>, element = <Label/>}",
        "return (<Frame/>).field",
        "return <Frame/>.field",
        "return <Frame/>[index]",
        "return <Frame/>:render()",
        "return <Frame/>()",
        "return <Frame/> :: Element",
    ] {
        assert!(check(source.as_bytes()).diagnostics.is_empty(), "{source}");
    }
}

#[test]
fn raw_text() {
    let source = b"return <Label>\n  don't trim \\{name} \xff \0\n</Label>";
    let tree = check(source);
    assert!(tree.diagnostics.is_empty(), "{:?}", tree.diagnostics);

    let text = tree
        .nodes
        .iter()
        .position(|node| node.kind == Kind::MarkupText)
        .unwrap();

    assert_eq!(tree.text(text), b"\n  don't trim \\{name} \xff \0\n");
}

#[test]
fn malformed() {
    for markup in [
        "<Frame></Label>",
        "<Frame>",
        "<Frame",
        "<Frame Visible ={props.Size}/>",
        "<Frame\n Visible\n ={props.Size}/>",
        "<Frame = />",
        "<Frame =\"name\" />",
        "<Frame =Size />",
        "<Frame Value={} />",
        "<Frame Value={   } />",
        "<Frame>{}</Frame>",
        "<Frame Value={--[[ note ]]} />",
        "<Frame {--[[ note ]]} />",
        "<Frame>{value + }</Frame>",
        "<Frame>{value value}</Frame>",
        "<Frame><!-- missing</Frame>",
        "<Frame Value='missing\n/>",
        "<Frame>{",
        "<Frame>{function()",
        "<Frame>{`{<Frame/>}",
        "<Frame></>",
        "<></Frame>",
        "<Frame><Label></Frame>",
    ] {
        let source = format!("return {markup}");
        let tree = check(source.as_bytes());
        assert!(!tree.diagnostics.is_empty(), "accepted {source}");
    }

    let tree = check(b"local broken = <Frame ?\nlocal valid = 1\nreturn valid");
    assert!(tree.nodes.iter().any(|node| node.kind == Kind::Local));
}

#[test]
fn isolation() {
    for entry in std::fs::read_dir("vendor/luau/tests/conformance").unwrap() {
        let path = entry.unwrap().path();

        if path.extension().is_none_or(|extension| extension != "luau") {
            continue;
        }

        let source = std::fs::read(path).unwrap();
        let plain = parse(source.as_slice().into());
        let markup = check(&source);
        assert_eq!(plain.tokens, markup.tokens);
        assert_eq!(plain.nodes, markup.nodes);
        assert_eq!(plain.children, markup.children);
        assert_eq!(plain.diagnostics, markup.diagnostics);
    }

    for source in [
        "return left < right",
        "return left <= right",
        "return left < right > other",
        "type Value = Array<Item>",
        "type Callback = <Value>(Value) -> Value",
        "local function identity<Value>(value: Value): Value return value end",
        "return identity<<Array<Value>>>(value)",
        "return object:render(<Frame/>)",
        "return (function<Value>(value: Value): Value return value end)(<Frame/>)",
    ] {
        assert!(check(source.as_bytes()).diagnostics.is_empty(), "{source}");
    }
}

#[test]
fn bytes_and_nesting() {
    for first in 0..=u8::MAX {
        for second in 0..=u8::MAX {
            for (prefix, suffix) in [
                (b"return <Frame>".as_slice(), b"</Frame>".as_slice()),
                (b"return <Frame>{".as_slice(), b"}</Frame>".as_slice()),
            ] {
                let source = [prefix, &[first, second], suffix].concat();
                check(&source);
            }
        }
    }

    let nested = format!(
        "return {}{}",
        "<Frame>".repeat(1000),
        "</Frame>".repeat(1000)
    );

    assert!(
        !check(nested.as_bytes()).diagnostics.is_empty(),
        "unbounded markup nesting"
    );
}
