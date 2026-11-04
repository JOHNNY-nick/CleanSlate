//! Thin wrappers over the Win32 calls of the quarantine: SHA-256 (CNG
//! BCrypt), original security descriptor (SDDL) and `MoveFileExW`.

use std::fs::File;
use std::io::Read;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;

use windows::core::{PCWSTR, PWSTR};
use windows::Win32::Foundation::{LocalFree, HLOCAL};
use windows::Win32::Security::Authorization::{
    ConvertSecurityDescriptorToStringSecurityDescriptorW, GetNamedSecurityInfoW, SDDL_REVISION_1,
    SE_FILE_OBJECT,
};
use windows::Win32::Security::Cryptography::{
    BCryptCreateHash, BCryptDestroyHash, BCryptFinishHash, BCryptHashData, BCRYPT_HASH_HANDLE,
    BCRYPT_SHA256_ALG_HANDLE,
};
use windows::Win32::Security::{
    DACL_SECURITY_INFORMATION, GROUP_SECURITY_INFORMATION, OWNER_SECURITY_INFORMATION,
    PSECURITY_DESCRIPTOR,
};
use windows::Win32::Storage::FileSystem::{
    MoveFileExW, MOVEFILE_DELAY_UNTIL_REBOOT, MOVEFILE_WRITE_THROUGH,
};

use crate::error::{CoreError, CoreResult};

/// `ERROR_NOT_SAME_DEVICE`: the move needs a copy.
pub const ERROR_NOT_SAME_DEVICE: u32 = 17;
/// `ERROR_SHARING_VIOLATION`: the file is used by a program.
pub const ERROR_SHARING_VIOLATION: u32 = 32;
/// `ERROR_LOCK_VIOLATION`.
pub const ERROR_LOCK_VIOLATION: u32 = 33;

/// Size of the buffer used to hash files.
const CHUNK: usize = 1024 * 1024;

/// NUL-terminated UTF-16 path for Win32 calls.
fn wide(path: &Path) -> Vec<u16> {
    path.as_os_str().encode_wide().chain([0]).collect()
}

/// Incremental SHA-256 through Windows CNG (no extra crate, D11).
pub struct Sha256 {
    handle: BCRYPT_HASH_HANDLE,
}

impl Sha256 {
    /// Starts a hash.
    pub fn new() -> CoreResult<Self> {
        let mut handle = BCRYPT_HASH_HANDLE::default();
        // SAFETY: SHA-256 pseudo-handle, out-pointer to a local handle; the
        // hash object is allocated by CNG (no buffer passed).
        let status =
            unsafe { BCryptCreateHash(BCRYPT_SHA256_ALG_HANDLE, &mut handle, None, None, 0) };
        status
            .ok()
            .map_err(|e| CoreError::InvalidInput(format!("BCryptCreateHash: {e}")))?;
        Ok(Sha256 { handle })
    }

    /// Adds bytes.
    pub fn update(&mut self, data: &[u8]) -> CoreResult<()> {
        // SAFETY: the handle is live until Drop; `data` is a valid slice.
        let status = unsafe { BCryptHashData(self.handle, data, 0) };
        status
            .ok()
            .map_err(|e| CoreError::InvalidInput(format!("BCryptHashData: {e}")))
    }

    /// Finishes and returns the digest in lower-case hexadecimal.
    pub fn finish(self) -> CoreResult<String> {
        let mut digest = [0u8; 32];
        // SAFETY: 32-byte output buffer for SHA-256; handle still live.
        let status = unsafe { BCryptFinishHash(self.handle, &mut digest, 0) };
        status
            .ok()
            .map_err(|e| CoreError::InvalidInput(format!("BCryptFinishHash: {e}")))?;
        Ok(digest.iter().map(|b| format!("{b:02x}")).collect())
    }

    /// SHA-256 of bytes in memory.
    pub fn of_bytes(data: &[u8]) -> CoreResult<String> {
        let mut hash = Sha256::new()?;
        hash.update(data)?;
        hash.finish()
    }

    /// SHA-256 of a file, read by chunks (works for large files).
    pub fn of_file(path: &Path) -> CoreResult<String> {
        let mut file = File::open(path).map_err(|e| CoreError::from_io(path, e))?;
        let mut hash = Sha256::new()?;
        let mut buffer = vec![0u8; CHUNK];
        loop {
            let read = file
                .read(&mut buffer)
                .map_err(|e| CoreError::from_io(path, e))?;
            if read == 0 {
                break;
            }
            hash.update(&buffer[..read])?;
        }
        hash.finish()
    }
}

impl Drop for Sha256 {
    fn drop(&mut self) {
        // SAFETY: the handle comes from BCryptCreateHash and is destroyed once.
        let _ = unsafe { BCryptDestroyHash(self.handle) };
    }
}

/// Owner, group and DACL of a file or folder, as SDDL (best effort).
pub fn sddl_of(path: &Path) -> Option<String> {
    let name = wide(path);
    let info = OWNER_SECURITY_INFORMATION | GROUP_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION;
    let mut descriptor = PSECURITY_DESCRIPTOR::default();
    // SAFETY: NUL-terminated path; CNG-style out pointer; the descriptor is
    // allocated by the system and released with LocalFree below.
    unsafe {
        let error = GetNamedSecurityInfoW(
            PCWSTR(name.as_ptr()),
            SE_FILE_OBJECT,
            info,
            None,
            None,
            None,
            None,
            &mut descriptor,
        );
        if error.0 != 0 {
            return None;
        }
        let mut text = PWSTR::null();
        let converted = ConvertSecurityDescriptorToStringSecurityDescriptorW(
            descriptor,
            SDDL_REVISION_1,
            info,
            &mut text,
            None,
        );
        let result = converted.ok().and_then(|()| text.to_string().ok());
        if !text.is_null() {
            let _ = LocalFree(Some(HLOCAL(text.0.cast())));
        }
        let _ = LocalFree(Some(HLOCAL(descriptor.0)));
        result
    }
}

/// Moves a file or folder on the same volume. Returns the Win32 error code
/// on failure (e.g. [`ERROR_NOT_SAME_DEVICE`]).
pub fn move_path(from: &Path, to: &Path) -> Result<(), u32> {
    move_with(from, to, MOVEFILE_WRITE_THROUGH)
}

/// Schedules a file move at the next reboot (locked file, admin only).
pub fn move_at_reboot(from: &Path, to: &Path) -> Result<(), u32> {
    move_with(from, to, MOVEFILE_DELAY_UNTIL_REBOOT)
}

/// Deletes a file or an empty folder at the next restart (`MoveFileExW` with no target,
/// documented by Windows for files in use; D79).
pub fn delete_at_reboot(path: &Path) -> CoreResult<()> {
    let source = wide(path);
    // SAFETY: NUL-terminated path alive for the call; a null target means « delete ».
    unsafe {
        MoveFileExW(
            PCWSTR(source.as_ptr()),
            PCWSTR::null(),
            MOVEFILE_DELAY_UNTIL_REBOOT,
        )
    }
    .map_err(|e| {
        CoreError::InvalidInput(format!("not scheduled at restart: {}: {e}", path.display()))
    })
}

fn move_with(
    from: &Path,
    to: &Path,
    flags: windows::Win32::Storage::FileSystem::MOVE_FILE_FLAGS,
) -> Result<(), u32> {
    let (source, target) = (wide(from), wide(to));
    // SAFETY: both paths are NUL-terminated UTF-16 buffers alive for the call.
    let result = unsafe { MoveFileExW(PCWSTR(source.as_ptr()), PCWSTR(target.as_ptr()), flags) };
    result.map_err(|e| (e.code().0 as u32) & 0xFFFF)
}

/// A physical disk read by `DeviceIoControl` (D82): no PowerShell, no console window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PhysicalDisk {
    /// Number of `\\.\PhysicalDriveN`.
    pub number: u32,
    /// Vendor and product, « Samsung SSD 980 ».
    pub name: String,
    /// Size of the disk.
    pub size: u64,
    /// Sum of its partitions.
    pub allocated: u64,
}

/// Physical disks 0 to 31 that can be opened (read-only; administrator rights needed).
pub fn physical_disks() -> Vec<PhysicalDisk> {
    (0..32).filter_map(physical_disk).collect()
}

/// ASCII text at `offset` in `bytes`, up to its NUL (fields of `STORAGE_DEVICE_DESCRIPTOR`).
pub fn ascii_at(bytes: &[u8], offset: usize) -> String {
    if offset == 0 || offset >= bytes.len() {
        return String::new();
    }
    let text = &bytes[offset..];
    let end = text.iter().position(|b| *b == 0).unwrap_or(text.len());
    String::from_utf8_lossy(&text[..end]).trim().to_owned()
}

// Closes a handle of CreateFileW when dropped.
struct OwnedHandle(windows::Win32::Foundation::HANDLE);

impl Drop for OwnedHandle {
    fn drop(&mut self) {
        // SAFETY: the handle comes from CreateFileW and is closed once, here.
        let _ = unsafe { windows::Win32::Foundation::CloseHandle(self.0) };
    }
}

// One IOCTL into an 8-byte aligned buffer; the bytes written, or None.
fn ioctl(device: &OwnedHandle, code: u32, input: Option<&[u8]>, size: usize) -> Option<Vec<u8>> {
    use windows::Win32::System::IO::DeviceIoControl;
    let mut buffer = vec![0u64; size.div_ceil(8)];
    let mut returned = 0u32;
    let (input_ptr, input_len) = input.map_or((None, 0), |b| (Some(b.as_ptr().cast()), b.len()));
    // SAFETY: the output buffer is owned, writable and as large as said; the input, when
    // given, lives for the call; the handle is open; no overlapped I/O.
    unsafe {
        DeviceIoControl(
            device.0,
            code,
            input_ptr,
            u32::try_from(input_len).ok()?,
            Some(buffer.as_mut_ptr().cast()),
            u32::try_from(buffer.len() * 8).ok()?,
            Some(&mut returned),
            None,
        )
    }
    .ok()?;
    let mut bytes: Vec<u8> = buffer.iter().flat_map(|w| w.to_le_bytes()).collect();
    bytes.truncate(returned as usize);
    Some(bytes)
}

fn physical_disk(number: u32) -> Option<PhysicalDisk> {
    use windows::Win32::Foundation::GENERIC_READ;
    use windows::Win32::Storage::FileSystem::{
        CreateFileW, FILE_FLAGS_AND_ATTRIBUTES, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
    };
    use windows::Win32::System::Ioctl::{
        PropertyStandardQuery, StorageDeviceProperty, DRIVE_LAYOUT_INFORMATION_EX,
        IOCTL_DISK_GET_DRIVE_LAYOUT_EX, IOCTL_DISK_GET_LENGTH_INFO, IOCTL_STORAGE_QUERY_PROPERTY,
        PARTITION_INFORMATION_EX, STORAGE_DEVICE_DESCRIPTOR, STORAGE_PROPERTY_QUERY,
    };
    let name: Vec<u16> = format!(r"\\.\PhysicalDrive{number}")
        .encode_utf16()
        .chain([0])
        .collect();
    // SAFETY: NUL-terminated name that lives for the call; read access only.
    let device = OwnedHandle(
        unsafe {
            CreateFileW(
                PCWSTR(name.as_ptr()),
                GENERIC_READ.0,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                None,
                OPEN_EXISTING,
                FILE_FLAGS_AND_ATTRIBUTES(0),
                None,
            )
        }
        .ok()?,
    );

    let length = ioctl(&device, IOCTL_DISK_GET_LENGTH_INFO, None, 8)?;
    let size = u64::try_from(i64::from_le_bytes(length.get(..8)?.try_into().ok()?)).ok()?;

    // Partitions: header, then PartitionCount entries (an empty MBR entry has a length of 0).
    let layout = ioctl(&device, IOCTL_DISK_GET_DRIVE_LAYOUT_EX, None, 64 * 1024)?;
    let header = std::mem::offset_of!(DRIVE_LAYOUT_INFORMATION_EX, PartitionEntry);
    let entry = std::mem::size_of::<PARTITION_INFORMATION_EX>();
    let at = std::mem::offset_of!(PARTITION_INFORMATION_EX, PartitionLength);
    let count = u32::from_le_bytes(layout.get(4..8)?.try_into().ok()?) as usize;
    let allocated = (0..count)
        .filter_map(|i| layout.get(header + i * entry + at..header + i * entry + at + 8))
        .filter_map(|b| b.try_into().ok().map(i64::from_le_bytes))
        .map(|len| u64::try_from(len).unwrap_or(0))
        .sum();

    // Vendor and product, from the storage descriptor.
    let query = STORAGE_PROPERTY_QUERY {
        PropertyId: StorageDeviceProperty,
        QueryType: PropertyStandardQuery,
        AdditionalParameters: [0],
    };
    // SAFETY: a plain repr(C) struct read as bytes for the input of the IOCTL.
    let query_bytes = unsafe {
        std::slice::from_raw_parts(
            (&query as *const STORAGE_PROPERTY_QUERY).cast::<u8>(),
            std::mem::size_of::<STORAGE_PROPERTY_QUERY>(),
        )
    };
    let label = ioctl(
        &device,
        IOCTL_STORAGE_QUERY_PROPERTY,
        Some(query_bytes),
        1024,
    )
    .map(|d| {
        let field = |offset: usize| {
            d.get(offset..offset + 4)
                .and_then(|b| b.try_into().ok())
                .map_or(0, |b| u32::from_le_bytes(b) as usize)
        };
        let vendor = ascii_at(
            &d,
            field(std::mem::offset_of!(
                STORAGE_DEVICE_DESCRIPTOR,
                VendorIdOffset
            )),
        );
        let product = ascii_at(
            &d,
            field(std::mem::offset_of!(
                STORAGE_DEVICE_DESCRIPTOR,
                ProductIdOffset
            )),
        );
        format!("{vendor} {product}").trim().to_owned()
    })
    .unwrap_or_default();

    Some(PhysicalDisk {
        number,
        name: label,
        size,
        allocated,
    })
}

/// HMAC-SHA256 of `data` with `key` (CNG), the signature of the manifests of the quarantine (D88).
pub fn hmac_sha256(key: &[u8], data: &[u8]) -> CoreResult<[u8; 32]> {
    use windows::Win32::Security::Cryptography::{BCryptHash, BCRYPT_HMAC_SHA256_ALG_HANDLE};
    let mut out = [0u8; 32];
    // SAFETY: the pseudo-handle of HMAC-SHA256 needs no opening; the key, the input and the
    // 32-byte output buffer live for the call.
    let status = unsafe { BCryptHash(BCRYPT_HMAC_SHA256_ALG_HANDLE, Some(key), data, &mut out) };
    if status.is_err() {
        return Err(CoreError::InvalidInput(format!(
            "HMAC-SHA256 failed: {:#x}",
            status.0
        )));
    }
    Ok(out)
}

/// `count` bytes from the random generator of Windows (CNG).
pub fn random_bytes(count: usize) -> CoreResult<Vec<u8>> {
    use windows::Win32::Security::Cryptography::{
        BCryptGenRandom, BCRYPT_USE_SYSTEM_PREFERRED_RNG,
    };
    let mut out = vec![0u8; count];
    // SAFETY: the buffer is owned and as large as said; the system generator needs no handle.
    let status = unsafe { BCryptGenRandom(None, &mut out, BCRYPT_USE_SYSTEM_PREFERRED_RNG) };
    if status.is_err() {
        return Err(CoreError::InvalidInput(format!(
            "random generator failed: {:#x}",
            status.0
        )));
    }
    Ok(out)
}

/// Key of the registry that holds the key of the manifests (D88).
pub const INTEGRITY_KEY: &str = r"SOFTWARE\CleanSlate\Integrity";

/// The secret that signs the manifests of the quarantine (D88), and whether it was created now.
///
/// It lives in `HKLM\SOFTWARE\CleanSlate\Integrity`, whose rights are set here: SYSTEM and
/// Administrators only, nothing inherited.
pub fn manifest_key() -> CoreResult<(Vec<u8>, bool)> {
    use windows::Win32::Foundation::{LocalFree, HLOCAL};
    use windows::Win32::Security::Authorization::{
        ConvertStringSecurityDescriptorToSecurityDescriptorW, SetNamedSecurityInfoW,
        SE_REGISTRY_KEY,
    };
    use windows::Win32::Security::{
        GetSecurityDescriptorDacl, ACL, DACL_SECURITY_INFORMATION,
        PROTECTED_DACL_SECURITY_INFORMATION,
    };
    use winreg::enums::{HKEY_LOCAL_MACHINE, KEY_ALL_ACCESS};
    use winreg::{RegKey, RegValue};

    let denied = |e: std::io::Error| CoreError::from_registry(format!(r"HKLM\{INTEGRITY_KEY}"), e);
    let (key, _) = RegKey::predef(HKEY_LOCAL_MACHINE)
        .create_subkey_with_flags(INTEGRITY_KEY, KEY_ALL_ACCESS)
        .map_err(denied)?;

    // Rights set before the secret is written: SYSTEM and Administrators, full control.
    let sddl: Vec<u16> = "D:P(A;;KA;;;SY)(A;;KA;;;BA)\0".encode_utf16().collect();
    let name: Vec<u16> = format!(r"MACHINE\{INTEGRITY_KEY}\0")
        .encode_utf16()
        .collect();
    let mut descriptor = PSECURITY_DESCRIPTOR::default();
    // SAFETY: NUL-terminated SDDL text; the descriptor allocated by the call is freed below
    // with LocalFree, once, after its DACL was used.
    unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            PCWSTR(sddl.as_ptr()),
            SDDL_REVISION_1,
            &mut descriptor,
            None,
        )
    }
    .map_err(|e| CoreError::InvalidInput(format!("security descriptor: {e}")))?;
    let mut present = windows::core::BOOL(0);
    let mut defaulted = windows::core::BOOL(0);
    let mut dacl: *mut ACL = std::ptr::null_mut();
    // SAFETY: the descriptor was just built; the out-pointers are locals.
    let read =
        unsafe { GetSecurityDescriptorDacl(descriptor, &mut present, &mut dacl, &mut defaulted) };
    // SAFETY: the name is NUL-terminated; the DACL points into the descriptor, alive here.
    let set = read.map(|()| unsafe {
        SetNamedSecurityInfoW(
            PCWSTR(name.as_ptr()),
            SE_REGISTRY_KEY,
            DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
            None,
            None,
            Some(dacl),
            None,
        )
    });
    // SAFETY: allocated by ConvertStringSecurityDescriptorToSecurityDescriptorW, freed once.
    unsafe {
        let _ = LocalFree(Some(HLOCAL(descriptor.0)));
    }
    match set {
        Ok(code) if code.is_ok() => {}
        Ok(code) => {
            return Err(CoreError::from_registry(
                format!(r"HKLM\{INTEGRITY_KEY}"),
                std::io::Error::from_raw_os_error(code.0 as i32),
            ))
        }
        Err(e) => return Err(CoreError::InvalidInput(format!("security descriptor: {e}"))),
    }

    match key.get_raw_value("ManifestKey") {
        Ok(value) if value.bytes.len() == 32 => Ok((value.bytes.to_vec(), false)),
        _ => {
            let secret = random_bytes(32)?;
            key.set_raw_value(
                "ManifestKey",
                &RegValue {
                    bytes: secret.clone().into(),
                    vtype: winreg::enums::RegType::REG_BINARY,
                },
            )
            .map_err(denied)?;
            Ok((secret, true))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// RFC 4231, test case 2.
    #[test]
    fn hmac_sha256_matches_the_rfc() {
        let mac = hmac_sha256(b"Jefe", b"what do ya want for nothing?").unwrap();
        let hex: String = mac.iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(
            hex,
            "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
        );
        assert_ne!(random_bytes(32).unwrap(), random_bytes(32).unwrap());
    }

    #[test]
    fn descriptor_texts_are_read_up_to_their_nul() {
        let bytes = b"\0\0\0\0Samsung \0SSD 980\0";
        assert_eq!(ascii_at(bytes, 4), "Samsung");
        assert_eq!(ascii_at(bytes, 13), "SSD 980");
        assert_eq!(ascii_at(bytes, 0), "", "offset 0: no such field");
        assert_eq!(ascii_at(bytes, 99), "");
    }

    #[test]
    fn the_volume_root_of_a_folder_is_its_drive() {
        assert_eq!(
            volume_root(Path::new(r"C:\Users\Ana\AppData\Local\CleanSlate")),
            Path::new(r"C:\")
        );
        assert_eq!(volume_root(Path::new(r"X:\")), Path::new(r"X:\"));
        assert_eq!(
            volume_root(Path::new(r"\\server\share\folder")),
            Path::new(r"\\server\share\")
        );
        assert_eq!(volume_root(Path::new("relative")), Path::new("relative"));
        // A folder is accepted now: the free space of the volume of the temporary folder.
        let dir = tempfile::tempdir().unwrap();
        assert!(volume_space(dir.path()).is_some_and(|v| v.total > 0));
    }

    #[test]
    fn sha256_known_vectors() {
        assert_eq!(
            Sha256::of_bytes(b"").unwrap(),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            Sha256::of_bytes(b"abc").unwrap(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn sha256_of_file_by_chunks() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("big.bin");
        let data = vec![7u8; CHUNK * 2 + 5];
        std::fs::write(&path, &data).unwrap();
        assert_eq!(
            Sha256::of_file(&path).unwrap(),
            Sha256::of_bytes(&data).unwrap()
        );
    }

    #[test]
    fn sddl_and_move_in_a_temp_folder() {
        let dir = tempfile::tempdir().unwrap();
        let from = dir.path().join("a.txt");
        std::fs::write(&from, b"x").unwrap();
        let sddl = sddl_of(&from).expect("sddl");
        assert!(sddl.starts_with("O:"), "{sddl}");
        let to = dir.path().join("b.txt");
        move_path(&from, &to).unwrap();
        assert!(to.exists() && !from.exists());
        assert_eq!(move_path(&from, &to), Err(2), "ERROR_FILE_NOT_FOUND");
    }
}

/// Space of the volume holding a folder (D73).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VolumeSpace {
    /// Size of the volume, in bytes.
    pub total: u64,
    /// Free bytes.
    pub free: u64,
    /// Allocation unit (cluster), in bytes.
    pub cluster: u64,
}

/// Bytes a compressed or sparse file really takes on disk (`GetCompressedFileSizeW`, D73).
pub fn compressed_size(path: &Path) -> Option<u64> {
    use windows::Win32::Foundation::{GetLastError, NO_ERROR};
    use windows::Win32::Storage::FileSystem::GetCompressedFileSizeW;
    let path = wide(path);
    let mut high = 0u32;
    // SAFETY: NUL-terminated path that lives for the call; out-pointer to a local.
    let low = unsafe { GetCompressedFileSizeW(PCWSTR(path.as_ptr()), Some(&mut high)) };
    // SAFETY: reads the calling thread's last error, right after the call.
    if low == u32::MAX && unsafe { GetLastError() } != NO_ERROR {
        return None;
    }
    Some((u64::from(high) << 32) | u64::from(low))
}

/// Root of the volume of `path`: `C:\Users\Ana` gives `C:\`, `\\server\share\x` gives
/// `\\server\share\`. A path without a drive is returned as it is.
pub fn volume_root(path: &Path) -> std::path::PathBuf {
    use std::path::Component;
    let mut root = std::path::PathBuf::new();
    for part in path.components() {
        match part {
            Component::Prefix(_) | Component::RootDir => root.push(part.as_os_str()),
            _ => break,
        }
    }
    if root.as_os_str().is_empty() {
        path.to_path_buf()
    } else {
        root
    }
}

/// Total, free and cluster size of the volume holding `path`, a root or any folder on it
/// (D73). `GetDiskFreeSpaceW` only accepts a root: a folder failed before D80.
pub fn volume_space(path: &Path) -> Option<VolumeSpace> {
    use windows::Win32::Storage::FileSystem::{GetDiskFreeSpaceExW, GetDiskFreeSpaceW};
    let path = wide(&volume_root(path));
    let (mut free, mut total) = (0u64, 0u64);
    // SAFETY: NUL-terminated path that lives for the call; out-pointers to locals.
    unsafe {
        GetDiskFreeSpaceExW(
            PCWSTR(path.as_ptr()),
            Some(&mut free),
            Some(&mut total),
            None,
        )
    }
    .ok()?;
    let (mut sectors, mut bytes) = (0u32, 0u32);
    // SAFETY: same path; out-pointers to locals; the cluster counts are not asked.
    unsafe {
        GetDiskFreeSpaceW(
            PCWSTR(path.as_ptr()),
            Some(&mut sectors),
            Some(&mut bytes),
            None,
            None,
        )
    }
    .ok()?;
    Some(VolumeSpace {
        total,
        free,
        cluster: u64::from(sectors) * u64::from(bytes),
    })
}
