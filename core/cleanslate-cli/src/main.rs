//! Debug command-line front-end for the CleanSlate engine.
//!
//! Every command is read-only: `scan --dry-run --json` arrives with J3.

use std::process::ExitCode;
use std::time::Instant;

use cleanslate_core::model::InstalledApp;
use cleanslate_core::{Engine, EngineConfig};

const USAGE: &str = "\
Usage: cleanslate-cli <command> [options]

Commands:
  version [--json]          Print the engine version
  apps [--json] [--all]     List installed applications (read-only, EF-01);
                            --all also shows system components and updates
  help                      Print this help";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let flag = |name: &str| args.iter().any(|a| a == name);
    let command = args
        .iter()
        .find(|a| !a.starts_with("--"))
        .map(String::as_str);

    match command {
        Some("version") => {
            if flag("--json") {
                println!(r#"{{"version":"{}"}}"#, cleanslate_core::VERSION);
            } else {
                println!("CleanSlate engine {}", cleanslate_core::VERSION);
            }
            ExitCode::SUCCESS
        }
        Some("apps") => apps(flag("--json"), flag("--all")),
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

/// `apps`: reads the Uninstall keys and prints them; the duration goes to
/// stderr so that `--json` output stays clean (ENF-05: under 2 s).
fn apps(json: bool, all: bool) -> ExitCode {
    let engine = Engine::with_system(EngineConfig::default());
    let started = Instant::now();
    let apps = match engine.list_apps() {
        Ok(apps) => apps,
        Err(err) => {
            eprintln!("error {}: {err}", err.code());
            return ExitCode::FAILURE;
        }
    };
    let elapsed = started.elapsed();
    let shown: Vec<&InstalledApp> = apps
        .iter()
        .filter(|a| all || a.hidden_reason.is_none())
        .collect();

    if json {
        match serde_json::to_string_pretty(&shown) {
            Ok(text) => println!("{text}"),
            Err(err) => {
                eprintln!("error: {err}");
                return ExitCode::FAILURE;
            }
        }
    } else {
        for app in &shown {
            println!(
                "{:<50} {:<30} {}",
                app.display_name,
                app.publisher.as_deref().unwrap_or("-"),
                app.version.as_deref().unwrap_or("-")
            );
        }
    }
    eprintln!(
        "{} applications ({} shown) read in {} ms",
        apps.len(),
        shown.len(),
        elapsed.as_millis()
    );
    ExitCode::SUCCESS
}
