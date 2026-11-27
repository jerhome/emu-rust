//! Custom systems in the settings (design 8.0 §0.9, §0.12, ADR 0025 §9, §10, §13) with the process-wide catalog:
//! deleting, restoring and forgetting a system, the publication that waits for a scan, the games of an id in
//! conflict, a system just added. Each test publishes its own catalog and has its own data folder; one at a time.

use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{Mutex, MutexGuard};
use std::time::Duration;

use emu_ui::library_db::{
    Catalog, CustomSystemDraft, DatSource, DefinitionOrigin, Library, ScanOptions, system,
    write_custom,
};
use emu_ui::test_support::{DialogKind, LibraryHarness, hold_catalog};
use emu_ui::{Config, DataDirs};

static LOCK: Mutex<()> = Mutex::new(());

fn lock() -> MutexGuard<'static, ()> {
    LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

struct Root {
    _tmp: tempfile::TempDir,
    dirs: DataDirs,
}

fn root() -> Root {
    let tmp = tempfile::tempdir().unwrap();
    // `/var` is a link on macOS: the data folder is compared with canonical paths.
    let base = tmp.path().canonicalize().unwrap();
    let dirs = DataDirs::under(&base);
    dirs.create_all().unwrap();
    Root { _tmp: tmp, dirs }
}

fn draft(id: &str, database: Option<&str>, extensions: &[&str]) -> CustomSystemDraft {
    CustomSystemDraft {
        id: id.into(),
        name: format!("System {id}"),
        database: database.map(str::to_owned),
        extensions: extensions.iter().map(|e| (*e).to_owned()).collect(),
        dats: vec![DatSource::NoIntro],
        screenscraper: None,
        igdb: None,
        cores: vec!["fic_core".into()],
        ignored_firmware: Vec::new(),
        origin: DefinitionOrigin {
            core: "fic_core".into(),
            info_name: None,
            created_by: "8.0.0".into(),
            created_unix: 1_764_028_800,
        },
    }
}

/// `<roms>/<folder>/<file>` with some bytes, folders made.
fn game(root: &Root, folder: &str, file: &str, bytes: &[u8]) -> PathBuf {
    let dir = root.dirs.root.join("roms").join(folder);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(file);
    std::fs::write(&path, bytes).unwrap();
    path
}

fn scan(root: &Root) {
    let (mut db, _) = Library::open(&root.dirs.library_db).unwrap();
    let options = ScanOptions {
        roots: vec![root.dirs.root.join("roms")],
        ..ScanOptions::default()
    };
    db.scan(&options, &AtomicBool::new(false), &mut |_| {})
        .unwrap();
}

fn harness(root: &Root) -> LibraryHarness {
    LibraryHarness::open(root.dirs.clone(), Config::default())
}

const WAIT: Duration = Duration::from_secs(10);

#[test]
fn deleting_hides_the_games_restoring_brings_them_back_and_forgetting_removes_them() {
    let _guard = lock();
    let root = root();
    write_custom(
        &root.dirs.custom_systems,
        &draft("zzfic", Some("Zz - Fictional"), &["fic"]),
        &Catalog::official(),
    )
    .unwrap();
    emu_ui::publish_catalog(&root.dirs);
    game(&root, "zzfic", "Fic Game.fic", b"rom");
    scan(&root);
    let mut h = harness(&root);
    assert_eq!(h.games(), ["Fic Game"]);

    // « Supprimer »: confirmed, the definition is retired, the catalog is published, the game is hidden.
    assert_eq!(h.ask_delete("zzfic"), DialogKind::Delete);
    h.confirm(true);
    assert!(h.publication_waits());
    h.wait_publication(WAIT);
    assert!(system("zzfic").is_none());
    assert!(h.games().is_empty(), "hidden, not forgotten");
    assert!(root.dirs.custom_systems.join(".retired").is_dir());
    assert_eq!(h.retired(), [("zzfic".to_owned(), true, true)]);

    // « Restaurer »: the system and its game are back, with the same row.
    h.restore("zzfic");
    h.wait_publication(WAIT);
    assert!(system("zzfic").is_some());
    assert_eq!(h.games(), ["Fic Game"]);

    // Deleted again, then « Oublier ses jeux »: the rows are gone for good, restoring finds none.
    h.ask_delete("zzfic");
    h.confirm(true);
    h.wait_publication(WAIT);
    assert_eq!(h.ask_forget("zzfic"), DialogKind::Forget);
    h.confirm(true);
    h.restore("zzfic");
    h.wait_publication(WAIT);
    assert!(system("zzfic").is_some());
    assert!(h.games().is_empty());
}

#[test]
fn a_replaced_definition_is_listed_without_restore_and_forgetting_an_active_id_is_offered_for_its_frozen_rows()
 {
    let _guard = lock();
    let root = root();
    let retired = root.dirs.custom_systems.join(".retired");
    std::fs::create_dir_all(&retired).unwrap();
    let definition = |id: &str| {
        format!(
            "format = 1\nrender = \"software\"\n[system]\nid = \"{id}\"\nname = \"Mine\"\n\
             database = \"Nintendo - Super Nintendo Entertainment System\"\nextensions = [\"sfc\"]\n\
             [cores]\nlist = [\"fic_core\"]\n[origin]\ncore = \"fic_core\"\ncreated_by = \"t\"\ncreated_unix = 1\n"
        )
    };
    std::fs::write(retired.join("mysnes-5.replaced.toml"), definition("mysnes")).unwrap();
    std::fs::write(retired.join("gone-6.toml"), definition("gone")).unwrap();
    // A definition removed from an id an official system uses.
    std::fs::write(retired.join("snes-7.toml"), definition("snes")).unwrap();
    emu_ui::publish_catalog(&root.dirs);
    let mut h = harness(&root);
    let rows = h.retired();
    let find = |id: &str| rows.iter().find(|r| r.0 == id).cloned().unwrap();
    assert_eq!(
        find("mysnes"),
        ("mysnes".to_owned(), false, true),
        "replaced: no restore, but its games can be forgotten (ADR 0025 §10)"
    );
    assert_eq!(find("gone"), ("gone".to_owned(), true, true));
    assert_eq!(
        find("snes"),
        ("snes".to_owned(), true, true),
        "offered, but only the frozen rows of that id go (its own rows are the official system's)"
    );
    assert_eq!(h.ask_forget("snes"), DialogKind::Forget);
    // Restoring a replaced one is refused even if asked.
    h.restore("mysnes");
    assert!(!root.dirs.custom_systems.join("mysnes.toml").exists());
}

#[test]
fn a_change_made_during_a_scan_is_published_when_the_scan_ends() {
    let _guard = lock();
    let root = root();
    write_custom(
        &root.dirs.custom_systems,
        &draft("zzwait", Some("Zz - Waiting"), &["wai"]),
        &Catalog::official(),
    )
    .unwrap();
    emu_ui::publish_catalog(&root.dirs);
    let mut h = harness(&root);
    let hold = hold_catalog();
    h.ask_delete("zzwait");
    h.confirm(true);
    for _ in 0..5 {
        h.frame();
    }
    assert!(
        h.publication_waits(),
        "kept due while a scan reads the catalog"
    );
    assert!(
        system("zzwait").is_some(),
        "nothing was published under the scan"
    );
    drop(hold);
    h.wait_publication(WAIT);
    assert!(system("zzwait").is_none());
}

#[test]
fn the_games_of_an_id_in_conflict_that_the_definition_does_not_accept_are_hidden_and_survive() {
    let _guard = lock();
    let root = root();
    // Official catalog first: the Super Nintendo game is attributed to `snes`.
    emu_ui::publish_catalog(&root.dirs);
    game(&root, "snes", "Official.sfc", &[1; 600]);
    scan(&root);
    // Then a definition with the id of that official system, another database and another extension.
    std::fs::create_dir_all(&root.dirs.custom_systems).unwrap();
    std::fs::write(
        root.dirs.custom_systems.join("snes.toml"),
        "format = 1\nrender = \"software\"\n[system]\nid = \"snes\"\nname = \"Not a SNES\"\n\
         database = \"Zz - Not A Super Nintendo\"\nextensions = [\"xyz\"]\n[cores]\nlist = [\"fic_core\"]\n\
         [origin]\ncore = \"fic_core\"\ncreated_by = \"t\"\ncreated_unix = 1\n",
    )
    .unwrap();
    let load = emu_ui::publish_catalog(&root.dirs);
    assert!(load.problems.is_empty(), "{:?}", load.problems);
    let claimed = game(&root, "snes", "Claimed.xyz", b"x");
    scan(&root);
    let mut h = harness(&root);
    assert_eq!(
        h.games(),
        ["Claimed"],
        "the official game is frozen, hence hidden"
    );

    // Deleting that system asks to forget its games; one is still on disk: refused, listed, nothing retired.
    assert_eq!(h.ask_delete("snes"), DialogKind::DeleteUncertain);
    h.confirm(true);
    assert_eq!(h.dialog(), DialogKind::Blocked);
    h.confirm(false);
    assert!(root.dirs.custom_systems.join("snes.toml").is_file());
    assert!(!h.publication_waits());

    // The file moved away: the claimed row is forgotten, the official game is kept and shown again.
    std::fs::remove_file(&claimed).unwrap();
    assert_eq!(h.ask_delete("snes"), DialogKind::DeleteUncertain);
    h.confirm(true);
    assert_eq!(h.dialog(), DialogKind::None);
    h.wait_publication(WAIT);
    assert!(!root.dirs.custom_systems.join("snes.toml").exists());
    assert_eq!(
        h.games(),
        ["Official"],
        "its data is intact, the claimed row is gone"
    );
}

#[test]
fn a_system_just_added_gets_its_folder_and_a_notice_once_the_catalog_is_published() {
    let _guard = lock();
    let root = root();
    emu_ui::publish_catalog(&root.dirs);
    let mut h = harness(&root);
    h.add_systems(vec![draft("zzadded", Some("Zz - Added"), &["add"])]);
    assert!(h.publication_waits());
    h.wait_publication(WAIT);
    assert!(system("zzadded").is_some_and(|s| s.is_custom()));
    assert!(root.dirs.root.join("roms").join("zzadded").is_dir());
    assert_eq!(h.dialog(), DialogKind::Added);
    // A refused definition (this id is an official one) writes nothing and says so.
    let mut h = harness(&root);
    h.add_systems(vec![draft("snes", Some("Zz - Other"), &["xyz"])]);
    assert!(!root.dirs.custom_systems.join("snes.toml").exists());
    assert!(h.status().is_some());
}

/// A switch of `mysnes` to the official `snes` whose end is due (journal under `<data>/rangement`); the definition
/// is left where it was.
fn open_switch(root: &Root) {
    use emu_ui::library_db::{KeysChoice, SwitchChoices, SwitchMove, TidyOptions};
    let mut mine = draft(
        "mysnes",
        Some("Nintendo - Super Nintendo Entertainment System"),
        &["sfc"],
    );
    mine.dats = Vec::new();
    write_custom(&root.dirs.custom_systems, &mine, &Catalog::official()).unwrap();
    emu_ui::publish_catalog(&root.dirs);
    game(root, "mysnes", "Game.sfc", &[7; 3000]);
    scan(root);
    let (mut db, _) = Library::open(&root.dirs.library_db).unwrap();
    let options = TidyOptions::new(&root.dirs.root.join("roms"), &root.dirs.tidy_journals)
        .unwrap()
        .with_systems_dir(&root.dirs.custom_systems);
    let switch = SwitchMove {
        custom_id: "mysnes".into(),
        official_id: "snes".into(),
    };
    let cancel = AtomicBool::new(false);
    let plan = db
        .plan_switch(
            &options,
            &Catalog::official(),
            &switch,
            &cancel,
            &mut |_| {},
        )
        .unwrap();
    let choices = SwitchChoices {
        keys: KeysChoice::KeepUser,
        prior_preference: None,
        prior_profile: None,
    };
    db.apply_switch(&options, &plan, &choices, &cancel, &mut |_| {})
        .unwrap();
}

#[test]
fn forgetting_the_games_is_refused_while_a_switch_of_that_id_is_open() {
    let _guard = lock();
    let root = root();
    open_switch(&root);
    // The definition is retired (crash between the retirement and the end): its end is still due.
    let retired = root.dirs.custom_systems.join(".retired");
    std::fs::create_dir_all(&retired).unwrap();
    std::fs::rename(
        root.dirs.custom_systems.join("mysnes.toml"),
        retired.join("mysnes-1.toml"),
    )
    .unwrap();
    let mut h = harness(&root);
    assert_eq!(h.ask_forget("mysnes"), DialogKind::Forget);
    h.confirm(true);
    let status = h.status().unwrap();
    assert!(status.contains("bascule"), "{status}");
    assert!(
        !status.contains("oubli"),
        "nothing may be forgotten: {status}"
    );
}

#[test]
fn every_id_is_checked_before_the_first_definition_is_written() {
    let _guard = lock();
    let root = root();
    open_switch(&root);
    let mut h = harness(&root);
    h.add_systems(vec![
        draft("zzfic", Some("Zz - Fictional"), &["fic"]),
        draft("mysnes", None, &["xyz"]),
    ]);
    assert!(
        !root.dirs.custom_systems.join("zzfic.toml").exists(),
        "nothing written when a later id is refused"
    );
    assert!(h.status().unwrap().contains("bascule"));
}

fn mock_runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
        .unwrap()
}

#[test]
fn the_database_search_imports_only_what_it_found_and_asks_three_times_at_most() {
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    let _guard = lock();
    let root = root();
    let mut def = draft("zzfic", Some("Zz - Fictional"), &["fic"]);
    def.dats = Vec::new();
    write_custom(&root.dirs.custom_systems, &def, &Catalog::official()).unwrap();
    emu_ui::publish_catalog(&root.dirs);
    let rt = mock_runtime();
    let server = rt.block_on(MockServer::start());
    let dat = std::fs::read(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/data/dat/sample.dat"),
    )
    .unwrap();
    rt.block_on(
        Mock::given(method("GET"))
            .and(path("/metadat/tosec/Zz%20-%20Fictional.dat"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(dat))
            .mount(&server),
    );
    rt.block_on(
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(404))
            .with_priority(5)
            .mount(&server),
    );
    let mut h = harness(&root);
    h.set_dat_source(&format!("{}/", server.uri()));
    h.search_database("zzfic");
    h.wait_dat_work(WAIT);

    let requests = rt.block_on(server.received_requests()).unwrap();
    assert!(requests.len() <= 3, "{} requests", requests.len());
    assert!(
        requests
            .iter()
            .all(|r| r.url.path().ends_with("Zz%20-%20Fictional.dat")),
        "no request for another system: {requests:?}"
    );
    let catalog = Catalog::load(&root.dirs.custom_systems).catalog;
    assert_eq!(catalog.system("zzfic").unwrap().dats, [DatSource::Tosec]);
    let (db, _) = Library::open(&root.dirs.library_db).unwrap();
    assert!(
        db.dat_info_for("zzfic", DatSource::Tosec)
            .unwrap()
            .is_some()
    );
}
