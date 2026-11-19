//! OS-specific user-facing hints for core loading failures (French, like the rest of the CLI).

use std::path::Path;

/// Shared-library extension expected by libretro cores on this OS.
pub const CORE_EXTENSION: &str = if cfg!(windows) {
    "dll"
} else if cfg!(target_os = "macos") {
    "dylib"
} else {
    "so"
};

const BUILDBOT_HINT: &str = "Les cores officiels se téléchargent sur https://buildbot.libretro.com/nightly/ \
                             (archives .zip à décompresser, dossier correspondant à votre système, x86_64).";

/// Checks the core path before loading; the returned message replaces a cryptic loader error.
pub fn check_core(core: &Path) -> Result<(), String> {
    if !core.is_file() {
        return Err(format!(
            "core introuvable : {}\nUn core se présente sous la forme d'un fichier .{CORE_EXTENSION} sur ce système.\n{BUILDBOT_HINT}",
            core.display()
        ));
    }
    let ext = core.extension().and_then(|e| e.to_str()).unwrap_or("");
    if !ext.eq_ignore_ascii_case(CORE_EXTENSION) {
        return Err(format!(
            "{} n'a pas l'extension attendue (.{CORE_EXTENSION} sur ce système).\n{BUILDBOT_HINT}",
            core.display()
        ));
    }
    Ok(())
}

/// Appended to a runtime error (the core exists but could not be loaded or started).
pub fn load_failure_hint() -> String {
    format!(
        "Vérifiez que le core est compilé pour ce système et cette architecture (.{CORE_EXTENSION}, x86_64) \
         et que la ROM convient à ce core.\n{BUILDBOT_HINT}"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_core_names_the_extension() {
        let e = check_core(Path::new("/nonexistent/core")).unwrap_err();
        assert!(e.contains("introuvable"));
        assert!(e.contains(CORE_EXTENSION));
    }

    #[test]
    fn wrong_extension_is_rejected() {
        let dir = std::env::temp_dir();
        let f = dir.join(format!("emu-app-diag-{}.txt", std::process::id()));
        std::fs::write(&f, b"x").unwrap();
        let r = check_core(&f);
        std::fs::remove_file(&f).unwrap();
        assert!(r.unwrap_err().contains("extension"));
    }
}
