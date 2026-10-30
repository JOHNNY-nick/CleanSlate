//! `sandbox` commands: real quarantine, restore and uninstall, for the
//! acceptance tests inside Windows Sandbox only (D50, D53).
//!
//! Compiled only with the `sandbox-tests` feature. Before any action, three
//! independent conditions must all hold (see [`check_guard`]); otherwise the
//! command exits with code 3 and touches nothing (safety rule 1).

use std::process::ExitCode;

use cleanslate_core::model::{
    CleanPlan, Origin, RestoreRequest, ScanOptions, UninstallProgress, UninstallRequest,
};
use cleanslate_core::{Engine, EngineConfig};

/// Account of Windows Sandbox.
const SANDBOX_USER: &str = "WDAGUtilityAccount";
/// Variable set by the logon command of the `.wsb` file written by
/// `prepare.ps1` (one random token per preparation).
const TOKEN_VARIABLE: &str = "CLEANSLATE_SANDBOX_RUN";
/// Same token, in the repository shared read-only with the Sandbox: this
/// path exists only through the mapping of the `.wsb` file.
const TOKEN_FILE: &str = r"C:\CleanSlate\repo\artifacts\sandbox\run\token.txt";
/// A token is 64 hexadecimal digits (32 random bytes).
const TOKEN_LEN: usize = 64;

/// The three conditions of the guard, each from a different source:
/// the account is the Sandbox one (`USERNAME`), the `.wsb` logon command set
/// the token (`CLEANSLATE_SANDBOX_RUN`), and the same token is in the file
/// shared read-only by that `.wsb`. Returns why it refuses.
fn check_guard(
    user: Option<&str>,
    env_token: Option<&str>,
    file_token: Option<&str>,
) -> Result<(), &'static str> {
    if !user.is_some_and(|u| u.eq_ignore_ascii_case(SANDBOX_USER)) {
        return Err("not the Windows Sandbox account (WDAGUtilityAccount)");
    }
    let env_token = env_token
        .map(str::trim)
        .filter(|t| t.len() == TOKEN_LEN && t.chars().all(|c| c.is_ascii_hexdigit()))
        .ok_or("CLEANSLATE_SANDBOX_RUN is missing or malformed (set by the .wsb file)")?;
    match file_token.map(str::trim) {
        Some(file) if file.eq_ignore_ascii_case(env_token) => Ok(()),
        Some(_) => Err("the run token differs from the one shared by prepare.ps1"),
        None => Err("no run token shared by prepare.ps1 (C:\\CleanSlate\\repo\\…\\token.txt)"),
    }
}

/// The guard with the real values of this process.
fn guard() -> Result<(), &'static str> {
    let file = std::fs::read_to_string(TOKEN_FILE).ok();
    check_guard(
        std::env::var("USERNAME").ok().as_deref(),
        std::env::var(TOKEN_VARIABLE).ok().as_deref(),
        file.as_deref(),
    )
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
    if let Err(reason) = guard() {
        eprintln!(
            "sandbox commands only run inside Windows Sandbox started by prepare.ps1 ({reason}): \
             nothing was modified."
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

    const TOKEN: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    /// The guard alone is tested: the actions never run outside the Sandbox.
    #[test]
    fn all_three_conditions_are_required() {
        let user = Some("WDAGUtilityAccount");
        assert_eq!(check_guard(user, Some(TOKEN), Some(TOKEN)), Ok(()));
        assert_eq!(
            check_guard(
                Some("wdagutilityaccount"),
                Some(TOKEN),
                Some(&format!("{TOKEN}\r\n"))
            ),
            Ok(())
        );
        // Each condition missing on its own refuses.
        for other in [
            None,
            Some(""),
            Some("HP"),
            Some("Administrator"),
            Some("WDAGUtilityAccount2"),
        ] {
            assert!(
                check_guard(other, Some(TOKEN), Some(TOKEN)).is_err(),
                "{other:?}"
            );
        }
        assert!(check_guard(user, None, Some(TOKEN)).is_err());
        assert!(check_guard(user, Some("short"), Some("short")).is_err());
        assert!(check_guard(user, Some(&"z".repeat(64)), Some(&"z".repeat(64))).is_err());
        assert!(check_guard(user, Some(TOKEN), None).is_err());
        assert!(check_guard(user, Some(TOKEN), Some(&TOKEN.replace('0', "1"))).is_err());
    }

    /// This development machine fails the guard: reading the real values
    /// only, no action is ever called.
    #[test]
    fn this_machine_is_refused() {
        assert!(guard().is_err());
    }
}
