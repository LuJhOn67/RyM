//! Windows: each program gets its own icon inside its .exe (RyM the doll, RyM Slint the pear).
//! The GTK app is built for Linux only.
//! The Slint app's window (ui/app.slint) is compiled here when it's built.

fn main() {
    println!("cargo:rerun-if-changed=assets/terminal.rc");
    println!("cargo:rerun-if-changed=assets/gui.rc");
    println!("cargo:rerun-if-changed=assets/icons");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        embed_resource::compile_for("assets/terminal.rc", ["RyM"], embed_resource::NONE).manifest_optional().unwrap();
        embed_resource::compile_for("assets/gui.rc", ["RyM-Slint"], embed_resource::NONE).manifest_optional().unwrap();
    }
    #[cfg(feature = "slint-ui")]
    slint_build::compile("ui/app.slint").expect("ui/app.slint");
}
