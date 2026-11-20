//! The shipped EmuTOS catalogue (`data/free_bios.toml`) against the pins of `scripts/fetch-test-assets.sh` and,
//! in an `#[ignore]` test, against the real SourceForge download (ADR 0005, ADR 0007).

use std::fs;
use std::path::{Path, PathBuf};

use emu_assets::bios::{FreeBiosManager, FreeBiosState, ReplacePolicy, find_free_bios};
use emu_assets::http::{CancelToken, HttpClient, HttpConfig};
use sha2::{Digest, Sha256};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Value of `NAME='...'` in the fetch script.
fn script_pin(name: &str) -> String {
    let script = fs::read_to_string(root().join("scripts/fetch-test-assets.sh")).unwrap();
    let prefix = format!("{name}='");
    let line = script
        .lines()
        .find_map(|l| l.strip_prefix(&prefix))
        .unwrap_or_else(|| panic!("{name} not pinned in fetch-test-assets.sh"));
    line.trim_end_matches('\'').to_owned()
}

#[test]
fn catalogue_matches_the_pins_of_the_fetch_script() {
    let bios = find_free_bios("emutos").expect("emutos in the embedded catalogue");
    assert_eq!(
        bios.install_as, "tos.img",
        "the hatari core looks for tos.img"
    );
    assert_eq!(bios.system, "Atari - ST");
    let archive = bios.archive("emutos-192k-1.4.zip").expect("192k archive");
    assert_eq!(hex(&archive.sha256), script_pin("ETOS_ZIP_SHA256"));
    assert_eq!(archive.url, script_pin("ETOS_ZIP_URL"));
    for (id, pin) in [("192us", "ETOS_US_SHA256"), ("192fr", "ETOS_FR_SHA256")] {
        let variant = bios.variant(id).unwrap_or_else(|| panic!("variant {id}"));
        assert_eq!(hex(&variant.sha256), script_pin(pin), "variant {id}");
        assert_eq!(variant.size, 196_608);
    }
}

#[test]
fn every_variant_points_to_a_known_archive() {
    let bios = find_free_bios("emutos").unwrap();
    assert!(bios.variant(&bios.default_variant).is_some());
    assert!(!bios.variants.is_empty());
    for variant in &bios.variants {
        assert!(
            bios.archive(&variant.archive).is_some(),
            "variant {} → unknown archive {}",
            variant.id,
            variant.archive
        );
        assert!(
            variant
                .entry
                .starts_with(&variant.archive.replace(".zip", "/")),
            "variant {}: entry {} outside its archive folder",
            variant.id,
            variant.entry
        );
    }
    for archive in &bios.archives {
        assert!(archive.url.starts_with("https://"), "{}", archive.url);
    }
}

#[test]
#[ignore = "real network (SourceForge) and EmuTOS from scripts/fetch-test-assets.sh"]
fn real_download_installs_the_pinned_image() {
    let bios = find_free_bios("emutos").unwrap();
    let dir = tempfile::tempdir().unwrap();
    let system = dir.path().join("system");
    let http = HttpClient::new(HttpConfig::default()).unwrap();
    let manager = FreeBiosManager::open(&system, http).unwrap();
    assert_eq!(manager.state(&bios), FreeBiosState::Missing);
    let target = manager
        .install(
            &bios,
            "192us",
            ReplacePolicy::Refuse,
            &mut |_| {},
            &CancelToken::new(),
        )
        .expect("download and install EmuTOS");
    let image = fs::read(&target).unwrap();
    assert_eq!(hex(&Sha256::digest(&image)), script_pin("ETOS_US_SHA256"));
    let local = root().join("tests/roms/system/etos192us.img");
    assert!(local.is_file(), "run scripts/fetch-test-assets.sh");
    assert_eq!(image, fs::read(local).unwrap());
    assert!(matches!(
        manager.state(&bios),
        FreeBiosState::Installed { .. }
    ));
}
