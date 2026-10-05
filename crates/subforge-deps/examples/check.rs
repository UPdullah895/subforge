//! Prints the dependency report for this machine as JSON.
//! Run with: `cargo run --example check` (from crates/subforge-deps).

use subforge_deps::{check_all, Context};

fn main() {
    let report = check_all(&Context::from_process());
    println!("{}", serde_json::to_string_pretty(&report).expect("report serializes"));
}
