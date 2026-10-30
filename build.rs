//! Windows: each program gets its own icon inside its .exe (RyM the doll, RyM-Legacy-Player and RymPlayer-GTK the pear).

fn main() {
    println!("cargo:rerun-if-changed=assets/terminal.rc");
    println!("cargo:rerun-if-changed=assets/gui.rc");
    println!("cargo:rerun-if-changed=assets/icons");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        embed_resource::compile_for("assets/terminal.rc", ["RyM"], embed_resource::NONE).manifest_optional().unwrap();
        embed_resource::compile_for("assets/gui.rc", ["RyM-Legacy-Player", "RymPlayer-GTK"], embed_resource::NONE).manifest_optional().unwrap();
        // The GTK app asks hybrid laptops' drivers for the fast graphics card (see its main.rs).
        if std::env::var_os("CARGO_FEATURE_GTK_UI").is_some() && std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
            println!("cargo:rustc-link-arg-bin=RymPlayer-GTK=/EXPORT:NvOptimusEnablement");
            println!("cargo:rustc-link-arg-bin=RymPlayer-GTK=/EXPORT:AmdPowerXpressRequestHighPerformance");
        }
    }
}
