//! Linux: the fonts that come with RyM (beside the program, or in the AppImage): Noto Sans JP so
//! Japanese isn't drawn as boxes on systems without a CJK font, and Doto for the dot digits.
//! RyM GTK and RyM Slint both draw through fontconfig, so adding the folder to it is enough.

/// Adds RyM's own font folders to fontconfig (nothing on other systems).
pub fn add_bundled() {
    #[cfg(target_os = "linux")]
    {
        #[link(name = "fontconfig")]
        unsafe extern "C" {
            fn FcConfigAppFontAddDir(config: *mut std::ffi::c_void, dir: *const std::ffi::c_char) -> i32;
        }
        let beside = std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.join("fonts")));
        let in_appimage = std::env::var_os("APPDIR").map(|a| std::path::PathBuf::from(a).join("usr/share/rym/fonts"));
        for dir in [beside, in_appimage].into_iter().flatten().filter(|d| d.is_dir()) {
            if let Ok(c) = std::ffi::CString::new(dir.to_string_lossy().as_bytes()) {
                // SAFETY: a null config means the current one; the string lives through the call.
                unsafe { FcConfigAppFontAddDir(std::ptr::null_mut(), c.as_ptr()) };
            }
        }
    }
}
