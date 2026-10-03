use std::{
    collections::HashSet,
    error::Error,
    fmt,
    hash::{Hash, Hasher},
    panic::{AssertUnwindSafe, catch_unwind, resume_unwind},
    sync::Arc,
};

#[derive(Clone)]
pub struct Identity(Arc<()>);

impl Identity {
    pub(crate) fn new(identity: Arc<()>) -> Self {
        Self(identity)
    }
}

impl fmt::Debug for Identity {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_tuple("Identity")
            .field(&Arc::as_ptr(&self.0))
            .finish()
    }
}

impl PartialEq for Identity {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for Identity {}

impl Hash for Identity {
    fn hash<HasherType: Hasher>(&self, state: &mut HasherType) {
        Arc::as_ptr(&self.0).hash(state);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AnalysisError {
    Cycle,
    Capacity,
    Invalidated,
}

impl fmt::Display for AnalysisError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Cycle => "analysis query depends on itself",
            Self::Capacity => "analysis capacity cannot accommodate another active query",
            Self::Invalidated => "analysis query was invalidated during computation",
        })
    }
}

impl Error for AnalysisError {}

#[derive(Clone, PartialEq, Eq, Hash)]
struct Key<Environment> {
    identity: Identity,
    environment: Environment,
}

struct Entry<Environment, Value> {
    key: Key<Environment>,
    value: Arc<Value>,
    dependencies: HashSet<Key<Environment>>,
}

struct Pending<Environment> {
    key: Key<Environment>,
    dependencies: HashSet<Key<Environment>>,
    invalidated: bool,
}

pub struct Memo<Environment, Value> {
    capacity: usize,
    entries: Vec<Entry<Environment, Value>>,
    pending: Vec<Pending<Environment>>,
}

impl<Environment: Clone + Eq + Hash, Value> Memo<Environment, Value> {
    #[must_use]
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity,
            entries: Vec::new(),
            pending: Vec::new(),
        }
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// # Errors
    ///
    /// Returns an error for dependency cycles, exhausted active-query capacity,
    /// invalidation during computation, or an error returned by the computation.
    ///
    /// # Panics
    ///
    /// Resumes a panic from the supplied computation after removing its active query.
    pub fn query(
        &mut self,
        identity: Identity,
        environment: Environment,
        compute: impl FnOnce(&mut Self) -> Result<Value, AnalysisError>,
    ) -> Result<Arc<Value>, AnalysisError> {
        let key = Key {
            identity,
            environment,
        };

        if self.pending.iter().any(|pending| pending.key == key) {
            return Err(AnalysisError::Cycle);
        }

        if let Some(entry) = self.entries.iter().find(|entry| entry.key == key) {
            let value = Arc::clone(&entry.value);
            self.record(key);

            return Ok(value);
        }

        while self.entries.len() + self.pending.len() >= self.capacity {
            let Some(entry) = self.entries.first() else {
                return Err(AnalysisError::Capacity);
            };

            let oldest = entry.key.clone();
            self.invalidate_key(&oldest);
        }

        self.pending.push(Pending {
            key: key.clone(),
            dependencies: HashSet::new(),
            invalidated: false,
        });

        let computed = catch_unwind(AssertUnwindSafe(|| compute(self)));
        let pending = self.pending.pop().expect("active query exists");

        let computed = match computed {
            Ok(computed) => computed,
            Err(payload) => resume_unwind(payload),
        };

        if pending.invalidated {
            return Err(AnalysisError::Invalidated);
        }

        let value = Arc::new(computed?);

        self.entries.push(Entry {
            key: key.clone(),
            value: Arc::clone(&value),
            dependencies: pending.dependencies,
        });

        self.record(key);

        Ok(value)
    }

    pub fn invalidate(&mut self, identity: &Identity, environment: &Environment) {
        self.invalidate_key(&Key {
            identity: identity.clone(),
            environment: environment.clone(),
        });
    }

    pub fn clear(&mut self) {
        self.entries.clear();

        for pending in &mut self.pending {
            pending.invalidated = true;
            pending.dependencies.clear();
        }
    }

    fn record(&mut self, key: Key<Environment>) {
        if let Some(pending) = self.pending.last_mut()
            && !pending.invalidated
        {
            pending.dependencies.insert(key);
        }
    }

    fn invalidate_key(&mut self, key: &Key<Environment>) {
        let mut invalidated = HashSet::from([key.clone()]);
        let mut queue = vec![key.clone()];

        while let Some(key) = queue.pop() {
            self.entries.retain(|entry| {
                if entry.key == key || entry.dependencies.contains(&key) {
                    if invalidated.insert(entry.key.clone()) {
                        queue.push(entry.key.clone());
                    }

                    false
                } else {
                    true
                }
            });

            for pending in &mut self.pending {
                if pending.key == key || pending.dependencies.contains(&key) {
                    pending.invalidated = true;
                    pending.dependencies.clear();

                    if invalidated.insert(pending.key.clone()) {
                        queue.push(pending.key.clone());
                    }
                }
            }
        }
    }
}

#[cfg(test)]
#[test]
fn identities_keep_allocations_alive_without_retaining_syntax() {
    let tree = crate::parse(b"return 1");
    let syntax = Arc::downgrade(tree.syntax());
    let identity = tree.root().identity();
    let mut memo = Memo::new(2);
    assert_eq!(*memo.query(identity.clone(), (), |_| Ok(1)).unwrap(), 1);
    drop(tree);
    assert!(syntax.upgrade().is_none());
    let next = crate::parse(b"return 1");
    assert_ne!(identity, next.root().identity());

    assert_eq!(
        *memo.query(next.root().identity(), (), |_| Ok(2)).unwrap(),
        2
    );
}
