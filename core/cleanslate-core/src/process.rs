//! The single way the engine starts an external program (D82): never through a shell
//! (rule 8), and never with a console window. CleanSlate is a windowed application: a
//! console program started from it would otherwise open a console that flashes on screen.
//!
//! The official uninstaller is the one exception: it is started by `uninstall.rs` with
//! `CreateProcessW` (Job Object, D56) because its own window must stay visible.

use std::ffi::OsStr;
use std::os::windows::process::CommandExt;
use std::process::Command;

/// `CREATE_NO_WINDOW`: the program gets no console window.
pub const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// A [`Command`] for `program`, which runs without any console window.
pub fn hidden(program: impl AsRef<OsStr>) -> Command {
    let mut command = Command::new(program);
    command.creation_flags(CREATE_NO_WINDOW);
    command
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    // Source of `file` without its unit tests (the tests may start `cmd` to build junctions).
    fn production_code(file: &Path) -> String {
        let text = std::fs::read_to_string(file).unwrap().replace("\r\n", "\n");
        match text.find("#[cfg(test)]\nmod tests") {
            Some(end) => text[..end].to_owned(),
            None => text,
        }
    }

    /// D82: every external program of the engine goes through [`hidden`].
    #[test]
    fn every_external_program_starts_through_hidden() {
        let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut offenders = Vec::new();
        for entry in std::fs::read_dir(&src).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().is_none_or(|e| e != "rs") || path.ends_with("process.rs") {
                continue;
            }
            let code = production_code(&path);
            for needle in ["Command::new(", "process::Command", "ShellExecute"] {
                if code.contains(needle) {
                    offenders.push(format!("{} ({needle})", path.display()));
                }
            }
            // CreateProcessW: only the official uninstaller, whose window must be seen.
            if code.contains("CreateProcessW(") && !path.ends_with("uninstall.rs") {
                offenders.push(format!("{} (CreateProcessW)", path.display()));
            }
        }
        assert!(
            offenders.is_empty(),
            "external programs outside process::hidden: {offenders:?}"
        );
    }

    #[test]
    fn the_flag_is_create_no_window() {
        // Value of winbase.h; the command keeps its program.
        assert_eq!(CREATE_NO_WINDOW, 0x0800_0000);
        assert_eq!(hidden("dism.exe").get_program(), "dism.exe");
    }
}
