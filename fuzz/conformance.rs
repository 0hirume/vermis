#![no_main]

#[allow(dead_code)]
#[path = "../tests/support/oracle.rs"]
mod oracle;
#[allow(dead_code)]
#[path = "../tests/support/parser.rs"]
mod parser;

use libfuzzer_sys::fuzz_target;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

static ORACLE: OnceLock<Mutex<oracle::Oracle>> = OnceLock::new();

fn oracle() -> &'static Mutex<oracle::Oracle> {
    ORACLE.get_or_init(|| {
        let path = std::env::var_os("VERMIS_ORACLE")
            .map(PathBuf::from)
            .expect("VERMIS_ORACLE must point to the Luau oracle executable");

        Mutex::new(oracle::Oracle::spawn(&path).expect("failed to start Luau oracle"))
    })
}

fuzz_target!(|data: &[u8]| {
    let mut oracle = oracle().lock().expect("oracle mutex was poisoned");

    if let Err(error) = parser::compare_chunk(&mut oracle, data) {
        panic!("{error}");
    }
});
