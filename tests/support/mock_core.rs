//! Builds the mock libretro core on demand (ADR 0009). Include with
//! `#[path = "../../../tests/support/mock_core.rs"] mod mock_core;` from a crate's integration tests.
//! Output: `target/test-output/mock-core/<variant>.so` (`.dylib` on macOS), rebuilt when the C source is newer.
#![allow(dead_code)]

use std::path::{Path, PathBuf};
#[cfg(unix)]
use std::process::Command;

/// Compile-time variants of the mock core.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Variant {
    /// Content in memory, options v2.
    Default,
    /// `need_fullpath = true`.
    NeedFullpath,
    /// Options declared with `SET_CORE_OPTIONS` (v1).
    OptionsV1,
    /// Options declared with `SET_VARIABLES`.
    LegacyOptions,
    /// RGB565 frames with padded rows.
    Rgb565,
    /// 0RGB1555 frames with padded rows.
    Xrgb1555,
    /// The core changes `mock_color` itself (`SET_VARIABLE`) at frame 20 and reports `GET_VARIABLE_UPDATE`.
    DynamicOptions,
    /// Linked with `-z nodelete` (Linux): `dlclose` never unmaps it, like dosbox_pure (ADR 0022). Meant for use with
    /// `MOCK_REFUSE_REINIT`.
    NoDelete,
}

impl Variant {
    fn name(self) -> &'static str {
        match self {
            Variant::Default => "default",
            Variant::NeedFullpath => "fullpath",
            Variant::OptionsV1 => "options_v1",
            Variant::LegacyOptions => "legacy",
            Variant::Rgb565 => "rgb565",
            Variant::Xrgb1555 => "xrgb1555",
            Variant::DynamicOptions => "dynamic",
            Variant::NoDelete => "nodelete",
        }
    }

    fn define(self) -> Option<&'static str> {
        match self {
            Variant::Default => None,
            Variant::NeedFullpath => Some("-DMOCK_NEED_FULLPATH"),
            Variant::OptionsV1 => Some("-DMOCK_OPTIONS_V1"),
            Variant::LegacyOptions => Some("-DMOCK_LEGACY_OPTIONS"),
            Variant::Rgb565 => Some("-DMOCK_PIXEL_RGB565"),
            Variant::Xrgb1555 => Some("-DMOCK_PIXEL_1555"),
            Variant::DynamicOptions => Some("-DMOCK_DYNAMIC_OPTIONS"),
            Variant::NoDelete => None,
        }
    }
}

/// Workspace root, assuming the including crate lives in `crates/<name>/`.
pub fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Directory for test artifacts (ROMs written by tests, saves, built cores).
pub fn output_dir() -> PathBuf {
    workspace_root().join("target/test-output")
}

/// Returns the path of the compiled mock core, building it if missing or outdated.
/// Safe to call from parallel tests and parallel test binaries (write to a temp file, then rename).
#[cfg(unix)]
pub fn build(variant: Variant) -> PathBuf {
    let root = workspace_root();
    let source = root.join("tests/mock-core/mock_core.c");
    let dir = output_dir().join("mock-core");
    std::fs::create_dir_all(&dir).expect("create mock-core output dir");
    let ext = if cfg!(target_os = "macos") {
        "dylib"
    } else {
        "so"
    };
    let out = dir.join(format!("mock_core_{}.{ext}", variant.name()));

    // Serialize builds inside this process; a poisoned lock only means another test panicked.
    static BUILD_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let _guard = BUILD_LOCK.lock().unwrap_or_else(|e| e.into_inner());

    let mtime = |p: &Path| std::fs::metadata(p).and_then(|m| m.modified()).ok();
    if let (Some(o), Some(s)) = (mtime(&out), mtime(&source))
        && o >= s
    {
        return out;
    }

    let tmp = dir.join(format!(
        "mock_core_{}.{}.{}.tmp",
        variant.name(),
        std::process::id(),
        COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    let compiler = std::env::var("CC").unwrap_or_else(|_| "cc".to_string());
    let mut cmd = Command::new(&compiler);
    // clang (macOS `cc`) wants -dynamiclib; GNU ld wants -shared.
    let link = if cfg!(target_os = "macos") {
        "-dynamiclib"
    } else {
        "-shared"
    };
    cmd.args([link, "-fPIC", "-O1", "-Wall", "-Wextra", "-std=c99"]);
    if let Some(d) = variant.define() {
        cmd.arg(d);
    }
    if variant == Variant::NoDelete {
        cmd.arg("-Wl,-z,nodelete");
    }
    cmd.arg("-o").arg(&tmp).arg(&source);
    let output = cmd
        .output()
        .unwrap_or_else(|e| panic!("cannot run C compiler `{compiler}`: {e}"));
    assert!(
        output.status.success(),
        "mock core build failed ({}):\n{}",
        variant.name(),
        String::from_utf8_lossy(&output.stderr)
    );
    if let Err(e) = std::fs::rename(&tmp, &out) {
        let _ = std::fs::remove_file(&tmp);
        // Another process may have installed the same file meanwhile.
        assert!(out.exists(), "install mock core: {e}");
    }
    out
}
