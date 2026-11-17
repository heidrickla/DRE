//! Print the phase 1 workload hash and exit.
//!
//! Exists so determinism can be checked *across processes*, which a test
//! running inside one process cannot do. Run it twice and compare: a
//! difference means something outside the input reached the result —
//! address-space layout, a lazily-initialised static, an environment read.
//!
//! ```bash
//! cargo run -p dre-conformance --bin workload-hash
//! ```
//!
//! An optional seed argument varies construction order. Any seed must give
//! the same hash, so `workload-hash 0` and `workload-hash 99` agreeing is
//! the same property the acceptance test asserts, checked from outside.

use std::process::ExitCode;

use dre_conformance::phase1_workload;

fn main() -> ExitCode {
    let seed: u64 = match std::env::args().nth(1) {
        None => 0,
        Some(raw) => match raw.parse() {
            Ok(value) => value,
            Err(_) => {
                eprintln!("error: seed must be an unsigned integer, got {raw:?}");
                return ExitCode::FAILURE;
            }
        },
    };

    match phase1_workload(seed) {
        Ok(hash) => {
            println!("{hash}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}
