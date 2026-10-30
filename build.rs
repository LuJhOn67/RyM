//! Windows: each program gets its own icon inside its .exe (RyM the doll, RymPlayer-GUI the pear).

fn main() {
    println!("cargo:rerun-if-changed=assets/terminal.rc");
    println!("cargo:rerun-if-changed=assets/gui.rc");
    println!("cargo:rerun-if-changed=assets/icons");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        embed_resource::compile_for("assets/terminal.rc", ["RyM"], embed_resource::NONE).manifest_optional().unwrap();
        embed_resource::compile_for("assets/gui.rc", ["RymPlayer-GUI", "RymPlayer-GTK"], embed_resource::NONE).manifest_optional().unwrap();
    }
}
