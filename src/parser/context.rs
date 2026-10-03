use crate::{Keyword, Kind, Span, Token, TokenKind, lexer::State};
use std::collections::HashSet;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Rule {
    Statement,
    Expression(u8),
    Annotation { allow_pack: bool, declaration: bool },
    Condition,
    Arguments,
    TypeArguments,
    Parameters { types: bool },
    Generics { defaults: bool },
    Block(Vec<Keyword>),
    Markup,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Context {
    pub rule: Rule,
    pub markup: bool,
    pub depth: usize,
    pub previous: Option<Kind>,
    pub lexical: State,
}

impl Context {
    pub(crate) fn equivalent(&self, other: &Self, pairs: &mut HashSet<(usize, usize)>) -> bool {
        self.rule == other.rule
            && self.markup == other.markup
            && self.depth == other.depth
            && self.previous == other.previous
            && self.lexical.equivalent(&other.lexical, pairs)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Boundary {
    pub context: Context,
    pub consumed: Span,
    pub inspected: Vec<Span>,
    pub exit: State,
    pub current: Token,
    pub maximum_depth: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Expected {
    Role(&'static str),
    Token(TokenKind),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Expectation {
    pub span: Span,
    pub expected: Expected,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::{Brace, Mode};

    use crate::parser::{
        Parser,
        builder::Builder,
        control::{Control, Execution},
        controlled_unit_in, parse_source,
    };

    fn unit<'source>(source: &'source [u8], markup: bool, context: &Context) -> Builder<'source> {
        controlled_unit_in(
            source,
            markup,
            context,
            &Execution::new(&Control::default()),
        )
        .unwrap()
    }

    fn controlled<'source>(
        source: &'source [u8],
        markup: bool,
        control: &Control,
    ) -> Result<Builder<'source>, crate::parser::control::ParseError> {
        crate::parser::controlled_in(source, markup, &Execution::new(control))
    }

    fn covers(boundary: &Boundary, span: Span) -> bool {
        boundary
            .inspected
            .iter()
            .any(|inspection| inspection.start <= span.start && inspection.end >= span.end)
    }

    #[test]
    fn nested_contracts_replay_with_their_incoming_context() {
        let mut rules = Vec::new();

        for source in [
            b"function f<T>(value: Box<T>): number if const item = value then return f(item + 1) end end".as_slice(),
            b"declare callback: (value: number) -> (number, ...string)".as_slice(),
        ] {
            let builder = parse_source::<false>(source, None);
            assert_eq!(builder.diagnostics, [] as [crate::Diagnostic; 0]);

            for node in &builder.nodes {
            let Some(boundary) = node.boundaries.last() else { continue };

            let start = boundary.consumed.start;
            let replayed = unit(&source[start..], false, &boundary.context);
            let root = &replayed.nodes[replayed.root];
            assert_eq!(root.kind, node.kind, "{:?}", boundary.context.rule);
            assert_eq!(root.span.len(), node.span.len(), "{:?}", boundary.context.rule);
            let replayed_boundary = root.boundaries.last().unwrap();
            assert_eq!(replayed_boundary.context, boundary.context);
            assert_eq!(replayed_boundary.exit, boundary.exit);
            assert_eq!(replayed_boundary.current.kind, boundary.current.kind);
            assert_eq!(replayed_boundary.current.span.start + start, boundary.current.span.start);
            assert_eq!(replayed_boundary.current.span.end + start, boundary.current.span.end);
            rules.push(boundary.context.rule.clone());
        }
        }

        assert!(rules.iter().any(|rule| matches!(rule, Rule::Expression(_))));

        assert!(
            rules
                .iter()
                .any(|rule| matches!(rule, Rule::Annotation { .. }))
        );

        assert!(rules.contains(&Rule::Condition));
        assert!(rules.contains(&Rule::Arguments));
        assert!(rules.contains(&Rule::TypeArguments));
        assert!(rules.contains(&Rule::Parameters { types: false }));
        assert!(rules.contains(&Rule::Parameters { types: true }));
        assert!(rules.contains(&Rule::Generics { defaults: false }));

        assert!(
            rules
                .iter()
                .any(|rule| matches!(rule, Rule::Block(stops) if stops == &[Keyword::End]))
        );
    }

    #[test]
    fn direct_guards_do_not_absorb_child_reads() {
        let source = b"if const value = object + member then return value end";
        let builder = parse_source::<false>(source, None);

        let condition = builder
            .nodes
            .iter()
            .flat_map(|node| &node.boundaries)
            .find(|boundary| boundary.context.rule == Rule::Condition)
            .unwrap();

        let tokens: Vec<_> = builder
            .tokens
            .iter()
            .filter(|token| token.kind == TokenKind::Name)
            .collect();

        assert!(covers(condition, tokens[0].span));
        assert!(covers(condition, tokens[1].span));
        assert!(!covers(condition, tokens[2].span));
        assert!(!covers(condition, tokens[3].span));

        let expression = builder
            .nodes
            .iter()
            .flat_map(|node| &node.boundaries)
            .find(|boundary| {
                boundary.context.rule == Rule::Expression(0)
                    && boundary.consumed.start == tokens[2].span.start
            })
            .unwrap();

        assert!(covers(expression, tokens[2].span));
        assert!(!covers(expression, tokens[3].span));

        let source = b"continue\nlocal value = 1";
        let builder = parse_source::<false>(source, None);

        let statement = builder
            .nodes
            .iter()
            .find(|node| node.kind == Kind::Continue)
            .unwrap()
            .boundaries
            .last()
            .unwrap();

        let lookahead = builder
            .tokens
            .iter()
            .find(|token| token.kind == TokenKind::Keyword(Keyword::Local))
            .unwrap();

        assert!(lookahead.span.start >= statement.consumed.end);
        assert!(covers(statement, lookahead.span));

        let terminal = builder
            .nodes
            .iter()
            .flat_map(|node| &node.boundaries)
            .find(|boundary| boundary.context.rule == Rule::Expression(0))
            .unwrap();

        assert!(terminal.inspected.contains(&Span {
            start: source.len(),
            end: source.len()
        }));
    }

    #[test]
    fn shared_nodes_keep_expression_and_condition_contracts() {
        let builder = parse_source::<false>(b"if value then return end", None);

        let node = builder
            .nodes
            .iter()
            .find(|node| {
                node.boundaries
                    .iter()
                    .any(|boundary| boundary.context.rule == Rule::Condition)
            })
            .unwrap();

        assert!(
            node.boundaries
                .iter()
                .any(|boundary| boundary.context.rule == Rule::Expression(0))
        );

        assert_eq!(
            node.boundaries.last().unwrap().context.rule,
            Rule::Condition
        );
    }

    #[test]
    fn raw_markup_and_holes_keep_distinct_lexical_modes() {
        let source = b"return <Frame>{`value {item}`}<Child Name='x'/></Frame>";
        let builder = parse_source::<true>(source, None);
        assert_eq!(builder.diagnostics, [] as [crate::Diagnostic; 0]);
        assert_eq!(builder.tokens.len(), builder.checkpoints.len());

        for node in &builder.nodes {
            let Some(boundary) = node.boundaries.last() else {
                continue;
            };

            let replayed = unit(&source[node.span.start..], true, &boundary.context);
            let root = &replayed.nodes[replayed.root];
            assert_eq!(root.kind, node.kind, "{:?}", boundary.context.rule);

            assert_eq!(
                root.span.len(),
                node.span.len(),
                "{:?}",
                boundary.context.rule
            );

            assert_eq!(root.boundaries.last().unwrap().exit, boundary.exit);

            assert_eq!(
                root.boundaries.last().unwrap().current.kind,
                boundary.current.kind
            );

            assert_eq!(
                root.boundaries.last().unwrap().current.span.start + node.span.start,
                boundary.current.span.start
            );

            assert_eq!(
                root.boundaries.last().unwrap().current.span.end + node.span.start,
                boundary.current.span.end
            );
        }

        for (token, checkpoint) in builder.tokens.iter().zip(&builder.checkpoints) {
            assert_eq!(token.span.start, checkpoint.cursor);
        }

        let child = builder
            .nodes
            .iter()
            .find(|node| node.kind == Kind::Element && node.span.start > 7)
            .unwrap();

        let boundary = child.boundaries.last().unwrap();
        assert_eq!(boundary.context.lexical.mode, Mode::MarkupChildren);
        let replayed = unit(&source[child.span.start..], true, &boundary.context);
        assert_eq!(replayed.nodes[replayed.root].span.len(), child.span.len());

        assert_eq!(
            replayed.nodes[replayed.root]
                .boundaries
                .last()
                .unwrap()
                .exit,
            boundary.exit
        );

        let replayed_boundary = replayed.nodes[replayed.root].boundaries.last().unwrap();

        assert_eq!(
            replayed_boundary
                .inspected
                .iter()
                .map(|span| Span {
                    start: span.start + child.span.start,
                    end: span.end + child.span.start
                })
                .collect::<Vec<_>>(),
            boundary.inspected
        );

        assert!(
            builder
                .nodes
                .iter()
                .flat_map(|node| &node.boundaries)
                .any(|boundary| {
                    boundary.context.lexical.mode == Mode::MarkupHole
                        && boundary.context.lexical.braces
                            == crate::lexer::Braces::from([Brace::Interpolated])
                })
        );

        let opening = builder
            .nodes
            .iter()
            .find(|node| node.kind == Kind::Element && node.span.start == 7)
            .unwrap();

        let closing = builder
            .nodes
            .iter()
            .find(|node| node.kind == Kind::Closing)
            .unwrap();

        assert!(
            opening
                .boundaries
                .iter()
                .any(|boundary| covers(boundary, closing.span))
        );
    }

    #[test]
    fn recovery_origins_and_expectations_survive_discarded_nodes() {
        for (source, markup) in [
            (
                b"local value = 0x\nreceiver.\nlocal tail =".as_slice(),
                false,
            ),
            (b"return <Frame Value={item +".as_slice(), true),
            (b"function f(value:".as_slice(), false),
        ] {
            let builder = if markup {
                parse_source::<true>(source, None)
            } else {
                parse_source::<false>(source, None)
            };

            assert_ne!(builder.diagnostics, []);
            assert_eq!(builder.diagnostics.len(), builder.origins.len());
            let mut pending = vec![builder.root];
            let mut reachable = vec![false; builder.nodes.len()];

            while let Some(node) = pending.pop() {
                reachable[node] = true;
                pending.extend_from_slice(&builder.children[builder.nodes[node].children.clone()]);
            }

            for origin in &builder.origins {
                assert!(reachable[origin.expect("diagnostic has an owner")]);
            }

            assert!(builder.nodes.iter().any(|node| !node.recovery.is_empty()));

            for (index, node) in builder.nodes.iter().enumerate() {
                for expectation in &node.recovery {
                    assert!(
                        reachable[index],
                        "recovery expectation has a reachable owner"
                    );

                    assert!(expectation.span.is_empty());
                    assert!(expectation.span.end <= source.len());
                }
            }

            assert_eq!(builder.tokens.len(), builder.checkpoints.len());
        }

        let source = b"return (value";
        let builder = parse_source::<false>(source, None);

        let (owner, group) = builder
            .nodes
            .iter()
            .enumerate()
            .find(|(_, node)| node.kind == Kind::Group)
            .unwrap();

        assert_eq!(builder.origins, [Some(owner)]);

        assert_eq!(
            group.recovery,
            [Expectation {
                span: Span {
                    start: source.len(),
                    end: source.len(),
                },
                expected: Expected::Token(TokenKind::Byte(b')')),
            }]
        );
    }

    #[test]
    fn transactions_restore_scanner_and_dependencies() {
        let source = b"`before {value + 1} after`";
        let mut parser = Parser::<true>::new(source, 37, None, State::default(), None);
        parser.skip_trivia();
        parser.enter(Rule::Expression(0), 0, parser.entry_state());
        let checkpoint = parser.checkpoint();
        let token = parser.raw_current();
        let lexical = parser.entry_state();
        parser.expression(0).unwrap();
        assert_ne!(parser.cursor, checkpoint.cursor);
        parser.restore(checkpoint);
        assert_eq!(parser.raw_current(), token);
        assert_eq!(parser.entry_state(), lexical);
        assert_eq!(parser.depth, 37);
        assert!(parser.builder.nodes.is_empty());
        assert_eq!(parser.builder.diagnostics, [] as [crate::Diagnostic; 0]);

        assert_eq!(
            parser.frames.borrow().last().unwrap().inspected,
            [] as [crate::Span; 0]
        );

        let node = parser.expression(0).unwrap();
        assert_eq!(parser.builder.nodes[node].kind, Kind::Interpolation);

        let context = Context {
            rule: Rule::Expression(0),
            markup: false,
            depth: 256,
            previous: None,
            lexical: State::default(),
        };

        let builder = unit(b"value", false, &context);
        assert_eq!(builder.nodes[builder.root].kind, Kind::Missing);

        assert!(
            builder
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.message == "syntax nesting limit exceeded")
        );

        assert_eq!(
            builder.nodes[builder.root]
                .boundaries
                .last()
                .unwrap()
                .context
                .depth,
            256
        );
    }

    #[test]
    fn lexical_checkpoints_resume_every_interpolation_stack() {
        let source = b"`outer { { value = `inner {item}` } } done`";
        let mut lexer = crate::Lexer::new(source);
        let mut checkpoints = Vec::new();
        let mut tokens = Vec::new();

        loop {
            let checkpoint = lexer.checkpoint();

            let Some(token) = lexer.next() else { break };

            checkpoints.push(checkpoint);
            tokens.push(token);
        }

        assert!(
            checkpoints
                .iter()
                .any(|checkpoint| checkpoint.state.braces.len() == 3)
        );

        for (position, checkpoint) in checkpoints.iter().enumerate() {
            let mut replayed = crate::Lexer::new(source);
            replayed.restore(checkpoint);
            assert_eq!(replayed.collect::<Vec<_>>(), tokens[position..]);
        }

        let terminal = lexer.checkpoint();
        assert!(terminal.finished);
        lexer.restore(&terminal);
        assert!(lexer.next().is_none());
        let mut resumed = crate::Lexer::from_state(source, source.len(), &terminal.state);
        assert_eq!(resumed.next().unwrap().kind, TokenKind::Eof);
        assert_eq!(resumed.state(), terminal.state);
    }

    #[test]
    fn configured_limits_return_resource_errors_and_default_depth_recovers() {
        use crate::parser::control::{Limits, ParseError, Resource};
        let source = b"local value = f(1 + 2)";
        let baseline = controlled(source, false, &Control::default()).unwrap();

        for (limits, resource) in [
            (
                Limits {
                    source_bytes: Some(source.len() - 1),
                    ..Limits::default()
                },
                Resource::SourceBytes,
            ),
            (
                Limits {
                    tokens: Some(baseline.tokens.len() - 1),
                    ..Limits::default()
                },
                Resource::Tokens,
            ),
            (
                Limits {
                    nodes: Some(baseline.nodes.len() - 1),
                    ..Limits::default()
                },
                Resource::Nodes,
            ),
            (
                Limits {
                    depth: Some(1),
                    ..Limits::default()
                },
                Resource::Depth,
            ),
        ] {
            let result = controlled(
                source,
                false,
                &Control {
                    limits,
                    ..Control::default()
                },
            );

            assert!(matches!(result, Err(ParseError::Limit(actual)) if actual == resource));
        }

        let result = controlled(
            b"local value =",
            false,
            &Control {
                limits: Limits {
                    diagnostics: Some(0),
                    ..Limits::default()
                },
                ..Control::default()
            },
        );

        assert!(matches!(
            result,
            Err(ParseError::Limit(Resource::Diagnostics))
        ));

        let context = baseline
            .nodes
            .iter()
            .flat_map(|node| &node.boundaries)
            .find(|boundary| boundary.context.rule == Rule::Expression(0))
            .unwrap()
            .context
            .clone();

        let execution = Execution::new(&Control {
            limits: Limits {
                depth: Some(context.depth.saturating_sub(1)),
                ..Limits::default()
            },
            ..Control::default()
        });

        let result = controlled_unit_in(b"1", false, &context, &execution);
        assert!(matches!(result, Err(ParseError::Limit(Resource::Depth))));
        let source = format!("return {}value{}", "(".repeat(300), ")".repeat(300));
        let builder = controlled(source.as_bytes(), false, &Control::default()).unwrap();

        assert!(
            builder
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.message == "syntax nesting limit exceeded")
        );
    }

    #[test]
    fn cancelled_requests_return_cancellation_errors() {
        use crate::parser::control::ParseError;
        use std::sync::{Arc, atomic::AtomicBool};

        let control = Control {
            cancellation: Some(Arc::new(AtomicBool::new(true))),
            ..Control::default()
        };

        for markup in [false, true] {
            assert!(matches!(
                crate::parser::controlled(b"return <Frame>{value}</Frame>", markup, &control),
                Err(ParseError::Cancelled)
            ));
        }
    }

    #[test]
    fn raw_markup_boundaries_replay_the_actual_unconsumed_byte() {
        use crate::lexer::Braces;

        for mode in [Mode::MarkupTag, Mode::MarkupChildren] {
            let lexical = State {
                braces: Braces::from([Brace::Interpolated, Brace::Normal]),
                mode,
            };

            let context = Context {
                rule: Rule::Markup,
                markup: true,
                depth: 7,
                previous: None,
                lexical: lexical.clone(),
            };

            for source in [
                b"<Child/> text<Sibling/>".as_slice(),
                b"<Child/></Parent>".as_slice(),
                b"<Child/>".as_slice(),
            ] {
                let builder = unit(source, true, &context);
                let root = &builder.nodes[builder.root];
                assert_eq!(root.kind, Kind::Element);
                assert_eq!(root.span, Span { start: 0, end: 8 });
                let boundary = root.boundaries.last().unwrap();
                assert_eq!(boundary.context, context);
                assert_eq!(boundary.exit, lexical);

                assert_eq!(
                    boundary.current,
                    Token {
                        kind: source
                            .get(8)
                            .map_or(TokenKind::Eof, |byte| TokenKind::Byte(*byte)),
                        span: Span {
                            start: 8,
                            end: 9.min(source.len())
                        },
                    }
                );

                assert!(covers(boundary, boundary.current.span));
            }

            for source in [b"Child/>".as_slice(), b"".as_slice()] {
                let builder = unit(source, true, &context);
                assert_eq!(builder.nodes[builder.root].kind, Kind::Error);
                assert_ne!(builder.diagnostics, [] as [crate::Diagnostic; 0]);
                assert!(builder.origins.iter().all(Option::is_some));
            }
        }
    }

    #[test]
    fn shared_requests_preserve_prior_attempt_work_and_checkpoint_counters() {
        use crate::parser::control::{Limits, ParseError, Resource};

        let context = Context {
            rule: Rule::Expression(0),
            markup: false,
            depth: 0,
            previous: None,
            lexical: State::default(),
        };

        let execution = Execution::new(&Control {
            limits: Limits {
                tokens: Some(3),
                ..Limits::default()
            },
            ..Control::default()
        });

        controlled_unit_in(b"1", false, &context, &execution).unwrap();
        assert_eq!(execution.snapshot().tokens, 2);

        assert!(matches!(
            controlled_unit_in(b"2", false, &context, &execution),
            Err(ParseError::Limit(Resource::Tokens))
        ));

        let execution = Execution::new(&Control::default());
        assert!(execution.token() && execution.node() && execution.diagnostic());

        let mut parser =
            Parser::<true>::new(b"value", 9, None, State::default(), Some(execution.clone()));

        parser.skip_trivia();
        parser.enter(Rule::Expression(0), 0, parser.entry_state());
        let checkpoint = parser.checkpoint();
        let ledger = execution.snapshot();
        parser.expression(0).unwrap();
        parser.diagnose(parser.error("test diagnostic"));
        assert_ne!(execution.snapshot(), ledger);
        parser.restore(checkpoint);
        assert_eq!(execution.snapshot(), ledger);
        assert_eq!(parser.depth, 9);
    }

    #[test]
    fn successful_child_depth_stays_out_of_parent_direct_measurements() {
        let builder = parse_source::<false>(b"return (((value)))", None);

        let block = builder
            .nodes
            .iter()
            .find(|node| node.kind == Kind::Block)
            .unwrap();

        assert_eq!(block.boundaries.last().unwrap().maximum_depth, 0);

        let statement = builder
            .nodes
            .iter()
            .find(|node| node.kind == Kind::Return)
            .unwrap();

        assert_eq!(statement.boundaries.last().unwrap().maximum_depth, 1);

        assert!(
            builder
                .nodes
                .iter()
                .flat_map(|node| &node.boundaries)
                .any(|boundary| boundary.maximum_depth == 5)
        );

        let empty = parse_source::<false>(b"", None);

        let boundaries: Vec<_> = empty
            .nodes
            .iter()
            .flat_map(|node| &node.boundaries)
            .collect();

        assert!(!boundaries.is_empty());

        assert!(
            boundaries
                .iter()
                .all(|boundary| boundary.maximum_depth == 0)
        );
    }

    #[test]
    fn lexical_diagnostics_do_not_replace_required_roles_or_create_tokens() {
        let builder = parse_source::<false>(b"const value 'unfinished", None);
        assert_eq!(builder.diagnostics[0].message, "unterminated string");

        let missing = builder
            .nodes
            .iter()
            .find(|node| node.kind == Kind::Missing)
            .unwrap();

        assert!(
            missing
                .recovery
                .iter()
                .any(|expectation| expectation.expected == Expected::Role("initializer"))
        );

        assert!(
            builder
                .nodes
                .iter()
                .flat_map(|node| &node.recovery)
                .any(|expectation| expectation.expected == Expected::Token(TokenKind::Byte(b'=')))
        );

        assert!(
            !builder
                .tokens
                .iter()
                .any(|token| token.kind == TokenKind::Byte(b'='))
        );

        let builder = parse_source::<true>(b"return <Frame = ", None);

        assert!(
            builder
                .nodes
                .iter()
                .flat_map(|node| &node.recovery)
                .any(|expectation| expectation.expected == Expected::Token(TokenKind::Byte(b'{')))
        );

        assert!(
            !builder
                .tokens
                .iter()
                .any(|token| token.kind == TokenKind::Byte(b'{'))
        );
    }
}
