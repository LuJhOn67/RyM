//! The Google sign-in, kept so that only you can read it.
//! - Windows: encrypted with DPAPI, tied to your Windows account (`cookie.bin`); another account
//!   or another PC can't open it.
//! - Linux: a file only your user can read (`cookie.txt`, mode 0600).
//!
//! An old plain `cookie.txt` on Windows is encrypted the first time it's read, then deleted.

use std::path::PathBuf;

fn sealed_path() -> PathBuf {
    crate::data_dir().join("cookie.bin")
}

/// The saved cookie header, if signed in.
pub fn read_cookie() -> Option<String> {
    #[cfg(windows)]
    {
        if let Some(text) = std::fs::read(sealed_path()).ok().and_then(|b| dpapi::unprotect(&b)).and_then(|b| String::from_utf8(b).ok()) {
            return Some(text).filter(|t| !t.trim().is_empty());
        }
        // An old plain file: sealed now, the plain one goes.
        let plain = std::fs::read_to_string(crate::cookie_path()).ok().filter(|t| !t.trim().is_empty())?;
        if write_cookie(&plain).is_ok() {
            let _ = std::fs::remove_file(crate::cookie_path());
        }
        Some(plain)
    }
    #[cfg(not(windows))]
    std::fs::read_to_string(crate::cookie_path()).ok().filter(|t| !t.trim().is_empty())
}

/// Saves the cookie header (sealed on Windows, owner-only on Linux).
pub fn write_cookie(text: &str) -> std::io::Result<()> {
    std::fs::create_dir_all(crate::data_dir())?;
    #[cfg(windows)]
    {
        let sealed = dpapi::protect(text.as_bytes()).ok_or_else(|| std::io::Error::other("DPAPI couldn't encrypt the sign-in"))?;
        let tmp = sealed_path().with_extension("bin.tmp");
        std::fs::write(&tmp, sealed)?;
        std::fs::rename(&tmp, sealed_path())?;
        let _ = std::fs::remove_file(crate::cookie_path());
        Ok(())
    }
    #[cfg(not(windows))]
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let mut f = std::fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(crate::cookie_path())?;
        f.write_all(text.as_bytes())?;
        // An older file made before this keeps its old mode: set it again.
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(crate::cookie_path(), std::fs::Permissions::from_mode(0o600))
    }
}

/// Signs out: the saved sign-in is gone (both the sealed and any plain file).
pub fn forget_cookie() {
    let _ = std::fs::remove_file(sealed_path());
    let _ = std::fs::remove_file(crate::cookie_path());
}

/// [`read_cookie`] shaped like `fs::read_to_string` (NotFound when signed out).
pub fn read_cookie_io() -> std::io::Result<String> {
    read_cookie().ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "not signed in"))
}

pub fn has_cookie() -> bool {
    read_cookie().is_some()
}

#[cfg(windows)]
mod dpapi {
    #[repr(C)]
    struct Blob {
        len: u32,
        data: *mut u8,
    }

    #[link(name = "crypt32")]
    unsafe extern "system" {
        fn CryptProtectData(input: *const Blob, description: *const u16, entropy: *const Blob, reserved: *const u8, prompt: *const u8, flags: u32, output: *mut Blob) -> i32;
        fn CryptUnprotectData(input: *const Blob, description: *mut *mut u16, entropy: *const Blob, reserved: *const u8, prompt: *const u8, flags: u32, output: *mut Blob) -> i32;
    }

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn LocalFree(mem: *mut u8) -> *mut u8;
    }

    /// No UI may ever appear (the apps call this at start).
    const UI_FORBIDDEN: u32 = 0x1;

    /// Extra entropy: RyM's own, so another program running as you can't open it by asking
    /// DPAPI alone without knowing it.
    const ENTROPY: &[u8] = b"RyM sign-in v1";

    fn call(input: &[u8], seal: bool) -> Option<Vec<u8>> {
        let inp = Blob { len: input.len() as u32, data: input.as_ptr() as *mut u8 };
        let ent = Blob { len: ENTROPY.len() as u32, data: ENTROPY.as_ptr() as *mut u8 };
        let mut out = Blob { len: 0, data: std::ptr::null_mut() };
        // SAFETY: the blobs point at live slices for the call; DPAPI allocates `out` with
        // LocalAlloc, which is copied and freed below.
        unsafe {
            let ok = if seal {
                CryptProtectData(&inp, std::ptr::null(), &ent, std::ptr::null(), std::ptr::null(), UI_FORBIDDEN, &mut out)
            } else {
                CryptUnprotectData(&inp, std::ptr::null_mut(), &ent, std::ptr::null(), std::ptr::null(), UI_FORBIDDEN, &mut out)
            };
            if ok == 0 || out.data.is_null() {
                return None;
            }
            let bytes = std::slice::from_raw_parts(out.data, out.len as usize).to_vec();
            LocalFree(out.data);
            Some(bytes)
        }
    }

    pub fn protect(plain: &[u8]) -> Option<Vec<u8>> {
        call(plain, true)
    }

    pub fn unprotect(sealed: &[u8]) -> Option<Vec<u8>> {
        call(sealed, false)
    }

    #[cfg(test)]
    mod tests {
        #[test]
        fn seals_and_opens() {
            let text = b"SAPISID=abc; __Secure-3PSID=xyz";
            let sealed = super::protect(text).expect("sealed");
            assert!(!sealed.windows(6).any(|w| w == b"SAPISI"), "plain text must not show in the sealed bytes");
            assert_eq!(super::unprotect(&sealed).as_deref(), Some(&text[..]));
            // Tampered bytes don't open.
            let mut bad = sealed.clone();
            let last = bad.len() - 1;
            bad[last] ^= 0xff;
            assert!(super::unprotect(&bad).is_none());
        }
    }
}
