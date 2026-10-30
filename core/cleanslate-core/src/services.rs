//! Windows services (EF-07, D47): their full configuration, saved as JSON
//! before `DeleteService`, and the control through which the quarantine
//! deletes and recreates them.
//!
//! The real control is the Service Control Manager; engines built for tests
//! get [`NoServiceControl`], which refuses everything, so that no test ever
//! creates or deletes a real service.

use serde::{Deserialize, Serialize};

use crate::error::{CoreError, CoreResult};

/// `SERVICE_STOPPED`.
pub const STATE_STOPPED: u32 = 1;
/// `SERVICE_WIN32_OWN_PROCESS | SERVICE_WIN32_SHARE_PROCESS`.
pub const WIN32_TYPES: u32 = 0x10 | 0x20;
/// Kernel, file system, adapter and recognizer drivers.
pub const DRIVER_TYPES: u32 = 0x1 | 0x2 | 0x4 | 0x8;

/// One recovery action (`SC_ACTION`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecoveryAction {
    /// 0 none, 1 restart, 2 reboot, 3 run command.
    pub kind: u32,
    /// Delay before the action, in milliseconds.
    pub delay_ms: u32,
}

/// Recovery settings (`SERVICE_FAILURE_ACTIONSW`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FailureActions {
    /// Seconds without failure after which the count is reset.
    pub reset_period_s: u32,
    /// Message broadcast before a reboot.
    pub reboot_message: Option<String>,
    /// Command run by a « run command » action.
    pub command: Option<String>,
    /// Actions, in order.
    pub actions: Vec<RecoveryAction>,
}

/// Everything needed to recreate a service (saved before `DeleteService`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServiceConfig {
    /// Service name (key below `Services`).
    pub name: String,
    /// Display name.
    pub display_name: String,
    /// `SERVICE_WIN32_OWN_PROCESS`…
    pub service_type: u32,
    /// 2 automatic, 3 manual, 4 disabled…
    pub start_type: u32,
    /// Automatic start delayed after boot.
    pub delayed_auto_start: bool,
    /// Severity of a start failure.
    pub error_control: u32,
    /// Command line (`ImagePath`), as stored.
    pub binary_path: String,
    /// Load ordering group.
    pub load_order_group: Option<String>,
    /// Services and groups (`+Group`) this one depends on.
    pub dependencies: Vec<String>,
    /// Account; `None` is LocalSystem.
    pub account: Option<String>,
    /// Description.
    pub description: Option<String>,
    /// Recovery settings.
    pub failure_actions: Option<FailureActions>,
}

/// A service as seen right before acting on it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServiceState {
    /// Its configuration.
    pub config: ServiceConfig,
    /// `SERVICE_STOPPED`, `SERVICE_RUNNING`…
    pub current_state: u32,
    /// Services that depend on it.
    pub dependents: Vec<String>,
}

/// `true` for the built-in accounts, the only ones a service can be
/// recreated with identically (no password to give back).
pub fn is_builtin_account(account: Option<&str>) -> bool {
    let Some(account) = account.map(str::trim).filter(|a| !a.is_empty()) else {
        return true; // LocalSystem
    };
    let account = account.to_ascii_lowercase();
    [
        "localsystem",
        r".\localsystem",
        r"nt authority\system",
        r"nt authority\localservice",
        r"nt authority\local service",
        r"nt authority\networkservice",
        r"nt authority\network service",
    ]
    .contains(&account.as_str())
}

/// Splits a `REG_MULTI_SZ`-style buffer (double NUL-terminated).
pub fn split_multi_sz(units: &[u16]) -> Vec<String> {
    units
        .split(|&u| u == 0)
        .take_while(|s| !s.is_empty())
        .map(String::from_utf16_lossy)
        .collect()
}

/// Builds a double NUL-terminated buffer.
pub fn join_multi_sz(items: &[String]) -> Vec<u16> {
    let mut units: Vec<u16> = items
        .iter()
        .flat_map(|s| s.encode_utf16().chain([0]))
        .collect();
    units.push(0);
    if items.is_empty() {
        units.push(0);
    }
    units
}

/// The quarantine queries, deletes and recreates services through this.
pub trait ServiceControl: Send + Sync {
    /// Configuration, state and dependents of the service `name`.
    fn query(&self, name: &str) -> CoreResult<ServiceState>;
    /// Deletes the service (`DeleteService`).
    fn delete(&self, name: &str) -> CoreResult<()>;
    /// Recreates a service from its saved configuration.
    fn create(&self, config: &ServiceConfig) -> CoreResult<()>;
    /// `true` if the service exists.
    fn exists(&self, name: &str) -> bool {
        self.query(name).is_ok()
    }
}

/// Refuses everything (test engines).
#[derive(Debug, Default)]
pub struct NoServiceControl;

impl ServiceControl for NoServiceControl {
    fn query(&self, name: &str) -> CoreResult<ServiceState> {
        Err(CoreError::InvalidInput(format!(
            "no service control on this engine: {name}"
        )))
    }
    fn delete(&self, name: &str) -> CoreResult<()> {
        self.query(name).map(|_| ())
    }
    fn create(&self, config: &ServiceConfig) -> CoreResult<()> {
        self.query(&config.name).map(|_| ())
    }
}

/// The Service Control Manager.
#[derive(Debug, Default)]
pub struct ScmServiceControl;

impl ServiceControl for ScmServiceControl {
    fn query(&self, name: &str) -> CoreResult<ServiceState> {
        scm::query(name)
    }
    fn delete(&self, name: &str) -> CoreResult<()> {
        scm::delete(name)
    }
    fn create(&self, config: &ServiceConfig) -> CoreResult<()> {
        scm::create(config)
    }
}

mod scm {
    use windows::core::{BOOL, PCWSTR, PWSTR};
    use windows::Win32::System::Services::{
        ChangeServiceConfig2W, CloseServiceHandle, CreateServiceW, DeleteService,
        EnumDependentServicesW, OpenSCManagerW, OpenServiceW, QueryServiceConfig2W,
        QueryServiceConfigW, QueryServiceStatus, ENUM_SERVICE_STATUSW, ENUM_SERVICE_TYPE,
        QUERY_SERVICE_CONFIGW, SC_ACTION, SC_ACTION_TYPE, SC_HANDLE, SC_MANAGER_CONNECT,
        SC_MANAGER_CREATE_SERVICE, SERVICE_ALL_ACCESS, SERVICE_CONFIG,
        SERVICE_CONFIG_DELAYED_AUTO_START_INFO, SERVICE_CONFIG_DESCRIPTION,
        SERVICE_CONFIG_FAILURE_ACTIONS, SERVICE_DELAYED_AUTO_START_INFO, SERVICE_DESCRIPTIONW,
        SERVICE_ENUMERATE_DEPENDENTS, SERVICE_ERROR, SERVICE_FAILURE_ACTIONSW,
        SERVICE_QUERY_CONFIG, SERVICE_QUERY_STATUS, SERVICE_START_TYPE, SERVICE_STATE_ALL,
        SERVICE_STATUS,
    };

    use super::{
        join_multi_sz, split_multi_sz, FailureActions, RecoveryAction, ServiceConfig, ServiceState,
    };
    use crate::error::{CoreError, CoreResult};

    /// Standard `DELETE` access right.
    const DELETE: u32 = 0x0001_0000;

    /// A service handle closed on drop.
    struct Handle(SC_HANDLE);

    impl Drop for Handle {
        fn drop(&mut self) {
            // SAFETY: the handle was returned by OpenSCManagerW, OpenServiceW
            // or CreateServiceW and is closed exactly once.
            let _ = unsafe { CloseServiceHandle(self.0) };
        }
    }

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain([0]).collect()
    }

    fn failed(name: &str) -> impl Fn(windows::core::Error) -> CoreError + '_ {
        move |e| CoreError::InvalidInput(format!("service {name}: {e}"))
    }

    fn manager(access: u32, name: &str) -> CoreResult<Handle> {
        // SAFETY: plain call; null names mean the local machine and the
        // active database.
        unsafe { OpenSCManagerW(PCWSTR::null(), PCWSTR::null(), access) }
            .map(Handle)
            .map_err(failed(name))
    }

    fn open(name: &str, access: u32) -> CoreResult<(Handle, Handle)> {
        let scm = manager(SC_MANAGER_CONNECT, name)?;
        let wide_name = wide(name);
        // SAFETY: NUL-terminated name alive for the call; valid manager handle.
        let service = unsafe { OpenServiceW(scm.0, PCWSTR(wide_name.as_ptr()), access) }
            .map(Handle)
            .map_err(failed(name))?;
        Ok((scm, service))
    }

    /// Text of a possibly null wide string.
    fn text(p: PWSTR) -> Option<String> {
        if p.is_null() {
            return None;
        }
        // SAFETY: non-null pointers point into the API's own buffer,
        // NUL-terminated, alive while the buffer is.
        unsafe { p.to_string() }.ok().filter(|s| !s.is_empty())
    }

    /// 8-byte aligned buffer of at least `bytes` bytes.
    fn buffer(bytes: u32) -> Vec<u64> {
        vec![0u64; (bytes as usize).div_ceil(8).max(1)]
    }

    /// Bytes view of an aligned buffer.
    fn as_bytes(buffer: &mut [u64]) -> &mut [u8] {
        let len = buffer.len() * 8;
        // SAFETY: u64 storage reinterpreted as bytes, same length, exclusive borrow.
        unsafe { std::slice::from_raw_parts_mut(buffer.as_mut_ptr().cast::<u8>(), len) }
    }

    fn config2(service: &Handle, level: SERVICE_CONFIG, name: &str) -> CoreResult<Vec<u64>> {
        let mut needed = 0u32;
        // SAFETY: size query with no buffer, then a call with an aligned
        // buffer of the size returned.
        unsafe {
            let _ = QueryServiceConfig2W(service.0, level, None, &mut needed);
            let mut data = buffer(needed);
            QueryServiceConfig2W(service.0, level, Some(as_bytes(&mut data)), &mut needed)
                .map_err(failed(name))?;
            Ok(data)
        }
    }

    pub(super) fn query(name: &str) -> CoreResult<ServiceState> {
        let access = SERVICE_QUERY_CONFIG | SERVICE_QUERY_STATUS | SERVICE_ENUMERATE_DEPENDENTS;
        let (_scm, service) = open(name, access)?;
        let mut state = ServiceState::default();
        // SAFETY: every buffer is aligned and as large as the API asked;
        // the structures read point into those buffers, which outlive the
        // reads below.
        unsafe {
            let mut needed = 0u32;
            let _ = QueryServiceConfigW(service.0, None, 0, &mut needed);
            let mut data = buffer(needed);
            let raw = data.as_mut_ptr().cast::<QUERY_SERVICE_CONFIGW>();
            QueryServiceConfigW(service.0, Some(raw), needed, &mut needed).map_err(failed(name))?;
            let c = &*raw;
            let dependencies = if c.lpDependencies.is_null() {
                Vec::new()
            } else {
                let len = (0..)
                    .take_while(|&i| {
                        *c.lpDependencies.0.add(i) != 0 || *c.lpDependencies.0.add(i + 1) != 0
                    })
                    .count();
                split_multi_sz(std::slice::from_raw_parts(c.lpDependencies.0, len + 2))
            };
            state.config = ServiceConfig {
                name: name.to_owned(),
                display_name: text(c.lpDisplayName).unwrap_or_else(|| name.to_owned()),
                service_type: c.dwServiceType.0,
                start_type: c.dwStartType.0,
                error_control: c.dwErrorControl.0,
                binary_path: text(c.lpBinaryPathName).unwrap_or_default(),
                load_order_group: text(c.lpLoadOrderGroup),
                dependencies,
                account: text(c.lpServiceStartName),
                ..ServiceConfig::default()
            };

            if let Ok(data) = config2(&service, SERVICE_CONFIG_DESCRIPTION, name) {
                let d = &*data.as_ptr().cast::<SERVICE_DESCRIPTIONW>();
                state.config.description = text(d.lpDescription);
            }
            if let Ok(data) = config2(&service, SERVICE_CONFIG_DELAYED_AUTO_START_INFO, name) {
                let d = &*data.as_ptr().cast::<SERVICE_DELAYED_AUTO_START_INFO>();
                state.config.delayed_auto_start = d.fDelayedAutostart.as_bool();
            }
            if let Ok(data) = config2(&service, SERVICE_CONFIG_FAILURE_ACTIONS, name) {
                let f = &*data.as_ptr().cast::<SERVICE_FAILURE_ACTIONSW>();
                let actions = if f.lpsaActions.is_null() {
                    Vec::new()
                } else {
                    std::slice::from_raw_parts(f.lpsaActions, f.cActions as usize)
                        .iter()
                        .map(|a| RecoveryAction {
                            kind: a.Type.0 as u32,
                            delay_ms: a.Delay,
                        })
                        .collect()
                };
                let failure = FailureActions {
                    reset_period_s: f.dwResetPeriod,
                    reboot_message: text(f.lpRebootMsg),
                    command: text(f.lpCommand),
                    actions,
                };
                if failure != FailureActions::default() {
                    state.config.failure_actions = Some(failure);
                }
            }

            let mut status = SERVICE_STATUS::default();
            QueryServiceStatus(service.0, &mut status).map_err(failed(name))?;
            state.current_state = status.dwCurrentState.0;

            let (mut needed, mut count) = (0u32, 0u32);
            let _ = EnumDependentServicesW(
                service.0,
                SERVICE_STATE_ALL,
                None,
                0,
                &mut needed,
                &mut count,
            );
            if needed > 0 {
                let mut data = buffer(needed);
                let raw = data.as_mut_ptr().cast::<ENUM_SERVICE_STATUSW>();
                EnumDependentServicesW(
                    service.0,
                    SERVICE_STATE_ALL,
                    Some(raw),
                    needed,
                    &mut needed,
                    &mut count,
                )
                .map_err(failed(name))?;
                state.dependents = std::slice::from_raw_parts(raw, count as usize)
                    .iter()
                    .filter_map(|s| text(s.lpServiceName))
                    .collect();
            }
        }
        Ok(state)
    }

    pub(super) fn delete(name: &str) -> CoreResult<()> {
        let (_scm, service) = open(name, DELETE)?;
        // SAFETY: valid handle opened with DELETE access.
        unsafe { DeleteService(service.0) }.map_err(failed(name))
    }

    pub(super) fn create(config: &ServiceConfig) -> CoreResult<()> {
        let name = config.name.as_str();
        let scm = manager(SC_MANAGER_CONNECT | SC_MANAGER_CREATE_SERVICE, name)?;
        let service_name = wide(name);
        let display = wide(&config.display_name);
        let binary = wide(&config.binary_path);
        let group = config.load_order_group.as_deref().map(wide);
        let dependencies =
            (!config.dependencies.is_empty()).then(|| join_multi_sz(&config.dependencies));
        let account = config.account.as_deref().map(wide);
        let ptr = |v: &Option<Vec<u16>>| v.as_ref().map_or(PCWSTR::null(), |w| PCWSTR(w.as_ptr()));
        // SAFETY: every string buffer is NUL-terminated (double NUL for the
        // dependencies) and lives until the end of this function; built-in
        // accounts need no password.
        let service = unsafe {
            CreateServiceW(
                scm.0,
                PCWSTR(service_name.as_ptr()),
                PCWSTR(display.as_ptr()),
                SERVICE_ALL_ACCESS,
                ENUM_SERVICE_TYPE(config.service_type),
                SERVICE_START_TYPE(config.start_type),
                SERVICE_ERROR(config.error_control),
                PCWSTR(binary.as_ptr()),
                ptr(&group),
                None,
                ptr(&dependencies),
                ptr(&account),
                PCWSTR::null(),
            )
        }
        .map(Handle)
        .map_err(failed(name))?;

        let mut description = config.description.as_deref().map(wide);
        if let Some(d) = description.as_mut() {
            let info = SERVICE_DESCRIPTIONW {
                lpDescription: PWSTR(d.as_mut_ptr()),
            };
            // SAFETY: the structure and its string live for the call.
            unsafe {
                ChangeServiceConfig2W(
                    service.0,
                    SERVICE_CONFIG_DESCRIPTION,
                    Some((&info as *const SERVICE_DESCRIPTIONW).cast()),
                )
            }
            .map_err(failed(name))?;
        }
        if config.delayed_auto_start {
            let info = SERVICE_DELAYED_AUTO_START_INFO {
                fDelayedAutostart: BOOL::from(true),
            };
            // SAFETY: the structure lives for the call.
            unsafe {
                ChangeServiceConfig2W(
                    service.0,
                    SERVICE_CONFIG_DELAYED_AUTO_START_INFO,
                    Some((&info as *const SERVICE_DELAYED_AUTO_START_INFO).cast()),
                )
            }
            .map_err(failed(name))?;
        }
        if let Some(failure) = &config.failure_actions {
            let mut actions: Vec<SC_ACTION> = failure
                .actions
                .iter()
                .map(|a| SC_ACTION {
                    Type: SC_ACTION_TYPE(a.kind as i32),
                    Delay: a.delay_ms,
                })
                .collect();
            let mut reboot = failure.reboot_message.as_deref().map(wide);
            let mut command = failure.command.as_deref().map(wide);
            let mut_ptr = |v: &mut Option<Vec<u16>>| {
                v.as_mut().map_or(PWSTR::null(), |w| PWSTR(w.as_mut_ptr()))
            };
            let info = SERVICE_FAILURE_ACTIONSW {
                dwResetPeriod: failure.reset_period_s,
                lpRebootMsg: mut_ptr(&mut reboot),
                lpCommand: mut_ptr(&mut command),
                cActions: actions.len() as u32,
                lpsaActions: actions.as_mut_ptr(),
            };
            // SAFETY: the structure, its strings and its actions live for the call.
            let result = unsafe {
                ChangeServiceConfig2W(
                    service.0,
                    SERVICE_CONFIG_FAILURE_ACTIONS,
                    Some((&info as *const SERVICE_FAILURE_ACTIONSW).cast()),
                )
            };
            // A reboot action needs a privilege the session may lack: the
            // registry snapshot reimported next carries the same settings.
            if let Err(err) = result {
                tracing::warn!(service = name, %err, "failure actions not set through the SCM");
            }
        }
        Ok(())
    }
}

/// In-memory control for the tests: no real service is ever touched.
#[cfg(test)]
#[derive(Debug, Default)]
pub(crate) struct FakeServiceControl(
    pub std::sync::Mutex<std::collections::BTreeMap<String, ServiceState>>,
);

#[cfg(test)]
impl ServiceControl for FakeServiceControl {
    fn query(&self, name: &str) -> CoreResult<ServiceState> {
        self.0
            .lock()
            .unwrap()
            .get(&name.to_lowercase())
            .cloned()
            .ok_or_else(|| CoreError::NotFound {
                location: name.to_owned(),
            })
    }
    fn delete(&self, name: &str) -> CoreResult<()> {
        self.0
            .lock()
            .unwrap()
            .remove(&name.to_lowercase())
            .map(|_| ())
            .ok_or_else(|| CoreError::NotFound {
                location: name.to_owned(),
            })
    }
    fn create(&self, config: &ServiceConfig) -> CoreResult<()> {
        let mut services = self.0.lock().unwrap();
        if services.contains_key(&config.name.to_lowercase()) {
            return Err(CoreError::InvalidInput(format!(
                "service exists: {}",
                config.name
            )));
        }
        services.insert(
            config.name.to_lowercase(),
            ServiceState {
                config: config.clone(),
                current_state: STATE_STOPPED,
                dependents: Vec::new(),
            },
        );
        Ok(())
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A complete configuration, every field set.
    pub(crate) fn full_config(name: &str, binary: &str) -> ServiceConfig {
        ServiceConfig {
            name: name.into(),
            display_name: format!("{name} (ancien)"),
            service_type: 0x10,
            start_type: 2,
            delayed_auto_start: true,
            error_control: 1,
            binary_path: binary.into(),
            load_order_group: Some("Extended Base".into()),
            dependencies: vec!["Tcpip".into(), "+NetworkProvider".into()],
            account: Some(r"NT AUTHORITY\LocalService".into()),
            description: Some("Agent de mise à jour".into()),
            failure_actions: Some(FailureActions {
                reset_period_s: 86_400,
                reboot_message: None,
                command: Some(r#""C:\Gone\fix.exe" /now"#.into()),
                actions: vec![
                    RecoveryAction {
                        kind: 1,
                        delay_ms: 60_000,
                    },
                    RecoveryAction {
                        kind: 3,
                        delay_ms: 120_000,
                    },
                ],
            }),
        }
    }

    #[test]
    fn configuration_round_trips_through_json() {
        let config = full_config("OldSvc", r#""C:\Gone\svc.exe" -run"#);
        let json = serde_json::to_string_pretty(&config).unwrap();
        let back: ServiceConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(back, config);
        // Reconstruction through the control gives the same configuration.
        let control = FakeServiceControl::default();
        control.create(&back).unwrap();
        assert_eq!(control.query("oldsvc").unwrap().config, config);
        assert!(control.create(&back).is_err(), "never overwrites");
    }

    #[test]
    fn multi_strings_round_trip() {
        let items = vec!["Tcpip".to_owned(), "+Group".to_owned()];
        let units = join_multi_sz(&items);
        assert_eq!(units.last(), Some(&0));
        assert_eq!(split_multi_sz(&units), items);
        assert!(split_multi_sz(&join_multi_sz(&[])).is_empty());
    }

    #[test]
    fn builtin_accounts_only() {
        for account in [
            None,
            Some(""),
            Some("LocalSystem"),
            Some(r"NT AUTHORITY\LocalService"),
            Some(r"nt authority\NetworkService"),
            Some(r"NT AUTHORITY\SYSTEM"),
        ] {
            assert!(is_builtin_account(account), "{account:?}");
        }
        for account in [Some(r".\Ana"), Some(r"DOMAIN\svc-backup"), Some("svc@corp")] {
            assert!(!is_builtin_account(account), "{account:?}");
        }
    }

    #[test]
    fn test_control_refuses() {
        assert!(NoServiceControl.query("X").is_err());
        assert!(NoServiceControl.delete("X").is_err());
        assert!(NoServiceControl.create(&ServiceConfig::default()).is_err());
        assert!(!NoServiceControl.exists("X"));
    }
}
