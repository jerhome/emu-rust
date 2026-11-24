//! Windows executables get `assets/icon.ico` (design v7.1 point 2). The target OS is read from Cargo, not
//! `cfg!(windows)`, which describes the build host.

fn main() {
    const ICON: &str = "../../assets/icon.ico";
    println!("cargo::rerun-if-changed={ICON}");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let mut res = winresource::WindowsResource::new();
    res.set_icon(ICON);
    if let Err(e) = res.compile() {
        panic!("icône Windows non embarquée ({ICON}) : {e}");
    }
}
