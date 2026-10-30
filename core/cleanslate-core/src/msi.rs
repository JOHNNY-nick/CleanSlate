//! Uninstalling an MSI product through the Windows Installer API (EF-21):
//! `MsiConfigureProductExW(INSTALLSTATE_ABSENT)` with an external UI
//! handler (`MsiSetExternalUIRecord`) that receives the real progress
//! messages. When the API fails, the engine falls back to `msiexec /x`
//! with an indeterminate progress.
//!
//! The percentage follows the algorithm documented by Windows Installer
//! (« Parsing Windows Installer Messages »): `PROGRESS` records reset the
//! total, add ticks or announce how much each `ACTIONDATA` is worth.

use std::sync::atomic::AtomicBool;

use crate::error::{CoreError, CoreResult};

/// Exit codes of a successful (or user-cancelled) uninstall.
pub const SUCCESS_CODES: [u32; 4] = [
    0,    // ERROR_SUCCESS
    1602, // ERROR_INSTALL_USEREXIT
    1641, // ERROR_SUCCESS_REBOOT_INITIATED
    3010, // ERROR_SUCCESS_REBOOT_REQUIRED
];

/// Percentage computed from the `INSTALLMESSAGE_PROGRESS` records.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MsiProgress {
    total: i64,
    done: i64,
    forward: bool,
    per_action_data: i64,
    use_action_data: bool,
    /// Generating the script (before execution): not reported as progress.
    scripting: bool,
}

impl MsiProgress {
    /// One `PROGRESS` record, fields 1 to 4.
    pub fn on_progress(&mut self, fields: [i32; 4]) {
        let [kind, a, b, c] = fields.map(i64::from);
        match kind {
            // Reset: total ticks, direction (0 forward), 1 while scripting.
            0 => {
                self.total = a.max(0);
                self.forward = b == 0;
                self.done = if self.forward { 0 } else { self.total };
                self.use_action_data = false;
                self.scripting = c == 1;
            }
            // Action info: ticks per ACTIONDATA when field 3 is 1.
            1 => {
                self.per_action_data = a;
                self.use_action_data = b == 1;
            }
            // Progress report: ticks done.
            2 => self.advance(a),
            // More ticks expected.
            3 => self.total += a.max(0),
            _ => {}
        }
    }

    /// One `ACTIONDATA` message.
    pub fn on_action_data(&mut self) {
        if self.use_action_data {
            self.advance(self.per_action_data);
        }
    }

    fn advance(&mut self, ticks: i64) {
        self.done += if self.forward { ticks } else { -ticks };
        self.done = self.done.clamp(0, self.total.max(0));
    }

    /// 0 to 100 during the execution phase; `None` before (scripting, no total).
    pub fn percent(&self) -> Option<f64> {
        (!self.scripting && self.total > 0).then(|| 100.0 * self.done as f64 / self.total as f64)
    }
}

/// Uninstalls an MSI product and reports its progress.
pub trait MsiUninstaller: Send + Sync {
    /// Removes `product_code`; `report` receives the percentage and the
    /// action in progress; `cancel` asks Windows Installer to roll back.
    /// `Err` means the API could not be used: the engine falls back to
    /// `msiexec /x`. `Ok` carries the exit code.
    fn uninstall(
        &self,
        product_code: &str,
        cancel: &AtomicBool,
        report: &dyn Fn(Option<f64>, Option<String>),
    ) -> CoreResult<u32>;
}

/// Never uses the API (test engines): the engine falls back to its runner.
#[derive(Debug, Default)]
pub struct NoMsiUninstaller;

impl MsiUninstaller for NoMsiUninstaller {
    fn uninstall(
        &self,
        product_code: &str,
        _: &AtomicBool,
        _: &dyn Fn(Option<f64>, Option<String>),
    ) -> CoreResult<u32> {
        Err(CoreError::InvalidInput(format!(
            "no Windows Installer API on this engine: {product_code}"
        )))
    }
}

/// The Windows Installer API.
#[derive(Debug, Default)]
pub struct ApiMsiUninstaller;

impl MsiUninstaller for ApiMsiUninstaller {
    fn uninstall(
        &self,
        product_code: &str,
        cancel: &AtomicBool,
        report: &dyn Fn(Option<f64>, Option<String>),
    ) -> CoreResult<u32> {
        api::uninstall(product_code, cancel, report)
    }
}

mod api {
    use std::cell::RefCell;
    use std::ffi::c_void;
    use std::panic::{catch_unwind, AssertUnwindSafe};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::{Duration, Instant};

    use windows::core::{PCWSTR, PWSTR};
    use windows::Win32::System::ApplicationInstallationAndServicing::{
        MsiConfigureProductExW, MsiRecordGetInteger, MsiRecordGetStringW, MsiSetExternalUIRecord,
        MsiSetInternalUI, INSTALLLEVEL_DEFAULT, INSTALLLOGMODE_ACTIONDATA,
        INSTALLLOGMODE_ACTIONSTART, INSTALLLOGMODE_PROGRESS, INSTALLMESSAGE_ACTIONDATA,
        INSTALLMESSAGE_ACTIONSTART, INSTALLMESSAGE_PROGRESS, INSTALLSTATE_ABSENT,
        INSTALLUILEVEL_NONE, MSIHANDLE,
    };

    use super::{MsiProgress, SUCCESS_CODES};
    use crate::error::{CoreError, CoreResult};

    const IDOK: i32 = 1;
    const IDCANCEL: i32 = 2;
    const INTERVAL: Duration = Duration::from_millis(250);

    /// State shared with the handler, on the calling thread.
    struct Context<'a> {
        progress: RefCell<MsiProgress>,
        action: RefCell<Option<String>>,
        last: RefCell<Instant>,
        cancel: &'a AtomicBool,
        report: &'a dyn Fn(Option<f64>, Option<String>),
    }

    fn field_text(record: MSIHANDLE, field: u32) -> Option<String> {
        let mut len = 0u32;
        // SAFETY: size query (null buffer), then a buffer of len + 1
        // units; the record handle is valid during the callback.
        unsafe {
            let mut empty = [0u16; 1];
            let _ = MsiRecordGetStringW(
                record,
                field,
                Some(PWSTR(empty.as_mut_ptr())),
                Some(&mut len),
            );
            let mut buffer = vec![0u16; len as usize + 1];
            len += 1;
            if MsiRecordGetStringW(
                record,
                field,
                Some(PWSTR(buffer.as_mut_ptr())),
                Some(&mut len),
            ) != 0
            {
                return None;
            }
            buffer.truncate(len as usize);
            Some(String::from_utf16_lossy(&buffer)).filter(|s| !s.is_empty())
        }
    }

    /// External UI handler (`INSTALLUI_HANDLER_RECORD`).
    unsafe extern "system" fn handler(
        context: *mut c_void,
        message: u32,
        record: MSIHANDLE,
    ) -> i32 {
        // No panic may cross back into Windows Installer.
        catch_unwind(AssertUnwindSafe(|| {
            // SAFETY: `context` is the `Context` passed to
            // MsiSetExternalUIRecord, alive until MsiConfigureProductExW
            // returns; the handler runs on that same thread.
            let context = unsafe { &*(context as *const Context) };
            if context.cancel.load(Ordering::Relaxed) {
                return IDCANCEL;
            }
            let kind = (message & 0xFF00_0000) as i32;
            let mut force = false;
            if kind == INSTALLMESSAGE_PROGRESS.0 {
                // SAFETY: valid record during the callback; fields 1 to 4.
                let fields = [1, 2, 3, 4].map(|f| unsafe { MsiRecordGetInteger(record, f) });
                context.progress.borrow_mut().on_progress(fields);
            } else if kind == INSTALLMESSAGE_ACTIONDATA.0 {
                context.progress.borrow_mut().on_action_data();
            } else if kind == INSTALLMESSAGE_ACTIONSTART.0 {
                // Field 2: description of the action, else field 1: its name.
                *context.action.borrow_mut() =
                    field_text(record, 2).or_else(|| field_text(record, 1));
                force = true;
            } else {
                return 0;
            }
            let mut last = context.last.borrow_mut();
            if force || last.elapsed() >= INTERVAL {
                *last = Instant::now();
                (context.report)(
                    context.progress.borrow().percent(),
                    context.action.borrow().clone(),
                );
            }
            IDOK
        }))
        .unwrap_or(-1)
    }

    pub(super) fn uninstall(
        product_code: &str,
        cancel: &AtomicBool,
        report: &dyn Fn(Option<f64>, Option<String>),
    ) -> CoreResult<u32> {
        let context = Context {
            progress: RefCell::new(MsiProgress::default()),
            action: RefCell::new(None),
            last: RefCell::new(Instant::now() - INTERVAL),
            cancel,
            report,
        };
        let product: Vec<u16> = product_code.encode_utf16().chain([0]).collect();
        // Never restart the machine, never prompt for files in use.
        let command: Vec<u16> = "REBOOT=ReallySuppress MSIRESTARTMANAGERCONTROL=Disable"
            .encode_utf16()
            .chain([0])
            .collect();
        let filter = (INSTALLLOGMODE_PROGRESS.0
            | INSTALLLOGMODE_ACTIONSTART.0
            | INSTALLLOGMODE_ACTIONDATA.0) as u32;
        // SAFETY: the handler and its context live until the call below
        // returns; the previous UI settings are restored afterwards; the
        // strings are NUL-terminated and outlive the call.
        let code = unsafe {
            let previous_ui = MsiSetInternalUI(INSTALLUILEVEL_NONE, None);
            let set = MsiSetExternalUIRecord(
                Some(handler),
                filter,
                Some(&context as *const Context as *const c_void),
                None,
            );
            if set != 0 {
                MsiSetInternalUI(previous_ui, None);
                return Err(CoreError::InvalidInput(format!(
                    "MsiSetExternalUIRecord: erreur {set}"
                )));
            }
            let code = MsiConfigureProductExW(
                PCWSTR(product.as_ptr()),
                INSTALLLEVEL_DEFAULT,
                INSTALLSTATE_ABSENT,
                PCWSTR(command.as_ptr()),
            );
            let _ = MsiSetExternalUIRecord(None, 0, None, None);
            MsiSetInternalUI(previous_ui, None);
            code
        };
        if SUCCESS_CODES.contains(&code) {
            report(Some(100.0), context.action.borrow().clone());
            Ok(code)
        } else {
            Err(CoreError::InvalidInput(format!(
                "MsiConfigureProductExW: erreur {code}"
            )))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forward_progress_with_reports_and_action_data() {
        let mut p = MsiProgress::default();
        assert_eq!(p.percent(), None);
        // Script generation first: never reported.
        p.on_progress([0, 1000, 0, 1]);
        p.on_progress([2, 500, 0, 0]);
        assert_eq!(p.percent(), None);
        // Execution: 2000 ticks forward.
        p.on_progress([0, 2000, 0, 0]);
        assert_eq!(p.percent(), Some(0.0));
        p.on_progress([2, 500, 0, 0]);
        assert_eq!(p.percent(), Some(25.0));
        // Each ACTIONDATA is worth 100 ticks from now on.
        p.on_progress([1, 100, 1, 0]);
        for _ in 0..5 {
            p.on_action_data();
        }
        assert_eq!(p.percent(), Some(50.0));
        // More ticks expected: the percentage goes down accordingly.
        p.on_progress([3, 2000, 0, 0]);
        assert_eq!(p.percent(), Some(25.0));
        // Never above 100.
        p.on_progress([2, 99_999, 0, 0]);
        assert_eq!(p.percent(), Some(100.0));
    }

    #[test]
    fn backward_progress_and_ignored_action_data() {
        let mut p = MsiProgress::default();
        p.on_progress([0, 400, 1, 0]); // backwards (rollback)
        assert_eq!(p.percent(), Some(100.0));
        p.on_progress([2, 100, 0, 0]);
        assert_eq!(p.percent(), Some(75.0));
        p.on_action_data(); // not enabled: no change
        assert_eq!(p.percent(), Some(75.0));
        p.on_progress([9, 1, 1, 1]); // unknown kind: ignored
        assert_eq!(p.percent(), Some(75.0));
    }

    #[test]
    fn test_engine_never_uses_the_api() {
        let cancel = AtomicBool::new(false);
        assert!(NoMsiUninstaller
            .uninstall(
                "{00000000-0000-0000-0000-000000000000}",
                &cancel,
                &|_, _| {}
            )
            .is_err());
        assert!(SUCCESS_CODES.contains(&3010) && !SUCCESS_CODES.contains(&1603));
    }
}
