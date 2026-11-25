//! `config.toml` of the two product lines (design v7.5 §4): a file carrying every key of the 7.4 (Mac) and of the
//! 7.1.0-rc.1 / 7.2 (PC) lines loses nothing through a load / save / load cycle, and the legacy alias
//! `[interface] console` becomes `[app] show_terminal` and is then gone.

use std::fs;

use emu_ui::Config;

/// Keys of the 7.4: `[app]`, `[ui]`, gamepad and computer settings, cores, covers, credentials.
const FILE_74: &str = r#"
[app]
show_terminal = true

[ui]
wizard_done = true

[video]
vsync = false
scale = 3

[audio]
latency_ms = 120

[speed]
fast_forward_ratio = 4.0

[library]
roots = ["/roms", "/more"]
scan_on_start = false
covers_missing_reset = 2

[covers]
download = true
retry_days = 10

[cores]
buildbot_url = "https://example.org/b"

[cores.preferred]
megacd = "picodrive"

[core_options.picodrive]
some_option = "value"

[unknown_future_table]
key = "kept"
"#;

/// Keys of the 7.1.0-rc.1 + 7.2 (PC line).
const FILE_PC: &str = r#"
[interface]
console = true
firmware_hints_seen = ["swanstation/scph5501.bin", "mgba/gba_bios.bin"]

[library]
roots = ["/roms"]
cd32x = ["night trap (disc 1).chd", "night trap (disc 2).chd"]
"#;

fn cycle(text: &str) -> (Config, String, Config) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    fs::write(&path, text).unwrap();
    let first = Config::load(&path).unwrap();
    first.save(&path).unwrap();
    let written = fs::read_to_string(&path).unwrap();
    let second = Config::load(&path).unwrap();
    (first, written, second)
}

#[test]
fn a_7_4_file_loses_no_key() {
    let (first, written, second) = cycle(FILE_74);
    assert_eq!(first, second);
    assert!(first.app.show_terminal && first.ui.wizard_done);
    assert!(!first.video.vsync && first.video.scale == 3);
    assert_eq!(first.audio.latency_ms, 120);
    assert_eq!(first.library.roots.len(), 2);
    assert!(!first.library.scan_on_start);
    assert!(first.covers.download && first.covers.retry_days == 10);
    assert_eq!(
        first.cores.buildbot_url.as_deref(),
        Some("https://example.org/b")
    );
    assert_eq!(first.cores.preferred["megacd"], "picodrive");
    assert_eq!(first.core_options["picodrive"]["some_option"], "value");
    for needle in [
        "show_terminal = true",
        "wizard_done = true",
        "unknown_future_table",
        "kept",
    ] {
        assert!(written.contains(needle), "{needle} lost: {written}");
    }
    assert!(
        !written.contains("interface") && !written.contains("cd32x"),
        "{written}"
    );
}

#[test]
fn a_7_1_rc1_pc_file_loses_no_key_and_converts_console() {
    let (first, written, second) = cycle(FILE_PC);
    assert_eq!(first, second);
    assert!(
        first.app.show_terminal,
        "console = true becomes show_terminal"
    );
    assert_eq!(first.library.cd32x.len(), 2);
    assert!(!written.contains("console"), "{written}");
    assert!(
        written.contains("[app]") && written.contains("show_terminal = true"),
        "{written}"
    );
    assert!(written.contains("firmware_hints_seen"), "{written}");
    assert!(
        written.contains("swanstation/scph5501.bin") && written.contains("mgba/gba_bios.bin"),
        "{written}"
    );
    assert!(written.contains("night trap (disc 2).chd"), "{written}");
}

#[test]
fn console_false_does_not_set_the_terminal() {
    let (first, written, _) =
        cycle("[interface]\nconsole = false\nfirmware_hints_seen = [\"a/b\"]\n");
    assert!(!first.app.show_terminal);
    assert!(
        !written.contains("console") && written.contains("a/b"),
        "{written}"
    );
}

/// Both lines at once (a user who went 7.1.0-rc.1 -> 7.4 -> 7.5): the union is kept; an explicit
/// `show_terminal = true` and a `console = false` stay true.
#[test]
fn the_union_of_both_lines_keeps_everything() {
    let text =
        format!("{FILE_74}\n[interface]\nconsole = false\nfirmware_hints_seen = [\"x/y\"]\n");
    let text = text.replace("[library]\nroots", "[library]\ncd32x = [\"a.chd\"]\nroots");
    let (first, written, second) = cycle(&text);
    assert_eq!(first, second);
    assert!(
        first.app.show_terminal,
        "an explicit show_terminal is not reset by console = false"
    );
    assert_eq!(first.library.cd32x, ["a.chd"]);
    assert!(
        written.contains("x/y") && written.contains("unknown_future_table"),
        "{written}"
    );
}

/// The alias is one way: once converted and saved, unchecking the terminal setting sticks.
#[test]
fn unchecking_the_terminal_after_conversion_sticks() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    fs::write(&path, FILE_PC).unwrap();
    let mut cfg = Config::load(&path).unwrap();
    cfg.save(&path).unwrap();
    cfg.app.show_terminal = false;
    cfg.save(&path).unwrap();
    assert!(!Config::load(&path).unwrap().app.show_terminal);
}
