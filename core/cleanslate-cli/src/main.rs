//! Debug command-line front-end for the CleanSlate engine.
//!
//! Every command is read-only: nothing is ever moved or deleted from here.
//! The `sandbox` commands (real actions, for the acceptance tests) exist only
//! in a binary built with the `sandbox-tests` feature, never in a release
//! build, and refuse to run outside Windows Sandbox (D50, D53).

#[cfg(feature = "sandbox-tests")]
mod sandbox;

use std::process::ExitCode;
use std::time::Instant;

use cleanslate_core::model::{Confidence, InstalledApp, ScanOptions};
use cleanslate_core::{Engine, EngineConfig};

const USAGE: &str = "\
Usage: cleanslate-cli <command> [options]

Commands:
  version [--json]          Print the engine version
  apps [--json] [--all]     List installed applications (read-only, EF-01);
                            --all also shows system components and updates
  scan --dry-run [--json]   Look for orphan leftovers (read-only simulation,
                            EF-05 to EF-11); --dry-run is mandatory
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
        Some("scan") => {
            if !flag("--dry-run") {
                eprintln!(
                    "scan requires --dry-run (safety rule 1): the CLI never deletes anything."
                );
                return ExitCode::from(2);
            }
            scan(flag("--json"))
        }
        #[cfg(feature = "sandbox-tests")]
        Some("sandbox") => {
            let rest: Vec<String> = args
                .iter()
                .skip_while(|a| a.as_str() != "sandbox")
                .skip(1)
                .cloned()
                .collect();
            sandbox::run(&rest)
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

/// `scan --dry-run`: read-only scan of this machine; progress on stderr.
fn scan(json: bool) -> ExitCode {
    let engine = Engine::with_system(EngineConfig::default());
    let progress = |p: &cleanslate_core::model::ScanProgress| {
        eprintln!("[{:?}] {}/{} {}", p.phase, p.current, p.total, p.path);
    };
    let report = match engine.scan_orphans(&ScanOptions::default(), &progress) {
        Ok(report) => report,
        Err(err) => {
            eprintln!("error {}: {err}", err.code());
            return ExitCode::FAILURE;
        }
    };
    if json {
        match serde_json::to_string_pretty(&report) {
            Ok(text) => println!("{text}"),
            Err(err) => {
                eprintln!("error: {err}");
                return ExitCode::FAILURE;
            }
        }
    } else {
        for group in &report.groups {
            println!("{} ({} bytes)", group.label, group.total_bytes);
            for item in &group.items {
                let level = match item.confidence {
                    Confidence::High => "high",
                    Confidence::Medium => "medium",
                    Confidence::Low => "low",
                };
                println!("  [{level:<6}] {:?} {}", item.kind, item.location);
            }
        }
    }
    let items: usize = report.groups.iter().map(|g| g.items.len()).sum();
    eprintln!(
        "simulation: {} groups, {items} items, {} errors, {} ms{}",
        report.groups.len(),
        report.errors.len(),
        report.duration_ms,
        if report.cancelled { " (cancelled)" } else { "" }
    );
    ExitCode::SUCCESS
}
