#![no_main]

#[path = "../tests/support/oracle.rs"]
mod oracle;

use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use libfuzzer_sys::fuzz_target;

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

    if let Err(error) = oracle::compare(&mut oracle, data) {
        panic!("{error}");
    }
});
