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

#[cfg(test)]
mod tests {
    use super::*;

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
