#![no_main]

#[path = "../tests/support/mod.rs"]
pub mod support;

use libfuzzer_sys::fuzz_target;

fuzz_target!(|source: &[u8]| {
    support::check(source);
});
