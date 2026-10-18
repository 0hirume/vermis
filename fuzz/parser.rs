#![no_main]

#[path = "../tests/support/validation.rs"]
mod support;

use libfuzzer_sys::fuzz_target;

use self::support::check;

fuzz_target!(|source: &[u8]| {
    drop(check(source));
});
