//! Helpers shared by `tidy_apply.rs` and `tidy_undo.rs` (not a test crate by itself). Every tree is generated in a
//! temporary folder; Python 3 is only needed for the big generated tree (`tests/tools/mktidy.py`).
#![allow(dead_code)]

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::AtomicBool;

use emu_library::{
    Game, GameId, Library, ScanOptions, TidyOptions, TidyPlan, TidyReport, TidyStatus, TidyUnit,
};

pub fn never() -> AtomicBool {
    AtomicBool::new(false)
}

pub fn write(path: &Path, data: &[u8]) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, data).unwrap();
}

/// Deterministic incompressible-ish bytes (no header: the extension alone decides the system).
pub fn noise(len: usize, seed: u32) -> Vec<u8> {
    let mut x = seed.wrapping_mul(2_654_435_761).wrapping_add(1);
    (0..len)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            (x >> 24) as u8
        })
        .collect()
}

/// A temporary `roms/` root with a sibling journal folder.
pub struct Fx {
    pub tmp: tempfile::TempDir,
    pub roms: PathBuf,
    pub journals: PathBuf,
}

impl Fx {
    pub fn new() -> Self {
        let tmp = tempfile::tempdir().unwrap();
        let roms = tmp.path().join("roms");
        fs::create_dir(&roms).unwrap();
        let journals = tmp.path().join("rangement");
        Fx {
            tmp,
            roms,
            journals,
        }
    }

    pub fn options(&self) -> TidyOptions {
        TidyOptions::new(&self.roms, &self.journals).unwrap()
    }

    pub fn plan(&self, library: &Library) -> TidyPlan {
        library
            .plan_tidy(&self.options(), &never(), &mut |_| {})
            .unwrap()
    }

    pub fn scan(&self, library: &mut Library) {
        scan(library, &self.roms);
    }

    pub fn journal_files(&self) -> Vec<PathBuf> {
        let Ok(rd) = fs::read_dir(&self.journals) else {
            return Vec::new();
        };
        let mut v: Vec<PathBuf> = rd
            .map(|e| e.unwrap().path())
            .filter(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with("journal-") && n.ends_with(".txt"))
            })
            .collect();
        v.sort();
        v
    }
}

pub fn scan(library: &mut Library, root: &Path) {
    let options = ScanOptions {
        roots: vec![root.to_path_buf()],
        ..Default::default()
    };
    library.scan(&options, &never(), &mut |_| {}).unwrap();
}

/// Ids of the units the UI checks by default (`Sure`), moves included.
pub fn sure_ids(plan: &TidyPlan) -> Vec<u32> {
    plan.units
        .iter()
        .filter(|u| matches!(u.status, TidyStatus::Sure(_)) && u.system.is_some())
        .map(|u| u.id)
        .collect()
}

pub fn apply(library: &mut Library, fx: &Fx, plan: &TidyPlan, ids: &[u32]) -> TidyReport {
    library
        .apply_tidy(&fx.options(), plan, ids, &never(), &mut |_| {})
        .unwrap()
}

pub fn undo(library: &mut Library, fx: &Fx, journal: &Path) -> TidyReport {
    library
        .undo_tidy(&fx.options(), journal, &never(), &mut |_| {})
        .unwrap()
}

/// The unit whose first source (or whole folder) is `primary` (relative, `/`) or whose title is `primary`.
pub fn unit<'p>(plan: &'p TidyPlan, primary: &str) -> &'p TidyUnit {
    plan.units
        .iter()
        .find(|u| {
            u.whole_dir
                .as_ref()
                .is_some_and(|w| w.from == Path::new(primary))
                || u.moves.iter().any(|m| m.from == Path::new(primary))
                || u.title == primary
        })
        .unwrap_or_else(|| panic!("no unit for {primary}: {:#?}", plan.units))
}

/// Every file under `root`: relative path (`/`) to content; folders as `path/` with empty content. Links are not
/// followed. Hard links show as distinct entries (the test then sees a leftover second name).
pub fn tree(root: &Path) -> BTreeMap<String, Vec<u8>> {
    fn walk(dir: &Path, root: &Path, out: &mut BTreeMap<String, Vec<u8>>) {
        for e in fs::read_dir(dir).unwrap() {
            let path = e.unwrap().path();
            let meta = fs::symlink_metadata(&path).unwrap();
            let rel = path
                .strip_prefix(root)
                .unwrap()
                .components()
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join("/");
            if meta.is_dir() {
                out.insert(format!("{rel}/"), Vec::new());
                walk(&path, root, out);
            } else if meta.is_symlink() {
                out.insert(
                    rel,
                    format!("->{}", fs::read_link(&path).unwrap().display()).into_bytes(),
                );
            } else {
                out.insert(rel, fs::read(&path).unwrap());
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(root, root, &mut out);
    out
}

/// Files only (folders dropped): what must never be lost.
pub fn files(root: &Path) -> BTreeMap<String, Vec<u8>> {
    tree(root)
        .into_iter()
        .filter(|(k, _)| !k.ends_with('/'))
        .collect()
}

/// Contents of every file, sorted, ignoring where they are: "nothing lost, nothing duplicated".
pub fn contents(root: &Path) -> Vec<Vec<u8>> {
    let mut v: Vec<Vec<u8>> = files(root).into_values().collect();
    v.sort();
    v
}

/// What the database says, without the paths' spelling: `(id, path relative to root, favorite, plays, media)`.
pub type Row = (GameId, String, bool, u32, Vec<String>);

pub fn rows(library: &Library, root: &Path) -> Vec<Row> {
    let rel = |p: &Path| {
        p.strip_prefix(root)
            .unwrap_or_else(|_| panic!("{p:?} not under {root:?}"))
            .components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join("/")
    };
    let mut v: Vec<Row> = library
        .games()
        .unwrap()
        .iter()
        .map(|g: &Game| {
            (
                g.id,
                rel(&g.path),
                g.favorite,
                g.play_count,
                g.media.iter().map(|m| rel(m)).collect(),
            )
        })
        .collect();
    v.sort();
    v
}

/// Every game of the library points at an existing path (file or folder).
pub fn assert_rows_exist(library: &Library) {
    for g in library.games().unwrap() {
        assert!(
            fs::symlink_metadata(&g.path).is_ok(),
            "row {:?} points at a missing path {:?}",
            g.title,
            g.path
        );
        for m in &g.media {
            assert!(
                fs::symlink_metadata(m).is_ok(),
                "media {m:?} of {:?} is missing",
                g.title
            );
        }
    }
}

pub fn python3_available() -> bool {
    Command::new("python3")
        .args(["-I", "-c", "pass"])
        .output()
        .is_ok_and(|o| o.status.success())
}

/// The big generated tree of `tests/tools/mktidy.py`; `None` (with a message) when python3 is missing.
pub struct Generated {
    pub _tmp: tempfile::TempDir,
    pub out: PathBuf,
    pub roms: PathBuf,
    pub journals: PathBuf,
}

pub fn generate() -> Option<Generated> {
    if !python3_available() {
        eprintln!("python3 not found: generated-tree test skipped");
        return None;
    }
    let tmp = tempfile::tempdir().unwrap();
    let out = tmp.path().join("gen");
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/tools/mktidy.py");
    let status = Command::new("python3")
        .arg("-I")
        .arg(script)
        .arg(&out)
        .status()
        .expect("python3 runs");
    assert!(status.success(), "mktidy.py failed");
    Some(Generated {
        roms: out.join("roms"),
        journals: tmp.path().join("rangement"),
        out,
        _tmp: tmp,
    })
}

impl Generated {
    pub fn options(&self) -> TidyOptions {
        TidyOptions::new(&self.roms, &self.journals).unwrap()
    }
}

/// Unix: true when this process may write inside a folder whose write bit was removed (root, some CI sandboxes).
#[cfg(unix)]
pub fn ignores_permissions(scratch: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    let dir = scratch.join("perm-probe");
    fs::create_dir(&dir).unwrap();
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o555)).unwrap();
    let writable = fs::write(dir.join("x"), b"x").is_ok();
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o755)).unwrap();
    fs::remove_dir_all(&dir).unwrap();
    writable
}
