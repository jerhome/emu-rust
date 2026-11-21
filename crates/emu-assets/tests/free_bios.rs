//! Free BIOS installer against a local mock server (wiremock): no real network. The catalogue is injected with
//! hashes computed from a fixture archive (the real `data/free_bios.toml` is checked by unit tests).

use std::fs;
use std::io::{Cursor, Write};
use std::path::Path;
use std::time::{Duration, SystemTime};

use emu_assets::AssetError;
use emu_assets::bios::{
    ArchiveFormat, BiosArchive, BiosError, BiosVariant, FreeBios, FreeBiosKind, FreeBiosManager,
    FreeBiosState, MANIFEST_FILE_NAME, ReplacePolicy,
};
use emu_assets::http::{CancelToken, HttpClient, HttpConfig};
use md5::Md5;
use sha2::{Digest, Sha256};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};
use zip::write::SimpleFileOptions;

const ZIP_PATH: &str = "/project/emutos/emutos/1.4/emutos-192k-1.4.zip";
const IMAGE_FR: &[u8] = b"EmuTOS fr fixture image";
const IMAGE_US: &[u8] = b"EmuTOS us fixture image, longer";

struct Server {
    server: MockServer,
    rt: tokio::runtime::Runtime,
}

impl Server {
    fn start() -> Self {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .unwrap();
        let server = rt.block_on(MockServer::start());
        Self { server, rt }
    }

    fn mount(&self, mock: Mock) {
        self.rt.block_on(mock.mount(&self.server));
    }

    fn serve(&self, body: Vec<u8>, expected_calls: u64) {
        self.mount(
            Mock::given(method("GET"))
                .and(path(ZIP_PATH))
                .respond_with(ResponseTemplate::new(200).set_body_bytes(body))
                .expect(expected_calls),
        );
    }

    fn url(&self) -> String {
        format!("{}{ZIP_PATH}", self.server.uri())
    }

    fn verify(&self) {
        self.rt.block_on(self.server.verify());
    }
}

fn client() -> HttpClient {
    HttpClient::new(HttpConfig {
        allow_loopback_http: true,
        timeout: Duration::from_secs(10),
        ..HttpConfig::default()
    })
    .unwrap()
}

fn zip_of(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, data) in entries {
        zip.start_file(*name, SimpleFileOptions::default()).unwrap();
        zip.write_all(data).unwrap();
    }
    zip.finish().unwrap().into_inner()
}

/// Real layout: images in a folder, plus documentation and a hostile name that must never be written.
fn fixture_zip() -> Vec<u8> {
    zip_of(&[
        ("emutos-192k-1.4/readme.txt", b"EmuTOS"),
        ("../../evil.img", IMAGE_FR),
        ("emutos-192k-1.4/etos192fr.img", IMAGE_FR),
        ("emutos-192k-1.4/etos192us.img", IMAGE_US),
    ])
}

fn variant(id: &str, entry: &str, image: &[u8], language: &str) -> BiosVariant {
    BiosVariant {
        id: id.into(),
        label: format!("EmuTOS 192 Ko — {language}"),
        language: Some(language.into()),
        archive: "emutos-192k-1.4.zip".into(),
        entry: entry.into(),
        size: image.len() as u64,
        sha256: Sha256::digest(image).into(),
        md5: Md5::digest(image).into(),
    }
}

fn catalogue(url: &str, archive: &[u8]) -> FreeBios {
    FreeBios {
        id: "emutos".into(),
        name: "EmuTOS".into(),
        version: "1.4".into(),
        license: "GPL-2.0-or-later".into(),
        homepage: "https://emutos.sourceforge.io/".into(),
        system: "Atari - ST".into(),
        install_as: "tos.img".into(),
        archives: vec![BiosArchive {
            name: "emutos-192k-1.4.zip".into(),
            url: url.into(),
            size: archive.len() as u64,
            sha256: Sha256::digest(archive).into(),
            format: ArchiveFormat::Zip,
        }],
        variants: vec![
            variant("192fr", "emutos-192k-1.4/etos192fr.img", IMAGE_FR, "fr"),
            variant("192us", "emutos-192k-1.4/etos192us.img", IMAGE_US, "en"),
            variant("192de", "emutos-192k-1.4/etos192de.img", IMAGE_US, "de"),
        ],
        default_variant: "192fr".into(),
        kind: FreeBiosKind::Bios,
    }
}

fn install(
    manager: &FreeBiosManager,
    bios: &FreeBios,
    variant: &str,
    policy: ReplacePolicy,
) -> Result<std::path::PathBuf, BiosError> {
    manager.install(bios, variant, policy, &mut |_| {}, &CancelToken::new())
}

/// Every file of `dir` (names only), sorted.
fn listing(dir: &Path) -> Vec<String> {
    let mut names: Vec<_> = fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

#[test]
fn install_writes_the_image_and_the_manifest() {
    let server = Server::start();
    let zip = fixture_zip();
    server.serve(zip.clone(), 1);
    let dir = tempfile::tempdir().unwrap();
    let system = dir.path().join("data/system");
    let bios = catalogue(&server.url(), &zip);
    let manager = FreeBiosManager::open(&system, client()).unwrap();
    assert_eq!(manager.state(&bios), FreeBiosState::Missing);

    let mut calls = 0;
    let target = manager
        .install(
            &bios,
            "192fr",
            ReplacePolicy::Refuse,
            &mut |p| {
                calls += 1;
                assert!(p.done <= zip.len() as u64);
            },
            &CancelToken::new(),
        )
        .unwrap();
    assert!(calls > 0);
    assert_eq!(target, system.join("tos.img"));
    assert_eq!(fs::read(&target).unwrap(), IMAGE_FR);
    assert_eq!(listing(&system), [MANIFEST_FILE_NAME, "tos.img"]);
    // Nothing written from the hostile entry name.
    assert!(!dir.path().join("evil.img").exists());
    assert!(!dir.path().join("data/evil.img").exists());
    assert_eq!(
        manager.state(&bios),
        FreeBiosState::Installed {
            variant: "192fr".into()
        }
    );
    let md5: [u8; 16] = Md5::digest(IMAGE_FR).into();
    let installed = manager.installed_md5();
    assert_eq!(installed.len(), 1);
    assert_eq!(installed[0].0, "tos.img");
    assert_eq!(installed[0].1, md5);
    assert!(installed[0].2.contains("1.4"));
    let manifest = fs::read_to_string(system.join(MANIFEST_FILE_NAME)).unwrap();
    assert!(manifest.contains("variant = \"192fr\""));
    assert!(!manifest.contains("127.0.0.1"));
    server.verify();
}

#[test]
fn switching_variant_replaces_our_own_file() {
    let server = Server::start();
    let zip = fixture_zip();
    server.serve(zip.clone(), 2);
    let dir = tempfile::tempdir().unwrap();
    let bios = catalogue(&server.url(), &zip);
    let manager = FreeBiosManager::open(dir.path(), client()).unwrap();
    install(&manager, &bios, "192fr", ReplacePolicy::Refuse).unwrap();
    install(&manager, &bios, "192us", ReplacePolicy::Refuse).unwrap();
    assert_eq!(fs::read(dir.path().join("tos.img")).unwrap(), IMAGE_US);
    assert_eq!(
        manager.state(&bios),
        FreeBiosState::Installed {
            variant: "192us".into()
        }
    );
    assert_eq!(manager.installed_md5().len(), 1);
    server.verify();
}

#[test]
fn wrong_archive_hash_writes_nothing() {
    let server = Server::start();
    let zip = fixture_zip();
    let mut tampered = zip.clone();
    // Same size, last byte changed: refused on the SHA-256, before the zip is even opened.
    let last = tampered.len() - 1;
    tampered[last] ^= 0xff;
    server.serve(tampered, 1);
    let dir = tempfile::tempdir().unwrap();
    let bios = catalogue(&server.url(), &zip);
    let manager = FreeBiosManager::open(dir.path(), client()).unwrap();
    let err = install(&manager, &bios, "192fr", ReplacePolicy::Refuse).unwrap_err();
    assert!(
        matches!(&err, BiosError::HashMismatch(n) if n == "emutos-192k-1.4.zip"),
        "{err:?}"
    );
    assert!(listing(dir.path()).is_empty());
    server.verify();
}

#[test]
fn oversized_body_is_refused() {
    let server = Server::start();
    let zip = fixture_zip();
    let mut longer = zip.clone();
    longer.extend_from_slice(&[0; 1024]);
    server.serve(longer, 1);
    let dir = tempfile::tempdir().unwrap();
    let bios = catalogue(&server.url(), &zip);
    let manager = FreeBiosManager::open(dir.path(), client()).unwrap();
    let err = install(&manager, &bios, "192fr", ReplacePolicy::Refuse).unwrap_err();
    assert!(
        matches!(err, BiosError::Asset(AssetError::TooLarge { .. })),
        "{err:?}"
    );
    assert!(listing(dir.path()).is_empty());
}

#[test]
fn wrong_image_hash_and_missing_entry() {
    let server = Server::start();
    let zip = fixture_zip();
    server.serve(zip.clone(), 2);
    let dir = tempfile::tempdir().unwrap();
    let mut bios = catalogue(&server.url(), &zip);
    bios.variants[0].sha256[0] ^= 1;
    let manager = FreeBiosManager::open(dir.path(), client()).unwrap();
    let err = install(&manager, &bios, "192fr", ReplacePolicy::Refuse).unwrap_err();
    assert!(
        matches!(&err, BiosError::HashMismatch(n) if n.ends_with("etos192fr.img")),
        "{err:?}"
    );
    let err = install(&manager, &bios, "192de", ReplacePolicy::Refuse).unwrap_err();
    assert!(matches!(err, BiosError::MissingEntry(_)), "{err:?}");
    assert!(listing(dir.path()).is_empty());
    let err = install(&manager, &bios, "192xx", ReplacePolicy::Refuse).unwrap_err();
    assert!(matches!(err, BiosError::UnknownVariant(_)), "{err:?}");
    server.verify();
}

#[test]
fn redirection_to_another_https_host_is_followed() {
    // Two local servers = two origins (SourceForge → mirror). The caller's client keeps the default
    // `same_host_redirects = true`: the manager relaxes it for BIOS downloads.
    let mirror = Server::start();
    let zip = fixture_zip();
    mirror.serve(zip.clone(), 1);
    let front = Server::start();
    front.mount(
        Mock::given(method("GET"))
            .and(path(ZIP_PATH))
            .respond_with(ResponseTemplate::new(302).insert_header("Location", mirror.url()))
            .expect(1),
    );
    let dir = tempfile::tempdir().unwrap();
    let bios = catalogue(&front.url(), &zip);
    let manager = FreeBiosManager::open(dir.path(), client()).unwrap();
    install(&manager, &bios, "192fr", ReplacePolicy::Refuse).unwrap();
    assert_eq!(fs::read(dir.path().join("tos.img")).unwrap(), IMAGE_FR);
    front.verify();
    mirror.verify();
}

#[test]
fn http_errors_and_cancellation() {
    let server = Server::start();
    let zip = fixture_zip();
    server.mount(
        Mock::given(method("GET"))
            .and(path(ZIP_PATH))
            .respond_with(ResponseTemplate::new(404)),
    );
    let dir = tempfile::tempdir().unwrap();
    let bios = catalogue(&server.url(), &zip);
    let manager = FreeBiosManager::open(dir.path(), client()).unwrap();
    let err = install(&manager, &bios, "192fr", ReplacePolicy::Refuse).unwrap_err();
    assert!(
        matches!(err, BiosError::Asset(AssetError::Http { status: 404 })),
        "{err:?}"
    );
    let cancel = CancelToken::new();
    cancel.cancel();
    let err = manager
        .install(&bios, "192fr", ReplacePolicy::Refuse, &mut |_| {}, &cancel)
        .unwrap_err();
    assert!(
        matches!(err, BiosError::Asset(AssetError::Cancelled)),
        "{err:?}"
    );
    assert!(listing(dir.path()).is_empty());

    let plain = catalogue("http://example.com/x.zip", &zip);
    let err = install(&manager, &plain, "192fr", ReplacePolicy::Refuse).unwrap_err();
    assert!(
        matches!(err, BiosError::Asset(AssetError::UrlRefused)),
        "{err:?}"
    );
    let mut hostile = catalogue(&server.url(), &zip);
    hostile.install_as = "../tos.img".into();
    let err = install(&manager, &hostile, "192fr", ReplacePolicy::Refuse).unwrap_err();
    assert!(matches!(err, BiosError::InvalidCatalogue(_)), "{err:?}");
    assert!(matches!(
        manager.state(&hostile),
        FreeBiosState::Unreadable(_)
    ));
}

#[test]
fn user_file_is_never_overwritten_without_consent() {
    let server = Server::start();
    let zip = fixture_zip();
    // Refused before any download; then one download for the backup-and-replace.
    server.serve(zip.clone(), 1);
    let dir = tempfile::tempdir().unwrap();
    let user_tos = b"TOS 1.02 dumped by the user";
    fs::write(dir.path().join("tos.img"), user_tos).unwrap();
    let bios = catalogue(&server.url(), &zip);
    let manager = FreeBiosManager::open(dir.path(), client()).unwrap();
    let user_md5: [u8; 16] = Md5::digest(user_tos).into();
    assert_eq!(
        manager.state(&bios),
        FreeBiosState::UserFile { md5: user_md5 }
    );

    let err = install(&manager, &bios, "192fr", ReplacePolicy::Refuse).unwrap_err();
    assert!(matches!(err, BiosError::UserFileExists(_)), "{err:?}");
    assert_eq!(fs::read(dir.path().join("tos.img")).unwrap(), user_tos);
    assert!(!manager.remove(&bios).unwrap());
    assert_eq!(fs::read(dir.path().join("tos.img")).unwrap(), user_tos);
    assert!(manager.installed_md5().is_empty());

    install(&manager, &bios, "192fr", ReplacePolicy::BackupAndReplace).unwrap();
    assert_eq!(fs::read(dir.path().join("tos.img")).unwrap(), IMAGE_FR);
    let backups: Vec<_> = listing(dir.path())
        .into_iter()
        .filter(|n| n.starts_with("tos.img.user-"))
        .collect();
    assert_eq!(backups.len(), 1);
    assert_eq!(fs::read(dir.path().join(&backups[0])).unwrap(), user_tos);
    server.verify();
}

#[test]
fn offline_install_from_archive() {
    let dir = tempfile::tempdir().unwrap();
    let system = dir.path().join("system");
    let zip = fixture_zip();
    let archive = dir.path().join("emutos-192k-1.4.zip");
    fs::write(&archive, &zip).unwrap();
    // URL never contacted.
    let bios = catalogue("https://emutos.invalid/emutos-192k-1.4.zip", &zip);
    let manager = FreeBiosManager::open(&system, client()).unwrap();
    manager
        .install_from_archive(&bios, "192us", &archive, ReplacePolicy::Refuse)
        .unwrap();
    assert_eq!(fs::read(system.join("tos.img")).unwrap(), IMAGE_US);

    let other = dir.path().join("other.zip");
    fs::write(
        &other,
        zip_of(&[("emutos-192k-1.4/etos192fr.img", IMAGE_FR)]),
    )
    .unwrap();
    let err = manager
        .install_from_archive(&bios, "192fr", &other, ReplacePolicy::Refuse)
        .unwrap_err();
    assert!(matches!(err, BiosError::HashMismatch(_)), "{err:?}");
    assert_eq!(fs::read(system.join("tos.img")).unwrap(), IMAGE_US);
    let err = manager
        .install_from_archive(
            &bios,
            "192fr",
            &dir.path().join("absent.zip"),
            ReplacePolicy::Refuse,
        )
        .unwrap_err();
    assert!(matches!(err, BiosError::Io { .. }), "{err:?}");
}

/// A file installed by us then replaced by the user's own dump is protected like any user file.
fn installed_then_replaced(dir: &Path, zip: &[u8], url: &str) -> (FreeBiosManager, FreeBios) {
    let archive = dir.join("a.zip");
    fs::write(&archive, zip).unwrap();
    let system = dir.join("system");
    let bios = catalogue(url, zip);
    let manager = FreeBiosManager::open(&system, client()).unwrap();
    manager
        .install_from_archive(&bios, "192fr", &archive, ReplacePolicy::Refuse)
        .unwrap();
    fs::write(system.join("tos.img"), b"user TOS 1.02").unwrap();
    assert_eq!(manager.state(&bios), FreeBiosState::Modified);
    (manager, bios)
}

#[test]
fn modified_file_with_refuse_fails_before_any_request() {
    let server = Server::start();
    let zip = fixture_zip();
    server.serve(zip.clone(), 0);
    let dir = tempfile::tempdir().unwrap();
    let (manager, bios) = installed_then_replaced(dir.path(), &zip, &server.url());
    let err = install(&manager, &bios, "192fr", ReplacePolicy::Refuse).unwrap_err();
    assert!(matches!(err, BiosError::UserFileExists(_)), "{err:?}");
    let tos = dir.path().join("system/tos.img");
    assert_eq!(fs::read(tos).unwrap(), b"user TOS 1.02");
    server.verify();
}

#[test]
fn modified_file_with_backup_is_kept_then_replaced() {
    let server = Server::start();
    let zip = fixture_zip();
    server.serve(zip.clone(), 1);
    let dir = tempfile::tempdir().unwrap();
    let (manager, bios) = installed_then_replaced(dir.path(), &zip, &server.url());
    let system = dir.path().join("system");
    install(&manager, &bios, "192fr", ReplacePolicy::BackupAndReplace).unwrap();
    assert_eq!(fs::read(system.join("tos.img")).unwrap(), IMAGE_FR);
    let backups: Vec<_> = listing(&system)
        .into_iter()
        .filter(|n| n.starts_with("tos.img.user-"))
        .collect();
    assert_eq!(backups.len(), 1);
    assert_eq!(
        fs::read(system.join(&backups[0])).unwrap(),
        b"user TOS 1.02"
    );
    server.verify();
}

#[test]
fn modified_and_remove() {
    let dir = tempfile::tempdir().unwrap();
    let zip = fixture_zip();
    let archive = dir.path().join("a.zip");
    fs::write(&archive, &zip).unwrap();
    let system = dir.path().join("system");
    let bios = catalogue("https://emutos.invalid/a.zip", &zip);
    let manager = FreeBiosManager::open(&system, client()).unwrap();
    manager
        .install_from_archive(&bios, "192fr", &archive, ReplacePolicy::Refuse)
        .unwrap();

    // Changed outside the application: possibly the user's own dump, so a new install must ask,
    // and `remove` leaves it alone.
    fs::write(system.join("tos.img"), b"patched").unwrap();
    assert_eq!(manager.state(&bios), FreeBiosState::Modified);
    assert!(manager.installed_md5().is_empty());
    assert!(!manager.remove(&bios).unwrap());
    assert_eq!(fs::read(system.join("tos.img")).unwrap(), b"patched");
    let err = manager
        .install_from_archive(&bios, "192fr", &archive, ReplacePolicy::Refuse)
        .unwrap_err();
    assert!(matches!(err, BiosError::UserFileExists(_)), "{err:?}");
    assert_eq!(fs::read(system.join("tos.img")).unwrap(), b"patched");
    manager
        .install_from_archive(&bios, "192fr", &archive, ReplacePolicy::BackupAndReplace)
        .unwrap();
    assert_eq!(fs::read(system.join("tos.img")).unwrap(), IMAGE_FR);

    assert!(manager.remove(&bios).unwrap());
    assert_eq!(manager.state(&bios), FreeBiosState::Missing);
    assert!(!system.join("tos.img").exists());
    assert!(!manager.remove(&bios).unwrap());
    assert!(manager.installed_md5().is_empty());

    // A corrupt manifest means "nothing is ours": the safe side.
    manager
        .install_from_archive(&bios, "192fr", &archive, ReplacePolicy::Refuse)
        .unwrap();
    fs::write(system.join(MANIFEST_FILE_NAME), "not = [toml").unwrap();
    assert!(matches!(
        manager.state(&bios),
        FreeBiosState::UserFile { .. }
    ));
    assert!(!manager.remove(&bios).unwrap());
    assert!(system.join("tos.img").exists());
}

#[test]
fn stale_temporaries_are_cleaned_on_open() {
    let dir = tempfile::tempdir().unwrap();
    let old = SystemTime::now() - Duration::from_secs(2 * 3600);
    let make = |name: &str, age: Option<SystemTime>| {
        let p = dir.path().join(name);
        fs::write(&p, b"x").unwrap();
        if let Some(t) = age {
            fs::File::options()
                .write(true)
                .open(&p)
                .unwrap()
                .set_modified(t)
                .unwrap();
        }
    };
    make(".tos.img.tmp-AbC123", Some(old));
    make(".emu-rust-bios.toml.tmp-zz99zz", Some(old));
    make(".tos.img.tmp-Recent", None);
    make("bios.tmp-AbC123", Some(old));
    make("tos.img", Some(old));
    let _manager = FreeBiosManager::open(dir.path(), client()).unwrap();
    assert_eq!(
        listing(dir.path()),
        [".tos.img.tmp-Recent", "bios.tmp-AbC123", "tos.img"]
    );
}

// --- SoundFont (`kind = "soundfont"`, `format = "raw"`, design v4.0 §3.3) ---

const SF2_PATH: &str = "/mrbumpy409/GeneralUser-GS/0123abcd/GeneralUser-GS.sf2";

/// Larger than the BIOS image limit (4 MiB): only the SoundFont limit lets it through.
fn sf2_body() -> Vec<u8> {
    let mut body = b"RIFF\0\0\0\0sfbk".to_vec();
    body.extend((0..5u32 << 20).map(|i| (i % 251) as u8));
    body
}

fn soundfont(url: &str, pinned: &[u8]) -> FreeBios {
    let size = pinned.len() as u64;
    let sha256: [u8; 32] = Sha256::digest(pinned).into();
    FreeBios {
        // The embedded catalogue's id: `installed_md5` must skip it.
        id: "generaluser-gs".into(),
        name: "GeneralUser GS".into(),
        version: "2.0.3".into(),
        license: "GeneralUser GS License v2.0".into(),
        homepage: "https://github.com/mrbumpy409/GeneralUser-GS".into(),
        system: "DOS".into(),
        install_as: "GeneralUser-GS.sf2".into(),
        archives: vec![BiosArchive {
            name: "GeneralUser-GS.sf2".into(),
            url: url.into(),
            size,
            sha256,
            format: ArchiveFormat::Raw,
        }],
        variants: vec![BiosVariant {
            id: "sf2".into(),
            label: "GeneralUser GS".into(),
            language: None,
            archive: "GeneralUser-GS.sf2".into(),
            entry: String::new(),
            size,
            sha256,
            md5: Md5::digest(pinned).into(),
        }],
        default_variant: "sf2".into(),
        kind: FreeBiosKind::SoundFont,
    }
}

fn serve_sf2(server: &Server, body: Vec<u8>, expected_calls: u64) -> String {
    server.mount(
        Mock::given(method("GET"))
            .and(path(SF2_PATH))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(body))
            .expect(expected_calls),
    );
    format!("{}{SF2_PATH}", server.server.uri())
}

#[test]
fn raw_soundfont_is_installed_as_is() {
    let server = Server::start();
    let body = sf2_body();
    let url = serve_sf2(&server, body.clone(), 1);
    let dir = tempfile::tempdir().unwrap();
    let system = dir.path().join("system");
    let sf = soundfont(&url, &body);
    let manager = FreeBiosManager::open(&system, client()).unwrap();
    assert_eq!(manager.state(&sf), FreeBiosState::Missing);
    let target = install(&manager, &sf, "sf2", ReplacePolicy::Refuse).unwrap();
    assert_eq!(target, system.join("GeneralUser-GS.sf2"));
    assert_eq!(fs::read(&target).unwrap(), body);
    assert_eq!(listing(&system), [MANIFEST_FILE_NAME, "GeneralUser-GS.sf2"]);
    assert_eq!(
        manager.state(&sf),
        FreeBiosState::Installed {
            variant: "sf2".into()
        }
    );
    // Not a firmware: never reported to `check_firmware`.
    assert!(manager.installed_md5().is_empty());
    assert!(manager.remove(&sf).unwrap());
    assert_eq!(listing(&system), [MANIFEST_FILE_NAME]);
    server.verify();
}

#[test]
fn raw_soundfont_with_a_wrong_hash_writes_nothing() {
    let server = Server::start();
    let mut body = sf2_body();
    let sf = soundfont("", &body);
    *body.last_mut().unwrap() ^= 0xff;
    let url = serve_sf2(&server, body, 1);
    let sf = FreeBios {
        archives: vec![BiosArchive {
            url,
            ..sf.archives[0].clone()
        }],
        ..sf
    };
    let dir = tempfile::tempdir().unwrap();
    let system = dir.path().join("system");
    let manager = FreeBiosManager::open(&system, client()).unwrap();
    let err = install(&manager, &sf, "sf2", ReplacePolicy::Refuse).unwrap_err();
    assert!(
        matches!(err, BiosError::HashMismatch(ref n) if n == "GeneralUser-GS.sf2"),
        "{err}"
    );
    assert!(listing(&system).is_empty());
    server.verify();
}

#[test]
fn raw_soundfont_longer_than_pinned_is_cut_off() {
    let server = Server::start();
    let body = sf2_body();
    let mut served = body.clone();
    served.extend_from_slice(b"trailing");
    let url = serve_sf2(&server, served, 1);
    let dir = tempfile::tempdir().unwrap();
    let system = dir.path().join("system");
    let manager = FreeBiosManager::open(&system, client()).unwrap();
    let err = install(
        &manager,
        &soundfont(&url, &body),
        "sf2",
        ReplacePolicy::Refuse,
    )
    .unwrap_err();
    assert!(
        matches!(err, BiosError::Asset(AssetError::TooLarge { limit }) if limit == body.len() as u64),
        "{err}"
    );
    assert!(listing(&system).is_empty());
    server.verify();
}

#[test]
fn soundfont_sized_file_declared_as_a_bios_is_refused_before_any_request() {
    let server = Server::start();
    let body = sf2_body();
    let url = serve_sf2(&server, body.clone(), 0);
    let dir = tempfile::tempdir().unwrap();
    let system = dir.path().join("system");
    let manager = FreeBiosManager::open(&system, client()).unwrap();
    let as_bios = FreeBios {
        kind: FreeBiosKind::Bios,
        ..soundfont(&url, &body)
    };
    let err = install(&manager, &as_bios, "sf2", ReplacePolicy::Refuse).unwrap_err();
    assert!(matches!(err, BiosError::InvalidCatalogue(_)), "{err}");
    // An empty entry outside `raw` is refused too.
    let zip_without_entry = soundfont(&url, b"small");
    let zip_without_entry = FreeBios {
        archives: vec![BiosArchive {
            format: ArchiveFormat::Zip,
            ..zip_without_entry.archives[0].clone()
        }],
        ..zip_without_entry
    };
    let err = install(&manager, &zip_without_entry, "sf2", ReplacePolicy::Refuse).unwrap_err();
    assert!(matches!(err, BiosError::InvalidCatalogue(_)), "{err}");
    assert_eq!(
        manager.state(&as_bios),
        FreeBiosState::Unreadable("catalogue invalide".into())
    );
    assert!(listing(&system).is_empty());
    server.verify();
}

#[test]
fn raw_soundfont_offline_install_and_user_file_kept() {
    let dir = tempfile::tempdir().unwrap();
    let system = dir.path().join("system");
    let body = sf2_body();
    let src = dir.path().join("downloaded.sf2");
    fs::write(&src, &body).unwrap();
    let sf = soundfont("https://example.invalid/GeneralUser-GS.sf2", &body);
    let manager = FreeBiosManager::open(&system, client()).unwrap();
    // A SoundFont the user placed there himself is never overwritten without consent.
    fs::write(system.join("GeneralUser-GS.sf2"), b"user sf2").unwrap();
    assert!(matches!(
        manager.install_from_archive(&sf, "sf2", &src, ReplacePolicy::Refuse),
        Err(BiosError::UserFileExists(_))
    ));
    assert_eq!(
        fs::read(system.join("GeneralUser-GS.sf2")).unwrap(),
        b"user sf2"
    );
    let target = manager
        .install_from_archive(&sf, "sf2", &src, ReplacePolicy::BackupAndReplace)
        .unwrap();
    assert_eq!(fs::read(&target).unwrap(), body);
    assert!(
        listing(&system)
            .iter()
            .any(|n| n.starts_with("GeneralUser-GS.sf2.user-"))
    );
    // A wrong local file: nothing written.
    let other = dir.path().join("other.sf2");
    fs::write(&other, b"not the pinned file").unwrap();
    assert!(matches!(
        manager.install_from_archive(&sf, "sf2", &other, ReplacePolicy::BackupAndReplace),
        Err(BiosError::HashMismatch(_))
    ));
    assert_eq!(fs::read(&target).unwrap(), body);
}
