//! Windows shortcuts (`.lnk`, MS-SHLLINK): what a shortcut points to and
//! whether it is dead (EF-07, D20).
//!
//! A shortcut is dead **only** if its target is a local path that no longer
//! exists, on a fixed drive that is present. Everything else is "not
//! judged" and never proposed: MSI advertised shortcuts (Darwin descriptor),
//! packaged applications (AppUserModelID `Family!App`), network targets,
//! removable drives, missing drive letters, unresolved variables.

use std::path::Path;

use crate::fs::FileSystem;
use crate::safety::KnownFolders;
use crate::system::expand_known;

const HAS_LINK_TARGET_ID_LIST: u32 = 0x1;
const HAS_LINK_INFO: u32 = 0x2;
const HAS_NAME: u32 = 0x4;
const HAS_RELATIVE_PATH: u32 = 0x8;
const HAS_WORKING_DIR: u32 = 0x10;
const HAS_ARGUMENTS: u32 = 0x20;
const HAS_ICON_LOCATION: u32 = 0x40;
const IS_UNICODE: u32 = 0x80;
const HAS_DARWIN_ID: u32 = 0x1000;
const ENVIRONMENT_BLOCK: u32 = 0xA000_0001;
const DARWIN_BLOCK: u32 = 0xA000_0006;
const PROPERTY_STORE_BLOCK: u32 = 0xA000_0009;
/// `System.AppUserModel.ID`: format {9F4C2855-9F79-4B39-A8D0-E1D42DE1D5F3}, id 5.
const AUMID_FORMAT: [u8; 16] = [
    0x55, 0x28, 0x4C, 0x9F, 0x79, 0x9F, 0x39, 0x4B, 0xA8, 0xD0, 0xE1, 0xD4, 0x2D, 0xE1, 0xD5, 0xF3,
];
const AUMID_ID: u32 = 5;
const VT_LPWSTR: u16 = 0x1F;
/// `DRIVE_REMOVABLE`, `DRIVE_REMOTE`, `DRIVE_CDROM`.
const DRIVE_REMOVABLE: u32 = 2;
const DRIVE_REMOTE: u32 = 4;
const DRIVE_CDROM: u32 = 5;

/// What a shortcut file says about its target.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ShortcutInfo {
    /// Local path of `LinkInfo` (`LocalBasePath` + `CommonPathSuffix`).
    pub local_target: Option<String>,
    /// Target of the environment variables block, unexpanded (`%ProgramFiles%\…`).
    pub env_target: Option<String>,
    /// `DriveType` of the `VolumeID`, when present.
    pub drive_type: Option<u32>,
    /// The target is on the network (`CommonNetworkRelativeLink`).
    pub network: bool,
    /// MSI advertised shortcut (`HasDarwinID` or Darwin block).
    pub msi_advertised: bool,
    /// `System.AppUserModel.ID` of the shortcut, if any.
    pub app_user_model_id: Option<String>,
}

/// Verdict on one shortcut.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// The target is a local path that no longer exists.
    Dead(String),
    /// The target exists.
    Alive,
    /// Cannot be judged: never proposed. The text says why.
    NotJudged(&'static str),
}

/// Little-endian reader over the bytes of a shortcut.
struct Reader<'a>(&'a [u8]);

impl Reader<'_> {
    fn u16(&self, offset: usize) -> Option<u16> {
        let b = self.0.get(offset..offset + 2)?;
        Some(u16::from_le_bytes([b[0], b[1]]))
    }

    fn u32(&self, offset: usize) -> Option<u32> {
        let b = self.0.get(offset..offset + 4)?;
        Some(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    fn usize(&self, offset: usize) -> Option<usize> {
        usize::try_from(self.u32(offset)?).ok()
    }

    /// NUL-terminated ANSI string (code page bytes read as Latin-1).
    fn c_string(&self, start: usize) -> Option<String> {
        let rest = self.0.get(start..)?;
        let end = rest.iter().position(|&b| b == 0)?;
        Some(rest[..end].iter().map(|&b| char::from(b)).collect())
    }

    /// NUL-terminated UTF-16 string, at most `max_units` units.
    fn wide_string(&self, start: usize, max_units: usize) -> Option<String> {
        let units: Vec<u16> = (0..max_units)
            .map_while(|i| self.u16(start + 2 * i))
            .take_while(|&u| u != 0)
            .collect();
        Some(String::from_utf16_lossy(&units))
    }
}

/// Reads a shortcut file; `None` if it is not one.
pub fn parse(bytes: &[u8]) -> Option<ShortcutInfo> {
    let r = Reader(bytes);
    if r.u32(0)? != 0x4C {
        return None;
    }
    let flags = r.u32(0x14)?;
    let mut info = ShortcutInfo {
        msi_advertised: flags & HAS_DARWIN_ID != 0,
        ..ShortcutInfo::default()
    };
    let mut offset = 0x4C;
    if flags & HAS_LINK_TARGET_ID_LIST != 0 {
        offset += 2 + usize::from(r.u16(offset)?);
    }
    if flags & HAS_LINK_INFO != 0 {
        let start = offset;
        let size = r.usize(start)?;
        let link_flags = r.u32(start + 8)?;
        if link_flags & 0x2 != 0 {
            info.network = true;
        }
        if link_flags & 0x1 != 0 {
            let volume = start + r.usize(start + 0x0C)?;
            // A VolumeID is at least 16 bytes; a smaller one says nothing.
            if r.u32(volume).is_some_and(|s| s >= 0x10) {
                info.drive_type = r.u32(volume + 4);
            }
            let base = r.c_string(start + r.usize(start + 0x10)?)?;
            let suffix = r
                .c_string(start + r.usize(start + 0x18)?)
                .unwrap_or_default();
            let target = if suffix.is_empty() {
                base
            } else {
                format!(r"{}\{suffix}", base.trim_end_matches('\\'))
            };
            info.local_target = (!target.is_empty()).then_some(target);
        }
        offset = start + size;
    }
    // StringData: name, relative path, working dir, arguments, icon.
    let unit = if flags & IS_UNICODE != 0 { 2 } else { 1 };
    for flag in [
        HAS_NAME,
        HAS_RELATIVE_PATH,
        HAS_WORKING_DIR,
        HAS_ARGUMENTS,
        HAS_ICON_LOCATION,
    ] {
        if flags & flag != 0 {
            offset += 2 + unit * usize::from(r.u16(offset)?);
        }
    }
    // ExtraData blocks, until the terminal block (size < 4).
    while let (Some(size), Some(signature)) = (r.usize(offset), r.u32(offset + 4)) {
        if size < 8 {
            break;
        }
        match signature {
            ENVIRONMENT_BLOCK => {
                // TargetAnsi (260 bytes) then TargetUnicode (520 bytes).
                let wide = r.wide_string(offset + 8 + 260, 260);
                let ansi = r.c_string(offset + 8);
                info.env_target = wide
                    .filter(|t| !t.is_empty())
                    .or(ansi)
                    .filter(|t| !t.is_empty());
            }
            DARWIN_BLOCK => info.msi_advertised = true,
            PROPERTY_STORE_BLOCK => {
                info.app_user_model_id = app_user_model_id(&r, offset + 8, offset + size);
            }
            _ => {}
        }
        offset += size;
    }
    Some(info)
}

/// `System.AppUserModel.ID` in a serialized property store between `start` and `end`.
fn app_user_model_id(r: &Reader, start: usize, end: usize) -> Option<String> {
    let mut storage = start;
    while storage + 24 <= end {
        let storage_size = r.usize(storage)?;
        if storage_size == 0 {
            break;
        }
        let format = r.0.get(storage + 8..storage + 24)?;
        let mut value = storage + 24;
        while value + 9 <= storage + storage_size {
            let value_size = r.usize(value)?;
            if value_size == 0 {
                break;
            }
            let id = r.u32(value + 4)?;
            let kind = r.u16(value + 9)?;
            if format == AUMID_FORMAT && id == AUMID_ID && kind == VT_LPWSTR {
                let chars = r.usize(value + 13)?;
                return r.wide_string(value + 17, chars);
            }
            value += value_size;
        }
        storage += storage_size;
    }
    None
}

/// Decides whether a shortcut is dead. `folders` expands the environment
/// variables of the target; `fs` checks the drive and the target.
pub fn verdict(info: &ShortcutInfo, fs: &dyn FileSystem, folders: &KnownFolders) -> Verdict {
    if info.msi_advertised {
        return Verdict::NotJudged("raccourci MSI annoncé");
    }
    // A packaged application is identified by `PackageFamilyName!AppId`.
    if info
        .app_user_model_id
        .as_deref()
        .is_some_and(|id| id.contains('!'))
    {
        return Verdict::NotJudged("application du Microsoft Store");
    }
    let target = match (&info.env_target, &info.local_target) {
        (Some(raw), _) => expand_known(raw, folders),
        (None, Some(local)) => local.clone(),
        (None, None) if info.network => return Verdict::NotJudged("cible sur le réseau"),
        (None, None) => return Verdict::NotJudged("pas de cible locale"),
    };
    if target.contains('%') {
        return Verdict::NotJudged("variable d'environnement non résolue");
    }
    if target.starts_with(r"\\") || info.network {
        return Verdict::NotJudged("cible sur le réseau");
    }
    match info.drive_type {
        Some(DRIVE_REMOTE) => return Verdict::NotJudged("lecteur réseau"),
        Some(DRIVE_REMOVABLE | DRIVE_CDROM) => return Verdict::NotJudged("lecteur amovible"),
        _ => {}
    }
    let bytes = target.as_bytes();
    let has_drive =
        bytes.len() >= 3 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' && bytes[2] == b'\\';
    if !has_drive {
        return Verdict::NotJudged("pas de cible locale");
    }
    if !fs.exists(Path::new(&target[..3])) {
        return Verdict::NotJudged("lecteur absent");
    }
    if fs.exists(Path::new(&target)) {
        Verdict::Alive
    } else {
        Verdict::Dead(target)
    }
}

/// Target of a shortcut that works (D78): `None` when it is dead or cannot be judged.
pub fn live_target(
    info: &ShortcutInfo,
    fs: &dyn FileSystem,
    folders: &KnownFolders,
) -> Option<String> {
    if verdict(info, fs, folders) != Verdict::Alive {
        return None;
    }
    match (&info.env_target, &info.local_target) {
        (Some(raw), _) => Some(expand_known(raw, folders)),
        (None, Some(local)) => Some(local.clone()),
        (None, None) => None,
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::fs::FakeFileSystem;

    /// Builds a shortcut file (MS-SHLLINK) for the tests.
    #[derive(Default)]
    pub(crate) struct Lnk {
        pub local: Option<String>,
        pub drive_type: u32,
        pub network: bool,
        pub darwin: bool,
        pub env: Option<String>,
        pub aumid: Option<String>,
        pub arguments: Option<String>,
    }

    impl Lnk {
        pub(crate) fn local(target: &str) -> Self {
            Lnk {
                local: Some(target.into()),
                drive_type: 3, // DRIVE_FIXED
                ..Lnk::default()
            }
        }

        pub(crate) fn build(&self) -> Vec<u8> {
            let mut flags = IS_UNICODE;
            if self.local.is_some() || self.network {
                flags |= HAS_LINK_INFO;
            }
            if self.darwin {
                flags |= HAS_DARWIN_ID;
            }
            if self.arguments.is_some() {
                flags |= HAS_ARGUMENTS;
            }
            let mut bytes = vec![0u8; 0x4C];
            bytes[0..4].copy_from_slice(&0x4Cu32.to_le_bytes());
            bytes[0x14..0x18].copy_from_slice(&flags.to_le_bytes());
            if flags & HAS_LINK_INFO != 0 {
                bytes.extend(self.link_info());
            }
            if let Some(args) = &self.arguments {
                let units: Vec<u16> = args.encode_utf16().collect();
                bytes.extend((units.len() as u16).to_le_bytes());
                bytes.extend(units.iter().flat_map(|u| u.to_le_bytes()));
            }
            if let Some(env) = &self.env {
                let mut block = vec![0u8; 0x314];
                block[0..4].copy_from_slice(&0x314u32.to_le_bytes());
                block[4..8].copy_from_slice(&ENVIRONMENT_BLOCK.to_le_bytes());
                for (i, b) in env.bytes().enumerate() {
                    block[8 + i] = b;
                }
                for (i, u) in env.encode_utf16().enumerate() {
                    block[8 + 260 + 2 * i..8 + 262 + 2 * i].copy_from_slice(&u.to_le_bytes());
                }
                bytes.extend(block);
            }
            if self.darwin {
                let mut block = vec![0u8; 0x314];
                block[0..4].copy_from_slice(&0x314u32.to_le_bytes());
                block[4..8].copy_from_slice(&DARWIN_BLOCK.to_le_bytes());
                bytes.extend(block);
            }
            if let Some(aumid) = &self.aumid {
                bytes.extend(property_store(aumid));
            }
            bytes.extend([0u8; 4]); // terminal block
            bytes
        }

        fn link_info(&self) -> Vec<u8> {
            let header = 0x1Cu32;
            let volume = header;
            let volume_size = 0x10u32;
            let base_offset = volume + volume_size + 1; // empty volume label after the VolumeID
            let base = self.local.clone().unwrap_or_default();
            let suffix_offset = base_offset + base.len() as u32 + 1;
            let size = suffix_offset + 1;
            let mut link_flags = 0;
            if self.local.is_some() {
                link_flags |= 0x1;
            }
            if self.network {
                link_flags |= 0x2;
            }
            let mut info = Vec::new();
            for v in [
                size,
                header,
                link_flags,
                volume,
                base_offset,
                0,
                suffix_offset,
            ] {
                info.extend(v.to_le_bytes());
            }
            for v in [volume_size, self.drive_type, 0x1234, volume_size] {
                info.extend(v.to_le_bytes());
            }
            info.push(0); // volume label
            info.extend(base.bytes());
            info.push(0);
            info.push(0); // empty suffix
            info
        }
    }

    /// PropertyStore block holding `System.AppUserModel.ID`.
    fn property_store(aumid: &str) -> Vec<u8> {
        let text: Vec<u8> = aumid
            .encode_utf16()
            .chain([0])
            .flat_map(|u| u.to_le_bytes())
            .collect();
        let chars = (aumid.encode_utf16().count() + 1) as u32;
        let mut value = Vec::new();
        let value_size = 4 + 4 + 1 + 2 + 2 + 4 + text.len() as u32;
        value.extend(value_size.to_le_bytes());
        value.extend(AUMID_ID.to_le_bytes());
        value.push(0);
        value.extend(VT_LPWSTR.to_le_bytes());
        value.extend(0u16.to_le_bytes());
        value.extend(chars.to_le_bytes());
        value.extend(text);
        let storage_size = 4 + 4 + 16 + value.len() as u32 + 4;
        let mut storage = Vec::new();
        storage.extend(storage_size.to_le_bytes());
        storage.extend(0x5350_5331u32.to_le_bytes());
        storage.extend(AUMID_FORMAT);
        storage.extend(value);
        storage.extend(0u32.to_le_bytes()); // end of values
        let block_size = 8 + storage.len() as u32 + 4;
        let mut block = Vec::new();
        block.extend(block_size.to_le_bytes());
        block.extend(PROPERTY_STORE_BLOCK.to_le_bytes());
        block.extend(storage);
        block.extend(0u32.to_le_bytes()); // end of storages
        block
    }

    fn judge(lnk: &Lnk, fs: &FakeFileSystem) -> Verdict {
        let info = parse(&lnk.build()).expect("shortcut");
        verdict(&info, fs, &KnownFolders::under(r"C:\"))
    }

    fn machine() -> FakeFileSystem {
        let mut fs = FakeFileSystem::new();
        fs.add_file(r"C:\Program Files\Alive\alive.exe", 10, None)
            .add_dir(r"D:\Data");
        fs
    }

    #[test]
    fn local_target_present_or_absent() {
        let fs = machine();
        assert_eq!(
            judge(&Lnk::local(r"C:\Program Files\Alive\alive.exe"), &fs),
            Verdict::Alive
        );
        assert_eq!(
            judge(&Lnk::local(r"C:\Program Files\Gone\gone.exe"), &fs),
            Verdict::Dead(r"C:\Program Files\Gone\gone.exe".into())
        );
        // Another fixed drive that is present.
        assert_eq!(
            judge(&Lnk::local(r"D:\Data\Gone\x.exe"), &fs),
            Verdict::Dead(r"D:\Data\Gone\x.exe".into())
        );
    }

    #[test]
    fn arguments_and_string_data_are_skipped() {
        let lnk = Lnk {
            arguments: Some("--profile Old".into()),
            ..Lnk::local(r"C:\Program Files\Gone\gone.exe")
        };
        assert!(matches!(judge(&lnk, &machine()), Verdict::Dead(_)));
    }

    #[test]
    fn msi_advertised_is_not_judged() {
        let lnk = Lnk {
            darwin: true,
            ..Lnk::local(r"C:\Program Files\Gone\gone.exe")
        };
        assert_eq!(
            judge(&lnk, &machine()),
            Verdict::NotJudged("raccourci MSI annoncé")
        );
        // Darwin descriptor without any target path.
        let bare = Lnk {
            darwin: true,
            ..Lnk::default()
        };
        assert_eq!(
            judge(&bare, &machine()),
            Verdict::NotJudged("raccourci MSI annoncé")
        );
    }

    #[test]
    fn store_application_is_not_judged() {
        let packaged = Lnk {
            aumid: Some("Vendor.App_8wekyb3d8bbwe!App".into()),
            ..Lnk::default()
        };
        let info = parse(&packaged.build()).unwrap();
        assert_eq!(
            info.app_user_model_id.as_deref(),
            Some("Vendor.App_8wekyb3d8bbwe!App")
        );
        assert_eq!(
            judge(&packaged, &machine()),
            Verdict::NotJudged("application du Microsoft Store")
        );
        // A classic AppUserModelID (no « ! ») does not hide a dead target.
        let classic = Lnk {
            aumid: Some("Vendor.OldApp".into()),
            ..Lnk::local(r"C:\Program Files\Gone\gone.exe")
        };
        assert!(matches!(judge(&classic, &machine()), Verdict::Dead(_)));
    }

    #[test]
    fn network_targets_are_not_judged() {
        let unc = Lnk {
            network: true,
            ..Lnk::default()
        };
        assert_eq!(
            judge(&unc, &machine()),
            Verdict::NotJudged("cible sur le réseau")
        );
        let unc_path = Lnk::local(r"\\server\share\app.exe");
        assert_eq!(
            judge(&unc_path, &machine()),
            Verdict::NotJudged("cible sur le réseau")
        );
        let mapped = Lnk {
            drive_type: DRIVE_REMOTE,
            ..Lnk::local(r"C:\Gone\app.exe")
        };
        assert_eq!(
            judge(&mapped, &machine()),
            Verdict::NotJudged("lecteur réseau")
        );
    }

    #[test]
    fn removable_and_missing_drives_are_not_judged() {
        for kind in [DRIVE_REMOVABLE, DRIVE_CDROM] {
            let lnk = Lnk {
                drive_type: kind,
                ..Lnk::local(r"D:\Gone\app.exe")
            };
            assert_eq!(
                judge(&lnk, &machine()),
                Verdict::NotJudged("lecteur amovible")
            );
        }
        assert_eq!(
            judge(&Lnk::local(r"X:\Gone\app.exe"), &machine()),
            Verdict::NotJudged("lecteur absent")
        );
    }

    #[test]
    fn environment_variables() {
        // Resolved: the expanded target is judged.
        let resolved = Lnk {
            env: Some(r"%ProgramFiles%\Gone\gone.exe".into()),
            ..Lnk::default()
        };
        assert_eq!(
            judge(&resolved, &machine()),
            Verdict::Dead(r"C:\Program Files\Gone\gone.exe".into())
        );
        let alive = Lnk {
            env: Some(r"%ProgramFiles%\Alive\alive.exe".into()),
            ..Lnk::local(r"C:\Old\elsewhere.exe")
        };
        assert_eq!(judge(&alive, &machine()), Verdict::Alive);
        // Unresolved: never judged.
        let unresolved = Lnk {
            env: Some(r"%TOOLS_HOME%\gone.exe".into()),
            ..Lnk::local(r"C:\Program Files\Gone\gone.exe")
        };
        assert_eq!(
            judge(&unresolved, &machine()),
            Verdict::NotJudged("variable d'environnement non résolue")
        );
        let in_local = Lnk::local(r"%OLD%\gone.exe");
        assert_eq!(
            judge(&in_local, &machine()),
            Verdict::NotJudged("variable d'environnement non résolue")
        );
    }

    #[test]
    fn no_target_or_not_a_shortcut() {
        assert_eq!(
            judge(&Lnk::default(), &machine()),
            Verdict::NotJudged("pas de cible locale")
        );
        assert_eq!(
            judge(&Lnk::local(r"Gone\relative.exe"), &machine()),
            Verdict::NotJudged("pas de cible locale")
        );
        assert!(parse(b"not a shortcut").is_none());
        assert!(parse(&[]).is_none());
        // Truncated files never panic.
        let full = Lnk::local(r"C:\Gone\x.exe").build();
        for len in 0..full.len() {
            let _ = parse(&full[..len]);
        }
    }
}
