//! Shared syntax assertions.

mod queries;
mod validation;

pub(crate) use self::queries::{accepted, first};
pub(crate) use self::validation::check;
