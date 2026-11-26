//! Exposes the short commit of the build as `EMU_GIT_SHA` (footer and `--version`), to tell two builds of the
//! same version apart. CI: `GITHUB_SHA`; else `git rev-parse --short HEAD`; no git or no repository: nothing.
//! No network, no dependency.

use std::path::{Path, PathBuf};
use std::process::Command;

const SHORT: usize = 7;

fn git(args: &[&str]) -> Option<String> {
    let out = Command::new("git").args(args).output().ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8(out.stdout).ok()?;
    let text = text.trim();
    (!text.is_empty()).then(|| text.to_owned())
}

fn is_hex(s: &str) -> bool {
    s.len() >= SHORT && s.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Re-run when the checked-out commit changes: `HEAD` (branch switch) and the ref it points to (new commit).
fn watch_git(manifest_dir: &Path) {
    let path = |name: &str| -> Option<PathBuf> {
        let p = PathBuf::from(git(&[
            "-C",
            manifest_dir.to_str()?,
            "rev-parse",
            "--git-path",
            name,
        ])?);
        Some(if p.is_absolute() {
            p
        } else {
            manifest_dir.join(p)
        })
    };
    if let Some(head) = path("HEAD") {
        println!("cargo:rerun-if-changed={}", head.display());
        let target = std::fs::read_to_string(&head)
            .ok()
            .and_then(|t| t.trim().strip_prefix("ref: ").map(str::to_owned));
        if let Some(r) = target.and_then(|t| path(&t)) {
            println!("cargo:rerun-if-changed={}", r.display());
        }
    }
    if let Some(packed) = path("packed-refs") {
        println!("cargo:rerun-if-changed={}", packed.display());
    }
}

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=GITHUB_SHA");
    let manifest_dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap_or_default());
    let sha = std::env::var("GITHUB_SHA")
        .ok()
        .map(|s| s.trim().to_owned())
        .filter(|s| is_hex(s))
        .map(|s| s[..SHORT].to_owned())
        .or_else(|| {
            watch_git(&manifest_dir);
            git(&[
                "-C",
                manifest_dir.to_str()?,
                "rev-parse",
                "--short=7",
                "HEAD",
            ])
        });
    if let Some(sha) = sha {
        println!("cargo:rustc-env=EMU_GIT_SHA={sha}");
    }
}
