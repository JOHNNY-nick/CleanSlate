//! `sandbox` commands: real quarantine, restore and uninstall, for the
//! acceptance tests inside Windows Sandbox only (D50).
//!
//! They refuse to run outside the Sandbox account (`WDAGUtilityAccount`):
//! on any other machine the CLI stays read-only (safety rule 1).

use std::process::ExitCode;

use cleanslate_core::model::{
    CleanPlan, Origin, RestoreRequest, ScanOptions, UninstallProgress, UninstallRequest,
};
use cleanslate_core::{Engine, EngineConfig};

/// Account of Windows Sandbox.
const SANDBOX_USER: &str = "WDAGUtilityAccount";

/// `true` only for the account of Windows Sandbox.
fn is_sandbox_user(user: Option<&str>) -> bool {
    user.is_some_and(|u| u.eq_ignore_ascii_case(SANDBOX_USER))
}

/// `true` only inside Windows Sandbox.
fn in_sandbox() -> bool {
    is_sandbox_user(std::env::var("USERNAME").ok().as_deref())
}

fn print_json(value: &impl serde::Serialize) -> ExitCode {
    match serde_json::to_string_pretty(value) {
        Ok(text) => {
            println!("{text}");
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("error: {err}");
            ExitCode::FAILURE
        }
    }
}

fn fail(err: impl std::fmt::Display) -> ExitCode {
    eprintln!("error: {err}");
    ExitCode::FAILURE
}

/// Real engine (simulation off), for the Sandbox only.
fn engine() -> Engine {
    Engine::with_system(EngineConfig {
        dry_run: false,
        ..EngineConfig::default()
    })
}

/// `sandbox <action> <argument>`.
pub fn run(args: &[String]) -> ExitCode {
    if !in_sandbox() {
        eprintln!(
            "sandbox commands only run inside Windows Sandbox ({SANDBOX_USER}): \
             on this machine the CLI never modifies anything."
        );
        return ExitCode::from(3);
    }
    match (args.first().map(String::as_str), args.get(1)) {
        (Some("quarantine"), Some(text)) => quarantine(text),
        (Some("restore"), Some(session)) => restore(session),
        (Some("uninstall"), Some(name)) => uninstall(name),
        _ => {
            eprintln!(
                "Usage: cleanslate-cli sandbox quarantine <text> | restore <session-id> | uninstall <name>"
            );
            ExitCode::from(2)
        }
    }
}

/// Scans, then quarantines every proposed item whose location contains `text`.
fn quarantine(text: &str) -> ExitCode {
    let engine = engine();
    let report = match engine.scan_orphans(&ScanOptions::default(), &|_| {}) {
        Ok(report) => report,
        Err(err) => return fail(err),
    };
    let wanted = text.to_lowercase();
    let ids: Vec<String> = report
        .groups
        .iter()
        .flat_map(|g| &g.items)
        .filter(|i| i.location.to_lowercase().contains(&wanted))
        .map(|i| i.id.clone())
        .collect();
    if ids.is_empty() {
        eprintln!("no proposed item matches « {text} »");
        return ExitCode::from(4);
    }
    let mut plan = CleanPlan::new(ids, Origin::Orphans);
    plan.dry_run = false;
    match engine.clean(&plan) {
        Ok(session) => print_json(&session),
        Err(err) => fail(err),
    }
}

/// Restores a whole session.
fn restore(session: &str) -> ExitCode {
    let request: RestoreRequest = match serde_json::from_value(serde_json::json!({
        "session_id": session,
        "dry_run": false,
    })) {
        Ok(request) => request,
        Err(err) => return fail(err),
    };
    match engine().restore(&request) {
        Ok(session) => print_json(&session),
        Err(err) => fail(err),
    }
}

/// Uninstalls the application whose name contains `name`, printing each
/// progress event on stderr (EF-21).
fn uninstall(name: &str) -> ExitCode {
    let engine = engine();
    let wanted = name.to_lowercase();
    let app = match engine.list_apps() {
        Ok(apps) => apps
            .into_iter()
            .find(|a| a.display_name.to_lowercase().contains(&wanted)),
        Err(err) => return fail(err),
    };
    let Some(app) = app else {
        eprintln!("no application matches « {name} »");
        return ExitCode::from(4);
    };
    let request = UninstallRequest {
        app_id: app.id,
        quiet: true,
        create_restore_point: false,
        dry_run: false,
    };
    let progress = |event: &UninstallProgress| {
        if let Ok(line) = serde_json::to_string(event) {
            eprintln!("{line}");
        }
    };
    match engine.uninstall_with_progress(&request, &progress) {
        Ok(result) => print_json(&result),
        Err(err) => fail(err),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The guard alone is tested: the actions never run outside the Sandbox.
    #[test]
    fn only_the_sandbox_account_is_allowed() {
        assert!(is_sandbox_user(Some("WDAGUtilityAccount")));
        assert!(is_sandbox_user(Some("wdagutilityaccount")));
        for other in [
            None,
            Some(""),
            Some("HP"),
            Some("Administrator"),
            Some("WDAGUtilityAccount2"),
        ] {
            assert!(!is_sandbox_user(other), "{other:?}");
        }
        // This development machine is not the Sandbox.
        assert!(!in_sandbox());
    }
}
