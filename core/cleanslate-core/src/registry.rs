//! Registry access, behind a trait so tests never touch the real hives.
//!
//! [`WinRegistry::system`] reads the real registry. [`WinRegistry::sandboxed`]
//! redirects every hive below `HKCU\Software\CleanSlateTest\…`, the only key
//! tests may write to (safety rule 2). Read-only at this stage.

use std::fmt;

use winreg::enums::{
    RegType, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ, KEY_WOW64_32KEY, KEY_WOW64_64KEY,
};
use winreg::types::FromRegValue;
use winreg::{RegKey, RegValue};

use crate::error::{CoreError, CoreResult};

/// Root of the only registry area tests may write to.
pub const TEST_ROOT: &str = r"Software\CleanSlateTest";

/// Registry hive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Hive {
    /// `HKEY_CURRENT_USER`.
    CurrentUser,
    /// `HKEY_LOCAL_MACHINE`.
    LocalMachine,
}

impl Hive {
    /// Short name (`HKCU`, `HKLM`).
    pub fn short_name(self) -> &'static str {
        match self {
            Hive::CurrentUser => "HKCU",
            Hive::LocalMachine => "HKLM",
        }
    }
}

/// Which registry view to open (WOW64 redirection).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum KeyView {
    /// View of the current process.
    #[default]
    Default,
    /// Force the 64-bit view (`KEY_WOW64_64KEY`).
    Force64,
    /// Force the 32-bit view (`KEY_WOW64_32KEY`).
    Force32,
}

/// Full address of a registry key.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RegKeyPath {
    /// Hive.
    pub hive: Hive,
    /// Path below the hive, `\`-separated, without a leading `\`.
    pub path: String,
    /// WOW64 view.
    pub view: KeyView,
}

impl RegKeyPath {
    /// Builds a key path in the default view.
    pub fn new(hive: Hive, path: impl Into<String>) -> Self {
        RegKeyPath {
            hive,
            path: path.into(),
            view: KeyView::Default,
        }
    }

    /// Returns the same key in another view.
    pub fn with_view(mut self, view: KeyView) -> Self {
        self.view = view;
        self
    }

    /// Returns the path of the sub-key `name`.
    pub fn join(&self, name: &str) -> Self {
        let path = if self.path.is_empty() {
            name.to_owned()
        } else {
            format!(r"{}\{}", self.path, name)
        };
        RegKeyPath {
            path,
            ..self.clone()
        }
    }
}

impl fmt::Display for RegKeyPath {
    /// Formats as `HKLM\SOFTWARE\…`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.path.is_empty() {
            write!(f, "{}", self.hive.short_name())
        } else {
            write!(f, r"{}\{}", self.hive.short_name(), self.path)
        }
    }
}

/// Typed content of a registry value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegData {
    /// `REG_SZ`.
    String(String),
    /// `REG_EXPAND_SZ` (not expanded).
    ExpandString(String),
    /// `REG_MULTI_SZ`.
    MultiString(Vec<String>),
    /// `REG_DWORD`.
    Dword(u32),
    /// `REG_QWORD`.
    Qword(u64),
    /// `REG_BINARY` and every other type, as raw bytes.
    Binary(Vec<u8>),
}

impl RegData {
    /// Returns the text of a `REG_SZ` or `REG_EXPAND_SZ` value.
    pub fn as_str(&self) -> Option<&str> {
        match self {
            RegData::String(s) | RegData::ExpandString(s) => Some(s),
            _ => None,
        }
    }

    /// Returns a `REG_DWORD` or `REG_QWORD` value as `u64`.
    pub fn as_u64(&self) -> Option<u64> {
        match self {
            RegData::Dword(v) => Some(u64::from(*v)),
            RegData::Qword(v) => Some(*v),
            _ => None,
        }
    }

    fn from_winreg(value: &RegValue) -> Self {
        let decoded = match value.vtype {
            RegType::REG_SZ => String::from_reg_value(value).ok().map(RegData::String),
            RegType::REG_EXPAND_SZ => String::from_reg_value(value)
                .ok()
                .map(RegData::ExpandString),
            RegType::REG_MULTI_SZ => Vec::<String>::from_reg_value(value)
                .ok()
                .map(RegData::MultiString),
            RegType::REG_DWORD => u32::from_reg_value(value).ok().map(RegData::Dword),
            RegType::REG_QWORD => u64::from_reg_value(value).ok().map(RegData::Qword),
            _ => None,
        };
        decoded.unwrap_or_else(|| RegData::Binary(value.bytes.to_vec()))
    }
}

/// Read access to the registry.
pub trait Registry: Send + Sync {
    /// Lists the names of the direct sub-keys of `key`.
    fn subkeys(&self, key: &RegKeyPath) -> CoreResult<Vec<String>>;

    /// Lists the values of `key` (the default value has an empty name).
    fn values(&self, key: &RegKeyPath) -> CoreResult<Vec<(String, RegData)>>;

    /// Returns `true` if `key` exists and can be read.
    fn key_exists(&self, key: &RegKeyPath) -> bool {
        self.subkeys(key).is_ok()
    }
}

/// The Windows registry, real or redirected to a test area.
#[derive(Debug, Clone)]
pub struct WinRegistry {
    /// `None`: real hives. `Some(prefix)`: every hive below `HKCU\<prefix>`.
    sandbox: Option<String>,
}

impl WinRegistry {
    /// Reads the real registry.
    pub fn system() -> Self {
        WinRegistry { sandbox: None }
    }

    /// Redirects `HKLM\X` to `HKCU\<prefix>\HKLM\X` and `HKCU\X` to
    /// `HKCU\<prefix>\HKCU\X`. `prefix` must be below [`TEST_ROOT`].
    pub fn sandboxed(prefix: impl Into<String>) -> CoreResult<Self> {
        let prefix = prefix.into();
        let inside_test_root = prefix
            .get(..TEST_ROOT.len() + 1)
            .is_some_and(|head| head.eq_ignore_ascii_case(&format!(r"{TEST_ROOT}\")));
        if !inside_test_root {
            return Err(CoreError::InvalidInput(format!(
                r"registry sandbox must be below HKCU\{TEST_ROOT}: {prefix}"
            )));
        }
        Ok(WinRegistry {
            sandbox: Some(prefix),
        })
    }

    fn open(&self, key: &RegKeyPath) -> CoreResult<RegKey> {
        let (root, path, view_flag) = match &self.sandbox {
            Some(prefix) => {
                let path = if key.path.is_empty() {
                    format!(r"{prefix}\{}", key.hive.short_name())
                } else {
                    format!(r"{prefix}\{}\{}", key.hive.short_name(), key.path)
                };
                // HKCU\Software is not redirected by WOW64: the view is moot.
                (RegKey::predef(HKEY_CURRENT_USER), path, 0)
            }
            None => {
                let root = match key.hive {
                    Hive::CurrentUser => RegKey::predef(HKEY_CURRENT_USER),
                    Hive::LocalMachine => RegKey::predef(HKEY_LOCAL_MACHINE),
                };
                let view_flag = match key.view {
                    KeyView::Default => 0,
                    KeyView::Force64 => KEY_WOW64_64KEY,
                    KeyView::Force32 => KEY_WOW64_32KEY,
                };
                (root, key.path.clone(), view_flag)
            }
        };
        root.open_subkey_with_flags(&path, KEY_READ | view_flag)
            .map_err(|e| CoreError::from_registry(key.to_string(), e))
    }
}

impl Registry for WinRegistry {
    fn subkeys(&self, key: &RegKeyPath) -> CoreResult<Vec<String>> {
        let opened = self.open(key)?;
        opened
            .enum_keys()
            .map(|name| name.map_err(|e| CoreError::from_registry(key.to_string(), e)))
            .collect()
    }

    fn values(&self, key: &RegKeyPath) -> CoreResult<Vec<(String, RegData)>> {
        let opened = self.open(key)?;
        opened
            .enum_values()
            .map(|item| {
                item.map(|(name, value)| (name, RegData::from_winreg(&value)))
                    .map_err(|e| CoreError::from_registry(key.to_string(), e))
            })
            .collect()
    }
}

/// Test helper: creates `HKCU\Software\CleanSlateTest\<uuid>` and deletes it
/// when dropped, even if the test fails.
#[cfg(test)]
pub(crate) struct TestRegistryKey {
    prefix: String,
}

#[cfg(test)]
impl TestRegistryKey {
    /// Creates a fresh, empty test area.
    pub(crate) fn new() -> Self {
        let prefix = format!(r"{TEST_ROOT}\{}", uuid::Uuid::new_v4());
        RegKey::predef(HKEY_CURRENT_USER)
            .create_subkey(&prefix)
            .expect("create test registry key");
        TestRegistryKey { prefix }
    }

    /// Registry reading this test area.
    pub(crate) fn registry(&self) -> WinRegistry {
        WinRegistry::sandboxed(self.prefix.clone()).expect("prefix is below TEST_ROOT")
    }

    /// Creates `key` (redirected into the test area) and returns it for writing.
    pub(crate) fn create(&self, key: &RegKeyPath) -> RegKey {
        let path = format!(r"{}\{}\{}", self.prefix, key.hive.short_name(), key.path);
        RegKey::predef(HKEY_CURRENT_USER)
            .create_subkey(path)
            .expect("create test sub-key")
            .0
    }
}

#[cfg(test)]
impl Drop for TestRegistryKey {
    fn drop(&mut self) {
        // Only ever deletes a key below TEST_ROOT, created by `new`.
        let _ = RegKey::predef(HKEY_CURRENT_USER).delete_subkey_all(&self.prefix);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn uninstall_key() -> RegKeyPath {
        RegKeyPath::new(
            Hive::LocalMachine,
            r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall",
        )
    }

    #[test]
    fn key_path_display_and_join() {
        let key = uninstall_key().join("7-Zip");
        assert_eq!(
            key.to_string(),
            r"HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\7-Zip"
        );
        assert_eq!(RegKeyPath::new(Hive::CurrentUser, "").to_string(), "HKCU");
        assert_eq!(
            RegKeyPath::new(Hive::CurrentUser, "").join("Software").path,
            "Software"
        );
    }

    #[test]
    fn sandbox_must_stay_below_test_root() {
        assert!(WinRegistry::sandboxed(r"Software\CleanSlateTest\abc").is_ok());
        assert!(WinRegistry::sandboxed(r"software\cleanslatetest\abc").is_ok());
        for bad in [
            r"Software",
            r"Software\CleanSlateTest",
            r"Software\CleanSlateTestX\abc",
            r"Software\Microsoft",
            "",
        ] {
            assert_eq!(
                WinRegistry::sandboxed(bad).unwrap_err().code(),
                "INVALID_INPUT",
                "{bad}"
            );
        }
    }

    #[test]
    fn sandboxed_registry_reads_test_values() {
        let area = TestRegistryKey::new();
        let app = uninstall_key().join("7-Zip");
        let key = area.create(&app);
        key.set_value("DisplayName", &"7-Zip 24.08 (x64)").unwrap();
        key.set_value("EstimatedSize", &5_800u32).unwrap();
        key.set_value("Big", &7u64).unwrap();
        key.set_value("Tags", &vec!["a", "b"]).unwrap();
        key.set_raw_value(
            "Raw",
            &RegValue {
                bytes: vec![1u8, 2, 3].into(),
                vtype: RegType::REG_BINARY,
            },
        )
        .unwrap();
        area.create(&app.join("Child"));

        let reg = area.registry();
        assert_eq!(reg.subkeys(&uninstall_key()).unwrap(), ["7-Zip"]);
        assert_eq!(reg.subkeys(&app).unwrap(), ["Child"]);
        assert!(reg.key_exists(&app));

        let mut values = reg.values(&app).unwrap();
        values.sort_by(|a, b| a.0.cmp(&b.0));
        let find = |name: &str| values.iter().find(|(n, _)| n == name).map(|(_, v)| v);
        assert_eq!(
            find("DisplayName").and_then(RegData::as_str),
            Some("7-Zip 24.08 (x64)")
        );
        assert_eq!(find("EstimatedSize").and_then(RegData::as_u64), Some(5_800));
        assert_eq!(find("Big"), Some(&RegData::Qword(7)));
        assert_eq!(
            find("Tags"),
            Some(&RegData::MultiString(vec!["a".into(), "b".into()]))
        );
        assert_eq!(find("Raw"), Some(&RegData::Binary(vec![1, 2, 3])));
    }

    #[test]
    fn sandboxed_registry_reports_missing_keys() {
        let area = TestRegistryKey::new();
        let reg = area.registry();
        let missing = RegKeyPath::new(Hive::CurrentUser, r"Software\Nope");
        assert!(!reg.key_exists(&missing));
        let err = reg.subkeys(&missing).unwrap_err();
        assert_eq!(err.code(), "NOT_FOUND");
        assert_eq!(err.location().as_deref(), Some(r"HKCU\Software\Nope"));
    }

    #[test]
    fn test_area_is_deleted_on_drop() {
        let prefix = {
            let area = TestRegistryKey::new();
            area.prefix.clone()
        };
        let still_there = RegKey::predef(HKEY_CURRENT_USER)
            .open_subkey(&prefix)
            .is_ok();
        assert!(!still_there);
    }
}
