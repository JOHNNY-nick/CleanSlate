//! Debug command-line front-end for the CleanSlate engine.
//!
//! J1 only offers `version`; `scan --dry-run --json` arrives with J3.

use std::process::ExitCode;

const USAGE: &str = "\
Usage: cleanslate-cli <command> [--json]

Commands:
  version   Print the engine version
  help      Print this help";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let json = args.iter().any(|a| a == "--json");
    let command = args
        .iter()
        .find(|a| !a.starts_with("--"))
        .map(String::as_str);

    match command {
        Some("version") => {
            if json {
                println!(r#"{{"version":"{}"}}"#, cleanslate_core::VERSION);
            } else {
                println!("CleanSlate engine {}", cleanslate_core::VERSION);
            }
            ExitCode::SUCCESS
        }
        Some("help") | None => {
            println!("{USAGE}");
            ExitCode::SUCCESS
        }
        Some(other) => {
            eprintln!("Unknown or not yet available command: {other}\n\n{USAGE}");
            ExitCode::from(2)
        }
    }
}
