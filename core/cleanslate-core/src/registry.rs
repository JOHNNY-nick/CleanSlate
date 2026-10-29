//! Registry access, behind a trait so tests never touch the real hives.
//!
//! [`WinRegistry::system`] reads the real registry. [`WinRegistry::sandboxed`]
//! redirects every hive below `HKCU\Software\CleanSlateTest\…`, the only key
//! tests may write to (safety rule 2). Writes are only used by the
//! quarantine, after `SafetyRules::validate_key`, outside simulation mode.

use std::fmt;

use serde::{Deserialize, Serialize};
use winreg::enums::{
    RegType, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_ALL_ACCESS, KEY_READ, KEY_SET_VALUE,
    KEY_WOW64_32KEY, KEY_WOW64_64KEY,
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
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
    /// `REG_BINARY`.
    Binary(Vec<u8>),
    /// Any other type (`REG_NONE`, resource lists…), kept as is.
    Raw {
        /// Numeric value type (`REG_*`).
        vtype: u32,
        /// Raw bytes.
        bytes: Vec<u8>,
    },
}

/// Numeric `REG_*` type of a winreg value type.
fn vtype_number(vtype: &RegType) -> u32 {
    vtype.clone() as u32
}

/// winreg value type of a numeric `REG_*` type (`REG_BINARY` if unknown).
fn vtype_from_number(number: u32) -> RegType {
    [
        RegType::REG_NONE,
        RegType::REG_SZ,
        RegType::REG_EXPAND_SZ,
        RegType::REG_BINARY,
        RegType::REG_DWORD,
        RegType::REG_DWORD_BIG_ENDIAN,
        RegType::REG_LINK,
        RegType::REG_MULTI_SZ,
        RegType::REG_RESOURCE_LIST,
        RegType::REG_FULL_RESOURCE_DESCRIPTOR,
        RegType::REG_RESOURCE_REQUIREMENTS_LIST,
        RegType::REG_QWORD,
    ]
    .into_iter()
    .find(|t| vtype_number(t) == number)
    .unwrap_or(RegType::REG_BINARY)
}

/// UTF-16 LE bytes of `text` followed by a NUL character.
fn utf16_z(text: &str) -> Vec<u8> {
    text.encode_utf16()
        .chain([0])
        .flat_map(u16::to_le_bytes)
        .collect()
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
            RegType::REG_BINARY => Some(RegData::Binary(value.bytes.to_vec())),
            _ => None,
        };
        decoded.unwrap_or_else(|| RegData::Raw {
            vtype: vtype_number(&value.vtype),
            bytes: value.bytes.to_vec(),
        })
    }

    /// Raw winreg value, for writing back exactly what was read.
    fn to_winreg(&self) -> RegValue<'static> {
        let (vtype, bytes) = match self {
            RegData::String(s) => (RegType::REG_SZ, utf16_z(s)),
            RegData::ExpandString(s) => (RegType::REG_EXPAND_SZ, utf16_z(s)),
            RegData::MultiString(items) => {
                let mut bytes: Vec<u8> = items.iter().flat_map(|s| utf16_z(s)).collect();
                bytes.extend([0, 0]);
                (RegType::REG_MULTI_SZ, bytes)
            }
            RegData::Dword(v) => (RegType::REG_DWORD, v.to_le_bytes().to_vec()),
            RegData::Qword(v) => (RegType::REG_QWORD, v.to_le_bytes().to_vec()),
            RegData::Binary(b) => (RegType::REG_BINARY, b.clone()),
            RegData::Raw { vtype, bytes } => (vtype_from_number(*vtype), bytes.clone()),
        };
        RegValue {
            bytes: bytes.into(),
            vtype,
        }
    }

    /// `regedit` text of the data (`"…"`, `dword:…`, `hex(N):…`).
    fn to_reg_text(&self) -> String {
        let hex = |kind: Option<u32>, bytes: &[u8]| {
            let list: Vec<String> = bytes.iter().map(|b| format!("{b:02x}")).collect();
            match kind {
                Some(k) => format!("hex({k:x}):{}", list.join(",")),
                None => format!("hex:{}", list.join(",")),
            }
        };
        match self {
            RegData::String(s) => format!("\"{}\"", s.replace('\\', r"\\").replace('"', "\\\"")),
            RegData::Dword(v) => format!("dword:{v:08x}"),
            RegData::Binary(b) => hex(None, b),
            RegData::Raw { vtype, bytes } => hex(Some(*vtype), bytes),
            other => {
                let raw = other.to_winreg();
                hex(Some(vtype_number(&raw.vtype)), &raw.bytes)
            }
        }
    }
}

/// A registry key with all its values and sub-keys (quarantine snapshot).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeySnapshot {
    /// Values of the key.
    pub values: Vec<(String, RegData)>,
    /// Sub-keys, by name.
    pub subkeys: Vec<(String, KeySnapshot)>,
}

/// Deepest key tree copied into a snapshot.
const MAX_SNAPSHOT_DEPTH: usize = 64;

impl KeySnapshot {
    /// Reads `key` and everything below it.
    pub fn read(registry: &dyn Registry, key: &RegKeyPath) -> CoreResult<Self> {
        Self::read_depth(registry, key, 0)
    }

    fn read_depth(registry: &dyn Registry, key: &RegKeyPath, depth: usize) -> CoreResult<Self> {
        if depth > MAX_SNAPSHOT_DEPTH {
            return Err(CoreError::InvalidInput(format!("key too deep: {key}")));
        }
        let mut subkeys = Vec::new();
        for name in registry.subkeys(key)? {
            let child = Self::read_depth(registry, &key.join(&name), depth + 1)?;
            subkeys.push((name, child));
        }
        Ok(KeySnapshot {
            values: registry.values(key)?,
            subkeys,
        })
    }

    /// Recreates the snapshot at `key` (which must not exist yet).
    pub fn write(&self, registry: &dyn Registry, key: &RegKeyPath) -> CoreResult<()> {
        registry.create_key(key)?;
        for (name, data) in &self.values {
            registry.set_value(key, name, data)?;
        }
        for (name, child) in &self.subkeys {
            child.write(registry, &key.join(name))?;
        }
        Ok(())
    }

    /// Total size of names and data, in bytes (approximation).
    pub fn size_bytes(&self) -> u64 {
        let own: u64 = self
            .values
            .iter()
            .map(|(n, v)| (n.len() * 2) as u64 + v.to_winreg().bytes.len() as u64)
            .sum();
        own + self
            .subkeys
            .iter()
            .map(|(_, s)| s.size_bytes())
            .sum::<u64>()
    }

    /// Text of a `.reg` file re-importable with `regedit` (specs § 7.2).
    pub fn to_reg_file(&self, key: &RegKeyPath) -> String {
        let mut out = String::from("Windows Registry Editor Version 5.00\r\n");
        self.append_reg(key, &mut out);
        out
    }

    fn append_reg(&self, key: &RegKeyPath, out: &mut String) {
        let hive = match key.hive {
            Hive::CurrentUser => "HKEY_CURRENT_USER",
            Hive::LocalMachine => "HKEY_LOCAL_MACHINE",
        };
        out.push_str(&format!("\r\n[{hive}\\{}]\r\n", key.path));
        for (name, data) in &self.values {
            let name = if name.is_empty() {
                "@".to_owned()
            } else {
                format!("\"{}\"", name.replace('\\', r"\\").replace('"', "\\\""))
            };
            out.push_str(&format!("{name}={}\r\n", data.to_reg_text()));
        }
        for (name, child) in &self.subkeys {
            child.append_reg(&key.join(name), out);
        }
    }
}

/// UTF-16 LE bytes with BOM, the encoding of `regedit` exports.
pub fn utf16_le_with_bom(text: &str) -> Vec<u8> {
    [0xFF, 0xFE]
        .into_iter()
        .chain(text.encode_utf16().flat_map(u16::to_le_bytes))
        .collect()
}

/// Access to the registry.
///
/// The write methods are only called by `quarantine`, after
/// `SafetyRules::validate_key` and outside simulation mode.
pub trait Registry: Send + Sync {
    /// Lists the names of the direct sub-keys of `key`.
    fn subkeys(&self, key: &RegKeyPath) -> CoreResult<Vec<String>>;

    /// Lists the values of `key` (the default value has an empty name).
    fn values(&self, key: &RegKeyPath) -> CoreResult<Vec<(String, RegData)>>;

    /// Returns `true` if `key` exists and can be read.
    fn key_exists(&self, key: &RegKeyPath) -> bool {
        self.subkeys(key).is_ok()
    }

    /// Creates `key` (and its missing parents).
    fn create_key(&self, key: &RegKeyPath) -> CoreResult<()>;

    /// Writes one value of `key`, with its exact type.
    fn set_value(&self, key: &RegKeyPath, name: &str, data: &RegData) -> CoreResult<()>;

    /// Deletes `key` and everything below it.
    fn delete_tree(&self, key: &RegKeyPath) -> CoreResult<()>;
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
        let (root, path, view_flag) = self.locate(key);
        root.open_subkey_with_flags(&path, KEY_READ | view_flag)
            .map_err(|e| CoreError::from_registry(key.to_string(), e))
    }

    /// Root, path and WOW64 flag of `key` (redirected in sandbox mode).
    fn locate(&self, key: &RegKeyPath) -> (RegKey, String, u32) {
        match &self.sandbox {
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
        }
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

    fn create_key(&self, key: &RegKeyPath) -> CoreResult<()> {
        if key.path.is_empty() {
            return Err(CoreError::Protected {
                location: key.to_string(),
            });
        }
        let (root, path, view_flag) = self.locate(key);
        root.create_subkey_with_flags(&path, KEY_ALL_ACCESS | view_flag)
            .map(|_| ())
            .map_err(|e| CoreError::from_registry(key.to_string(), e))
    }

    fn set_value(&self, key: &RegKeyPath, name: &str, data: &RegData) -> CoreResult<()> {
        let (root, path, view_flag) = self.locate(key);
        let opened = root
            .open_subkey_with_flags(&path, KEY_SET_VALUE | view_flag)
            .map_err(|e| CoreError::from_registry(key.to_string(), e))?;
        opened
            .set_raw_value(name, &data.to_winreg())
            .map_err(|e| CoreError::from_registry(key.to_string(), e))
    }

    fn delete_tree(&self, key: &RegKeyPath) -> CoreResult<()> {
        // Never a whole hive: deleting starts at a named sub-key.
        let Some((parent_path, name)) = key.path.rsplit_once('\\') else {
            return Err(CoreError::Protected {
                location: key.to_string(),
            });
        };
        let parent = RegKeyPath {
            path: parent_path.to_owned(),
            ..key.clone()
        };
        let (root, path, view_flag) = self.locate(&parent);
        let opened = root
            .open_subkey_with_flags(&path, KEY_ALL_ACCESS | view_flag)
            .map_err(|e| CoreError::from_registry(key.to_string(), e))?;
        opened
            .delete_subkey_all(name)
            .map_err(|e| CoreError::from_registry(key.to_string(), e))
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
    fn snapshot_delete_and_restore_identically() {
        let area = TestRegistryKey::new();
        let key = RegKeyPath::new(Hive::CurrentUser, r"Software\Audacity");
        let k = area.create(&key);
        k.set_value("", &"default").unwrap();
        k.set_value("Path", &r"C:\Program Files\Audacity").unwrap();
        k.set_value("Count", &7u32).unwrap();
        k.set_raw_value(
            "Nothing",
            &RegValue {
                bytes: vec![9u8, 8].into(),
                vtype: RegType::REG_NONE,
            },
        )
        .unwrap();
        area.create(&key.join(r"Prefs\Deep"))
            .set_value("List", &vec!["a", "b"])
            .unwrap();

        let reg = area.registry();
        let snapshot = KeySnapshot::read(&reg, &key).unwrap();
        assert!(snapshot.size_bytes() > 0);
        let json = serde_json::to_string(&snapshot).unwrap();

        reg.delete_tree(&key).unwrap();
        assert!(!reg.key_exists(&key));

        let back: KeySnapshot = serde_json::from_str(&json).unwrap();
        back.write(&reg, &key).unwrap();
        assert_eq!(KeySnapshot::read(&reg, &key).unwrap(), snapshot);
        assert!(snapshot.values.iter().any(|(n, v)| n == "Nothing"
            && *v
                == RegData::Raw {
                    vtype: 0,
                    bytes: vec![9, 8]
                }));
    }

    #[test]
    fn reg_file_format() {
        let snapshot = KeySnapshot {
            values: vec![
                (String::new(), RegData::String(r#"a"b\c"#.into())),
                ("N".into(), RegData::Dword(255)),
                ("B".into(), RegData::Binary(vec![1, 0xab])),
                ("E".into(), RegData::ExpandString("x".into())),
            ],
            subkeys: vec![(
                "Sub".into(),
                KeySnapshot {
                    values: vec![],
                    subkeys: vec![],
                },
            )],
        };
        let text = snapshot.to_reg_file(&RegKeyPath::new(Hive::CurrentUser, r"Software\X"));
        assert!(text.starts_with("Windows Registry Editor Version 5.00\r\n"));
        assert!(text.contains("[HKEY_CURRENT_USER\\Software\\X]\r\n"));
        assert!(text.contains(r#"@="a\"b\\c""#));
        assert!(text.contains("\"N\"=dword:000000ff"));
        assert!(text.contains("\"B\"=hex:01,ab"));
        assert!(text.contains("\"E\"=hex(2):78,00,00,00"));
        assert!(text.contains("[HKEY_CURRENT_USER\\Software\\X\\Sub]"));
        let bytes = utf16_le_with_bom("A");
        assert_eq!(bytes, [0xFF, 0xFE, 0x41, 0x00]);
    }

    #[test]
    fn hives_and_roots_are_never_deleted() {
        let area = TestRegistryKey::new();
        let reg = area.registry();
        assert_eq!(
            reg.delete_tree(&RegKeyPath::new(Hive::CurrentUser, "Software"))
                .unwrap_err()
                .code(),
            "PROTECTED"
        );
        assert!(reg
            .create_key(&RegKeyPath::new(Hive::CurrentUser, ""))
            .is_err());
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
