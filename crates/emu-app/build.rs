//! Windows resource of both executables (icon `packaging/icon/emu-rust.ico`, version info) through `winresource`,
//! written by FU1 once FP1 has generated the `.ico` (ADR 0023 §2). Cross-compilation from Linux: no resource, no
//! failure. Until then: nothing to do.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
}
