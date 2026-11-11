//! Windows: each program gets its own icon inside its .exe (RyM the doll, RyM Slint the pear),
//! and its details (name, version, description, copyright: what Properties > Details shows),
//! the version taken from Cargo.toml.
//! The GTK app is built for Linux only.
//! The Slint app's window (ui/app.slint) is compiled here when it's built.

fn main() {
    println!("cargo:rerun-if-changed=assets/icons");
    println!("cargo:rerun-if-changed=build.rs");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let rc = |bin: &str, icon: &str, description: &str| {
            let path = std::path::Path::new(&std::env::var("OUT_DIR").unwrap()).join(format!("{bin}.rc"));
            std::fs::write(&path, resource(bin, icon, description)).unwrap();
            embed_resource::compile_for(&path, [bin], embed_resource::NONE).manifest_optional().unwrap();
        };
        rc("RyM", "rym-terminal.ico", "RyM - YouTube Music player for the terminal");
        rc("RyM-Slint", "rym-slint.ico", "RyM Slint - YouTube Music player");
    }
    #[cfg(feature = "slint-ui")]
    slint_build::compile("ui/app.slint").expect("ui/app.slint");
}

/// The resource script: the icon, then the version block.
fn resource(bin: &str, icon: &str, description: &str) -> String {
    let dir = std::env::var("CARGO_MANIFEST_DIR").unwrap().replace('\\', "/");
    let v = |k: &str| std::env::var(k).unwrap_or_else(|_| "0".into());
    let (major, minor, patch) = (v("CARGO_PKG_VERSION_MAJOR"), v("CARGO_PKG_VERSION_MINOR"), v("CARGO_PKG_VERSION_PATCH"));
    let version = std::env::var("CARGO_PKG_VERSION").unwrap_or_default();
    format!(
        r#"1 ICON "{dir}/assets/icons/{icon}"
1 VERSIONINFO
FILEVERSION {major},{minor},{patch},0
PRODUCTVERSION {major},{minor},{patch},0
FILEFLAGSMASK 0x3fL
FILEFLAGS 0x0L
FILEOS 0x40004L
FILETYPE 0x1L
FILESUBTYPE 0x0L
BEGIN
    BLOCK "StringFileInfo"
    BEGIN
        BLOCK "040904b0"
        BEGIN
            VALUE "CompanyName", "LuJhOn67"
            VALUE "FileDescription", "{description}"
            VALUE "FileVersion", "{version}"
            VALUE "InternalName", "{bin}"
            VALUE "LegalCopyright", "Copyright (c) 2026 LuJhOn67 (MIT License)"
            VALUE "OriginalFilename", "{bin}.exe"
            VALUE "ProductName", "RyM"
            VALUE "ProductVersion", "{version}"
            VALUE "Comments", "github.com/LuJhOn67/RyM"
        END
    END
    BLOCK "VarFileInfo"
    BEGIN
        VALUE "Translation", 0x409, 1200
    END
END
"#
    )
}
