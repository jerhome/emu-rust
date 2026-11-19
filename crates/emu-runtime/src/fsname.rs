//! One file-name sanitizer for everything the runtime derives from external names (game names, core ids,
//! archive entries). The rules are Windows' on every platform so that save files stay portable between
//! machines (a save folder copied from Linux to Windows must still be valid).

/// True for DOS device names, which Windows refuses as a file name with or without extension
/// (`CON`, `con.txt`, `COM1.srm`), case-insensitively.
fn is_reserved(name: &str) -> bool {
    let base = name.split('.').next().unwrap_or("");
    // Windows also ignores trailing spaces before the extension (`CON .txt`).
    let base = base.trim_end_matches(' ');
    let upper = base.to_ascii_uppercase();
    match upper.as_str() {
        "CON" | "PRN" | "AUX" | "NUL" => true,
        _ => {
            (upper.len() == 4 && (upper.starts_with("COM") || upper.starts_with("LPT")))
                && matches!(upper.as_bytes()[3], b'1'..=b'9')
        }
    }
}

/// Longest prefix of `s` that is at most `max` bytes, cut on a character boundary.
fn truncate_utf8(s: &str, max: usize) -> &str {
    if s.len() <= max {
        return s;
    }
    let mut end = max;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

/// Makes `raw` a single, portable file-name component of at most `max_bytes` bytes:
/// path separators, control characters and `<>:"|?*` become `_`; trailing dots and spaces (which Windows
/// strips silently) are removed; reserved device names get a leading `_`.
/// May return an empty string (`""`, `"."`, `".."`): callers decide the fallback.
pub(crate) fn sanitize_component(raw: &str, max_bytes: usize) -> String {
    let replaced: String = raw
        .chars()
        .map(|c| {
            if c.is_control() || matches!(c, '/' | '\\' | '<' | '>' | ':' | '"' | '|' | '?' | '*') {
                '_'
            } else {
                c
            }
        })
        .collect();
    let mut name = replaced.trim_end_matches(['.', ' ']).to_owned();
    if is_reserved(&name) {
        name.insert(0, '_');
    }
    // Truncating can expose new trailing dots or spaces.
    truncate_utf8(&name, max_bytes)
        .trim_end_matches(['.', ' '])
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(x: &str) -> String {
        sanitize_component(x, 200)
    }

    #[test]
    fn plain_names_are_untouched() {
        assert_eq!(s("Sonic (World)"), "Sonic (World)");
        assert_eq!(s("Ys.Book.I"), "Ys.Book.I");
        assert_eq!(s("ñandú"), "ñandú");
    }

    #[test]
    fn reserved_names_are_prefixed_on_every_platform() {
        for n in [
            "CON",
            "con",
            "Prn",
            "AUX",
            "nul",
            "COM1",
            "com9",
            "LPT1",
            "lpt9",
            "CON.txt",
            "nul.tar.gz",
            "COM3.srm",
            "con .txt",
        ] {
            assert_eq!(s(n), format!("_{n}"), "{n}");
        }
        for n in [
            "COM0",
            "COM10",
            "LPT",
            "CONX",
            "console",
            "AUXILIARY",
            "xCON",
            "COM",
        ] {
            assert_eq!(s(n), n, "{n}");
        }
    }

    #[test]
    fn forbidden_characters_and_controls() {
        assert_eq!(s("a<b>c:d\"e|f?g*h"), "a_b_c_d_e_f_g_h");
        assert_eq!(s("a/b\\c\u{1}d\0e\u{7f}"), "a_b_c_d_e_");
    }

    #[test]
    fn trailing_dots_and_spaces() {
        assert_eq!(s("x.bin. . "), "x.bin");
        assert_eq!(s("."), "");
        assert_eq!(s(".."), "");
        assert_eq!(s("   "), "");
        assert_eq!(s(""), "");
        assert_eq!(s(".hidden"), ".hidden");
    }

    #[test]
    fn length_is_bounded_on_char_boundaries() {
        let long = "é".repeat(300);
        let out = sanitize_component(&long, 200);
        assert_eq!(out.len(), 200);
        assert!(out.chars().all(|c| c == 'é'));
        assert_eq!(
            sanitize_component(&format!("{}.", "a".repeat(5)), 5),
            "aaaaa"
        );
        // cut right after a dot: no trailing dot left
        assert_eq!(sanitize_component("abc.def", 4), "abc");
        // the reserved prefix counts in the bound
        assert!(sanitize_component("CON", 3).len() <= 3);
    }
}
