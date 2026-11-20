use super::*;
use std::collections::VecDeque;

#[test]
fn thumbnail_names_follow_libretro_rules() {
    assert_eq!(thumbnail_file_name("Sonic (USA)"), "Sonic (USA).png");
    assert_eq!(
        thumbnail_file_name(r#"A&B*C/D:E`F<G>H?I\J|K"L"#),
        "A_B_C_D_E_F_G_H_I_J_K_L.png"
    );
}

#[test]
fn libretro_url_is_encoded() {
    let client = HttpClient::new(crate::http::HttpConfig::default()).unwrap();
    let source = LibretroThumbnails::new(client, "https://t.test");
    let url = source.url(&query("Bob's Game: #1 (USA)"));
    assert_eq!(
        url,
        "https://t.test/Nintendo%20-%20Game%20Boy/Named_Boxarts/Bob%27s%20Game_%20%231%20%28USA%29.png"
    );
}

#[test]
fn igdb_titles() {
    assert_eq!(igdb_search_title("Sonic (USA) [b]"), "Sonic");
    assert_eq!(igdb_search_title("Foo [!] (Europe)"), "Foo");
    assert_eq!(igdb_search_title("Say \"hi\"\\\n"), "Say hi");
    assert_eq!(igdb_search_title(" (USA)"), "");
    assert!(valid_image_id("co1abc"));
    assert!(!valid_image_id("../x"));
    assert!(!valid_image_id(""));
}

#[test]
fn resolve_refuses_escapes() {
    let cache = CoverCache::new(Path::new("/data/covers"));
    assert_eq!(
        cache.resolve(Path::new("gb/boxart/ab.png")),
        Some(PathBuf::from("/data/covers/gb/boxart/ab.png"))
    );
    assert_eq!(cache.resolve(Path::new("../x")), None);
    assert_eq!(cache.resolve(Path::new("gb/../../x")), None);
    assert_eq!(cache.resolve(Path::new("/etc/passwd")), None);
    assert_eq!(cache.resolve(Path::new("./gb/x.png")), None);
    assert_eq!(cache.resolve(Path::new("")), None);
}

const PNG: &[u8] = include_bytes!("../../../../tests/data/covers/images/valid.png");

fn found(bytes: &[u8]) -> CoverFound {
    CoverFound {
        source: CoverSourceKind::LibretroThumbnails,
        format: ImageFormat::Png,
        bytes: bytes.to_vec(),
    }
}

#[test]
fn store_is_atomic_and_validated() {
    let dir = tempfile::tempdir().unwrap();
    let cache = CoverCache::new(dir.path());
    let rel = cache.store("gb", "00ff", &found(PNG)).unwrap();
    assert_eq!(rel, Path::new("gb/boxart/00ff.png"));
    let path = cache.resolve(&rel).unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), PNG);
    let leftovers: Vec<_> = std::fs::read_dir(path.parent().unwrap())
        .unwrap()
        .filter_map(Result::ok)
        .filter(|e| e.file_name().to_string_lossy().contains(".tmp-"))
        .collect();
    assert!(leftovers.is_empty());

    assert!(cache.store("../x", "k", &found(PNG)).is_err());
    assert!(cache.store("gb", "a/b", &found(PNG)).is_err());
    assert!(cache.store("gb", "", &found(PNG)).is_err());
    let mut wrong = found(PNG);
    wrong.format = ImageFormat::Jpeg;
    assert!(matches!(
        cache.store("gb", "k", &wrong),
        Err(AssetError::InvalidImage(_))
    ));
}

#[test]
fn cache_is_bounded() {
    let dir = tempfile::tempdir().unwrap();
    let cache = CoverCache::with_limit(dir.path(), PNG.len() as u64 * 3);
    for i in 0..10 {
        cache.store("nes", &format!("k{i}"), &found(PNG)).unwrap();
    }
    let total: u64 = cache.files().iter().map(|f| f.1).sum();
    assert!(total <= PNG.len() as u64 * 3, "{total}");
    // The last one written is always kept.
    assert!(dir.path().join("nes/boxart/k9.png").exists());
}

fn query(title: &str) -> CoverQuery {
    CoverQuery {
        title: title.into(),
        identified: true,
        file_name: format!("{title}.gb"),
        size: 32768,
        crc32: Some(0x1234_ABCD),
        sha1: Some([0xAB; 20]),
        database: "Nintendo - Game Boy".into(),
        screenscraper_system: Some(9),
        igdb_platform: Some(33),
    }
}

/// Answers from a script, then "not found".
struct Fake {
    kind: CoverSourceKind,
    answers: VecDeque<Result<Option<CoverFound>, AssetError>>,
    calls: Arc<Mutex<u32>>,
}

impl CoverSource for Fake {
    fn kind(&self) -> CoverSourceKind {
        self.kind
    }
    fn fetch(&mut self, _: &CoverQuery) -> Result<Option<CoverFound>, AssetError> {
        *self.calls.lock().unwrap() += 1;
        self.answers.pop_front().unwrap_or(Ok(None))
    }
}

fn fake(
    kind: CoverSourceKind,
    answers: Vec<Result<Option<CoverFound>, AssetError>>,
) -> (Box<dyn CoverSource>, Arc<Mutex<u32>>) {
    let calls = Arc::new(Mutex::new(0));
    let source = Fake {
        kind,
        answers: answers.into(),
        calls: calls.clone(),
    };
    (Box::new(source), calls)
}

fn net() -> AssetError {
    AssetError::Network("x".into())
}

#[test]
fn chain_falls_back_in_order() {
    let mut ss_found = found(PNG);
    ss_found.source = CoverSourceKind::ScreenScraper;
    let (a, a_calls) = fake(CoverSourceKind::LibretroThumbnails, vec![Ok(None)]);
    let (b, _) = fake(CoverSourceKind::ScreenScraper, vec![Ok(Some(ss_found))]);
    let (c, c_calls) = fake(CoverSourceKind::Igdb, vec![]);
    let mut chain = CoverChain::new(vec![a, b, c]);
    let got = chain.fetch(&query("x")).unwrap().unwrap();
    assert_eq!(got.source, CoverSourceKind::ScreenScraper);
    assert_eq!((*a_calls.lock().unwrap(), *c_calls.lock().unwrap()), (1, 0));
    // Everybody says "not found": conclusive.
    assert_eq!(chain.fetch(&query("x")).unwrap(), None);
}

#[test]
fn chain_goes_offline_and_never_concludes_on_errors() {
    let (a, _) = fake(
        CoverSourceKind::LibretroThumbnails,
        vec![Err(net()), Err(net()), Err(net())],
    );
    let (b, _) = fake(
        CoverSourceKind::Igdb,
        vec![
            Err(net()),
            Err(AssetError::Http { status: 503 }),
            Err(net()),
        ],
    );
    let mut chain = CoverChain::new(vec![a, b]);
    for _ in 0..2 {
        assert!(chain.fetch(&query("x")).is_err());
        assert!(!chain.offline());
    }
    assert!(matches!(
        chain.fetch(&query("x")),
        Err(AssetError::Network(_))
    ));
    assert!(chain.offline());
    assert!(
        chain.fetch(&query("x")).is_err(),
        "offline: still not conclusive"
    );
    chain.reset();
    assert!(!chain.offline());
    assert_eq!(chain.fetch(&query("x")).unwrap(), None);
}

#[test]
fn chain_refused_and_quota() {
    let (a, a_calls) = fake(
        CoverSourceKind::ScreenScraper,
        vec![Err(AssetError::Unauthorized)],
    );
    let (b, _) = fake(CoverSourceKind::LibretroThumbnails, vec![]);
    let mut chain = CoverChain::new(vec![a, b]);
    // Refused credentials do not prevent a conclusive answer from the others.
    assert_eq!(chain.fetch(&query("x")).unwrap(), None);
    assert_eq!(chain.fetch(&query("x")).unwrap(), None);
    assert_eq!(*a_calls.lock().unwrap(), 1, "disabled for the session");

    let (q, q_calls) = fake(
        CoverSourceKind::ScreenScraper,
        vec![Err(AssetError::Quota { retry_after: None })],
    );
    let (b, _) = fake(CoverSourceKind::LibretroThumbnails, vec![]);
    let mut chain = CoverChain::new(vec![q, b]);
    for _ in 0..3 {
        assert!(matches!(
            chain.fetch(&query("x")),
            Err(AssetError::Quota { .. })
        ));
    }
    assert_eq!(*q_calls.lock().unwrap(), 1);
    assert!(!chain.offline());

    let (s, s_calls) = fake(
        CoverSourceKind::Igdb,
        vec![Err(AssetError::Quota {
            retry_after: Some(Duration::from_millis(50)),
        })],
    );
    let mut chain = CoverChain::new(vec![s]);
    assert!(chain.fetch(&query("x")).is_err());
    assert!(chain.fetch(&query("x")).is_err(), "suspended");
    std::thread::sleep(Duration::from_millis(60));
    assert_eq!(chain.fetch(&query("x")).unwrap(), None);
    assert_eq!(*s_calls.lock().unwrap(), 2);
}

#[test]
fn chain_edge_cases() {
    assert!(matches!(
        CoverChain::new(vec![]).fetch(&query("x")),
        Err(AssetError::NoSource)
    ));
    assert!(!CoverChain::new(vec![]).offline());
    let (a, _) = fake(CoverSourceKind::Igdb, vec![Err(AssetError::Unauthorized)]);
    let mut chain = CoverChain::new(vec![a]);
    assert!(matches!(
        chain.fetch(&query("x")),
        Err(AssetError::NoSource)
    ));
    // Unusable content counts as "not found".
    let (b, _) = fake(
        CoverSourceKind::Igdb,
        vec![Err(AssetError::InvalidImage("x".into()))],
    );
    assert_eq!(CoverChain::new(vec![b]).fetch(&query("x")).unwrap(), None);
    let (c, _) = fake(CoverSourceKind::Igdb, vec![Err(AssetError::Cancelled)]);
    assert!(matches!(
        CoverChain::new(vec![c]).fetch(&query("x")),
        Err(AssetError::Cancelled)
    ));
}

#[test]
fn cache_counts_rewrites_once_and_skips_temporaries_and_links() {
    let dir = tempfile::tempdir().unwrap();
    let cache = CoverCache::with_limit(dir.path(), PNG.len() as u64 * 3);
    for _ in 0..10 {
        cache.store("gb", "same", &found(PNG)).unwrap();
    }
    assert_eq!(*cache.used.lock().unwrap(), Some(PNG.len() as u64));
    let boxart = dir.path().join("gb").join(BOXART);
    std::fs::write(boxart.join(".same.png.tmp-abc"), vec![0; 1000]).unwrap();
    assert_eq!(cache.files().len(), 1);
    #[cfg(unix)]
    {
        // A link to a file or a directory outside the cache is neither counted nor pruned.
        let outside = tempfile::tempdir().unwrap();
        let victim = outside.path().join("victim.png");
        std::fs::write(&victim, vec![0; 10_000]).unwrap();
        std::os::unix::fs::symlink(&victim, boxart.join("link.png")).unwrap();
        std::fs::create_dir_all(outside.path().join(BOXART)).unwrap();
        std::fs::write(outside.path().join(BOXART).join("x.png"), vec![0; 10_000]).unwrap();
        std::os::unix::fs::symlink(outside.path(), dir.path().join("linked")).unwrap();
        assert_eq!(cache.files().len(), 1);
        let fresh = CoverCache::with_limit(dir.path(), PNG.len() as u64 * 3);
        for i in 0..5 {
            fresh.store("gb", &format!("k{i}"), &found(PNG)).unwrap();
        }
        assert!(victim.exists());
        assert!(outside.path().join(BOXART).join("x.png").exists());
    }
}

#[test]
fn prune_removes_stale_temporaries_only() {
    let dir = tempfile::tempdir().unwrap();
    let cache = CoverCache::with_limit(dir.path(), PNG.len() as u64 * 3);
    cache.store("gb", "first", &found(PNG)).unwrap();
    let boxart = dir.path().join("gb").join(BOXART);
    let stale = boxart.join(".old.png.tmp-abc");
    let fresh = boxart.join(".new.png.tmp-def");
    std::fs::write(&stale, b"x").unwrap();
    std::fs::write(&fresh, b"x").unwrap();
    std::fs::File::options()
        .write(true)
        .open(&stale)
        .unwrap()
        .set_modified(SystemTime::now() - Duration::from_secs(2 * 3600))
        .unwrap();
    // Past the limit: pruning runs.
    for i in 0..4 {
        cache.store("gb", &format!("k{i}"), &found(PNG)).unwrap();
    }
    assert!(!stale.exists());
    assert!(fresh.exists());
}

fn url(s: &str) -> Url {
    Url::parse(s).unwrap()
}

#[test]
fn screenscraper_media_hosts() {
    let api = url(ScreenScraperSource::BASE_URL);
    for ok in [
        "https://api.screenscraper.fr/api2/mediaJeu.php?x=1",
        "https://neoclone.screenscraper.fr/api2/mediaJeu.php",
        "https://NEOCLONE.screenscraper.fr/m.png",
    ] {
        assert!(media_allowed(&api, &url(ok)), "{ok}");
    }
    for refused in [
        "http://neoclone.screenscraper.fr/m.png",
        "http://api.screenscraper.fr/m.png",
        "https://screenscraper.fr.evil.example/m.png",
        "https://xscreenscraper.fr/m.png",
        "https://evil.example/m.png",
        "ftp://api.screenscraper.fr/m.png",
    ] {
        assert!(!media_allowed(&api, &url(refused)), "{refused}");
    }
    // No public suffix list: `co.uk`-like parents grant no sibling.
    assert!(!media_allowed(
        &url("https://x.co.uk/"),
        &url("https://y.co.uk/m.png")
    ));
    assert!(!media_allowed(
        &url("https://a.com.au/"),
        &url("https://b.com.au/m.png")
    ));
    assert!(media_allowed(
        &url("https://api.foo.co.uk/"),
        &url("https://img.foo.co.uk/m.png")
    ));
    // IP bases: same host only; http only on the same host and port as an http base (loopback tests).
    let local = url("http://127.0.0.1:8080/api2/");
    assert!(media_allowed(&local, &url("http://127.0.0.1:8080/m.png")));
    assert!(!media_allowed(&local, &url("http://127.0.0.1:9090/m.png")));
    assert!(!media_allowed(&local, &url("http://127.0.0.2:8080/m.png")));
}

#[test]
fn credentials_debug_says_only_set_or_not() {
    let ss = ScreenScraperCredentials {
        dev_id: "devid-xyz".into(),
        dev_password: Secret::new("devpass-xyz"),
        user: String::new(),
        password: Secret::new("pass-xyz"),
    };
    let text = format!("{ss:?} {ss:#?}");
    assert!(!text.contains("xyz"), "{text}");
    assert!(text.contains("non défini") && text.contains("défini"));
    let igdb = IgdbCredentials {
        client_id: "id-xyz".into(),
        client_secret: Secret::new("secret-xyz"),
    };
    let text = format!("{igdb:?}");
    assert!(!text.contains("xyz"), "{text}");
}
