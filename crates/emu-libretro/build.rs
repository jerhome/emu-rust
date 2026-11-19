//! Compiles the C shim behind `GET_LOG_INTERFACE` (ADR 0009).

fn main() {
    println!("cargo:rerun-if-changed=src/log_shim.c");
    cc::Build::new()
        .file("src/log_shim.c")
        .warnings(true)
        .extra_warnings(true)
        .compile("emu_libretro_log_shim");
}
