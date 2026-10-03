use std::{
    error::Error,
    fmt,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Limits {
    pub depth: Option<usize>,
    pub tokens: Option<usize>,
    pub nodes: Option<usize>,
    pub diagnostics: Option<usize>,
    pub source_bytes: Option<usize>,
}

#[derive(Clone, Debug, Default)]
pub struct Control {
    pub limits: Limits,
    pub cancellation: Option<Arc<AtomicBool>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Resource {
    Depth,
    Tokens,
    Nodes,
    Diagnostics,
    SourceBytes,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParseError {
    Cancelled,
    Limit(Resource),
}

impl fmt::Display for ParseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cancelled => formatter.write_str("parsing cancelled"),

            Self::Limit(resource) => {
                write!(formatter, "parser resource limit exceeded: {resource:?}")
            }
        }
    }
}

impl Error for ParseError {}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Ledger {
    pub tokens: usize,
    pub nodes: usize,
    pub diagnostics: usize,
}

pub(crate) struct Execution {
    pub limits: Limits,
    cancellation: Option<Arc<AtomicBool>>,
    aborted: AtomicBool,
    error: Mutex<Option<ParseError>>,
    tokens: AtomicUsize,
    nodes: AtomicUsize,
    diagnostics: AtomicUsize,
}

impl Execution {
    pub(crate) fn new(control: &Control) -> Arc<Self> {
        Arc::new(Self {
            limits: control.limits.clone(),
            cancellation: control.cancellation.clone(),
            aborted: AtomicBool::new(false),
            error: Mutex::new(None),
            tokens: AtomicUsize::new(0),
            nodes: AtomicUsize::new(0),
            diagnostics: AtomicUsize::new(0),
        })
    }

    pub(crate) fn poll(&self) -> bool {
        if self.aborted.load(Ordering::Relaxed) {
            return false;
        }

        if self
            .cancellation
            .as_ref()
            .is_some_and(|cancellation| cancellation.load(Ordering::Relaxed))
        {
            self.fail(ParseError::Cancelled);

            return false;
        }

        true
    }

    pub(crate) fn fail(&self, error: ParseError) {
        let mut recorded = self.error.lock().expect("parser error lock");

        if recorded.is_none() {
            *recorded = Some(error);
        }

        self.aborted.store(true, Ordering::Relaxed);
    }

    pub(crate) fn error(&self) -> Option<ParseError> {
        *self.error.lock().expect("parser error lock")
    }

    pub(crate) fn source(&self, initial: usize, length: usize) -> bool {
        self.poll()
            && self.check(
                initial.checked_add(length),
                self.limits.source_bytes,
                Resource::SourceBytes,
            )
    }

    pub(crate) fn depth(&self, depth: usize) -> bool {
        self.poll() && self.check(Some(depth), self.limits.depth, Resource::Depth)
    }

    pub(crate) fn token(&self) -> bool {
        self.charge(&self.tokens, self.limits.tokens, Resource::Tokens)
    }

    pub(crate) fn node(&self) -> bool {
        self.charge(&self.nodes, self.limits.nodes, Resource::Nodes)
    }

    pub(crate) fn diagnostic(&self) -> bool {
        self.charge(
            &self.diagnostics,
            self.limits.diagnostics,
            Resource::Diagnostics,
        )
    }

    pub(crate) fn snapshot(&self) -> Ledger {
        Ledger {
            tokens: self.tokens.load(Ordering::Relaxed),
            nodes: self.nodes.load(Ordering::Relaxed),
            diagnostics: self.diagnostics.load(Ordering::Relaxed),
        }
    }

    pub(crate) fn restore(&self, ledger: Ledger) {
        self.tokens.store(ledger.tokens, Ordering::Relaxed);
        self.nodes.store(ledger.nodes, Ordering::Relaxed);

        self.diagnostics
            .store(ledger.diagnostics, Ordering::Relaxed);
    }

    pub(crate) fn retain(
        &self,
        ledger: Ledger,
        diagnostics_before: usize,
        diagnostics_after: usize,
    ) {
        if !self.poll() {
            return;
        }

        let diagnostics = ledger
            .diagnostics
            .checked_sub(diagnostics_before)
            .and_then(|count| count.checked_add(diagnostics_after));

        if self.check(diagnostics, self.limits.diagnostics, Resource::Diagnostics) {
            self.restore(Ledger {
                tokens: ledger.tokens,
                nodes: ledger.nodes,
                diagnostics: diagnostics.expect("checked diagnostics"),
            });
        }
    }

    fn charge(&self, counter: &AtomicUsize, limit: Option<usize>, resource: Resource) -> bool {
        if !self.poll() {
            return false;
        }

        let previous = counter.try_update(Ordering::Relaxed, Ordering::Relaxed, |count| {
            count.checked_add(1)
        });

        self.check(
            previous.ok().and_then(|count| count.checked_add(1)),
            limit,
            resource,
        )
    }

    fn check(&self, used: Option<usize>, limit: Option<usize>, resource: Resource) -> bool {
        if used.is_none() || limit.is_some_and(|limit| used.is_some_and(|used| used > limit)) {
            self.fail(ParseError::Limit(resource));

            return false;
        }

        true
    }
}
