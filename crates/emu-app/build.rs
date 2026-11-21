//! Windows resource of both executables (icon `packaging/icon/emu-rust.ico`, version information) through
//! `winresource` (ADR 0023 §2). No effect when the target is not Windows, when the build host is not Windows
//! (the build-dependency only exists there), or while the `.ico` has not been generated.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=../../packaging/icon/emu-rust.ico");
    #[cfg(windows)]
    embed_resource();
}

#[cfg(windows)]
fn embed_resource() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let icon = std::path::Path::new(&std::env::var("CARGO_MANIFEST_DIR").unwrap_or_default())
        .join("../../packaging/icon/emu-rust.ico");
    let Some(icon) = icon.to_str().filter(|_| icon.is_file()) else {
        println!("cargo:warning=packaging/icon/emu-rust.ico absent : exécutable sans icône");
        return;
    };
    let mut resource = winresource::WindowsResource::new();
    resource
        .set_icon(icon)
        .set("ProductName", "emu-rust")
        .set("FileDescription", "emu-rust");
    if let Err(e) = resource.compile() {
        println!("cargo:warning=ressource Windows non écrite : {e}");
    }
}
