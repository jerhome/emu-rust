//! "Computed defaults" layer of core options (design v3.0 §3.5.4).
//!
//! Order, for every core: core defaults < `cores.toml` `options` < **computed defaults** < `config.toml`
//! `[core_options.<core>]` < per-game options. Computed defaults are never written anywhere, so an explicit user
//! setting always wins. 3.0: firmware fallback; 4.0 adds `layout_option`; 5.0 `options_by_extension`.

use emu_assets::cores::{
    CoreId, CorePreferences, FirmwareCheck, FirmwareFallback, FirmwareState, LayoutOption,
};

/// Everything the computed defaults depend on. `locale` and `extension` are read by later versions.
pub(crate) struct ComputedContext<'a> {
    pub prefs: &'a CorePreferences,
    pub core: &'a CoreId,
    /// Libretro database name of the system (`"Commodore - Amiga"`).
    pub system: &'a str,
    /// Result of the single firmware check of this launch, when one was made.
    pub firmware: Option<&'a FirmwareCheck>,
    /// System language for `layout_option`.
    pub locale: &'a str,
    /// Lower-case extension of the launched file, without the dot (`options_by_extension`); `""` when none.
    pub extension: &'a str,
}

/// Computed defaults for one launch, as `(key, value)` pairs ready to sit between `cores.toml` and the config.
pub(crate) fn computed_options(ctx: &ComputedContext) -> Vec<(String, String)> {
    // Looked up only when needed: the table is a per-(core, system) data lookup.
    let fallback = match ctx.firmware.map(|c| &c.state) {
        Some(FirmwareState::CoreFallback { .. }) => ctx
            .prefs
            .required_firmware(ctx.core, ctx.system)
            .and_then(|req| req.fallback.as_ref()),
        _ => None,
    };
    let mut out = firmware_fallback_options(ctx.firmware, fallback);
    if let Some(layout) = ctx.prefs.layout_option(ctx.core) {
        out.push((layout.option.clone(), layout_value(&layout, ctx.locale)));
    }
    // After the layout: a file type may pin a key the language default also sets (e.g. a `.cpr` model).
    if !ctx.extension.is_empty() {
        out.extend(ctx.prefs.options_by_extension(ctx.core, ctx.extension));
    }
    out
}

/// Value of a `layout_option` for the system language (`fr-FR` ⇒ `fr`, unknown ⇒ the fallback). Generic: the DOS
/// keyboard layout in 4.0, the CPC one in 5.0.
pub(crate) fn layout_value(opt: &LayoutOption, locale: &str) -> String {
    opt.value_for(Some(locale)).to_owned()
}

/// System language for the computed defaults (`sys-locale`), `""` when unknown (⇒ each table's fallback).
pub(crate) fn system_locale() -> String {
    sys_locale::get_locale().unwrap_or_default()
}

/// The wanted file is absent and the core has a built-in replacement: switch the core option to it.
fn firmware_fallback_options(
    check: Option<&FirmwareCheck>,
    fallback: Option<&FirmwareFallback>,
) -> Vec<(String, String)> {
    match (check.map(|c| &c.state), fallback) {
        (Some(FirmwareState::CoreFallback { .. }), Some(f)) => {
            vec![(f.option.clone(), f.value.clone())]
        }
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check(state: FirmwareState) -> FirmwareCheck {
        FirmwareCheck {
            wanted: Some("kick34005.A500".into()),
            state,
            others_present: Vec::new(),
        }
    }

    fn aros() -> FirmwareFallback {
        FirmwareFallback {
            option: "puae_kickstart".into(),
            value: "aros".into(),
        }
    }

    #[test]
    fn fallback_switches_kickstart_to_aros() {
        let c = check(FirmwareState::CoreFallback {
            label: "AROS".into(),
        });
        assert_eq!(
            firmware_fallback_options(Some(&c), Some(&aros())),
            vec![("puae_kickstart".to_owned(), "aros".to_owned())]
        );
    }

    #[test]
    fn no_override_when_the_rom_is_usable_or_unchecked() {
        for state in [
            FirmwareState::Ok,
            FirmwareState::Unverified,
            FirmwareState::Missing,
            FirmwareState::Builtin { label: "x".into() },
        ] {
            assert!(firmware_fallback_options(Some(&check(state)), Some(&aros())).is_empty());
        }
        assert!(firmware_fallback_options(None, Some(&aros())).is_empty());
    }

    #[test]
    fn no_override_without_declared_fallback() {
        let c = check(FirmwareState::CoreFallback {
            label: "AROS".into(),
        });
        assert!(firmware_fallback_options(Some(&c), None).is_empty());
    }

    #[test]
    fn other_checks_never_touch_the_table() {
        // `required_firmware` is only consulted for `CoreFallback` (the table lookup may not exist yet).
        let prefs = CorePreferences::default();
        let core = CoreId::new("puae").unwrap();
        let ok = check(FirmwareState::Ok);
        let ctx = ComputedContext {
            prefs: &prefs,
            core: &core,
            system: "Commodore - Amiga",
            firmware: Some(&ok),
            locale: "fr",
            extension: "adf",
        };
        assert!(computed_options(&ctx).is_empty());
    }

    fn dos_ctx<'a>(
        prefs: &'a CorePreferences,
        core: &'a CoreId,
        locale: &'a str,
    ) -> ComputedContext<'a> {
        ComputedContext {
            prefs,
            core,
            system: "DOS",
            firmware: None,
            locale,
            extension: "zip",
        }
    }

    /// The whole dosbox_pure table of `cores.toml` (design v4.0 §3.3).
    #[test]
    fn layout_value_follows_the_dosbox_pure_table() {
        let prefs = CorePreferences::builtin();
        let core = CoreId::new("dosbox_pure").unwrap();
        let layout = prefs
            .layout_option(&core)
            .expect("layout_option de dosbox_pure");
        assert_eq!(layout.option, "dosbox_pure_keyboard_layout");
        for (locale, want) in [
            ("fr-FR", "fr"),
            ("fr_FR.UTF-8", "fr"),
            ("fr", "fr"),
            ("fr-BE", "be"),
            ("fr-CH", "sf"),
            ("de-DE", "gr"),
            ("de-CH", "sg"),
            ("en-GB", "uk"),
            ("en-US", "us"),
            ("es-ES", "sp"),
            ("it-IT", "it"),
            ("ja-JP", "us"),
            ("", "us"),
        ] {
            assert_eq!(layout_value(&layout, locale), want, "{locale}");
        }
        assert_eq!(
            computed_options(&dos_ctx(&prefs, &core, "fr-FR")),
            vec![("dosbox_pure_keyboard_layout".to_owned(), "fr".to_owned())]
        );
        // Cores without a `layout_option` get nothing.
        let hatari = CoreId::new("hatari").unwrap();
        assert!(computed_options(&dos_ctx(&prefs, &hatari, "fr-FR")).is_empty());
    }

    /// Computed defaults sit under the config and the game (design v3.0 §3.5.4).
    #[test]
    fn config_and_game_override_the_computed_layout() {
        use crate::game_options::effective_core_options_with;
        use std::collections::BTreeMap;
        let prefs = CorePreferences::builtin();
        let core = CoreId::new("dosbox_pure").unwrap();
        let computed = computed_options(&dos_ctx(&prefs, &core, "fr-FR"));
        let frontend = prefs.frontend_options(&core);
        let key = "dosbox_pure_keyboard_layout";
        let value = |global: &BTreeMap<String, String>, game: &BTreeMap<String, String>| {
            effective_core_options_with(
                &[(key.to_owned(), "us".to_owned())],
                &frontend,
                &computed,
                global,
                game,
            )
            .into_iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v)
        };
        let none = BTreeMap::new();
        assert_eq!(value(&none, &none).as_deref(), Some("fr"));
        let global = BTreeMap::from([(key.to_owned(), "uk".to_owned())]);
        assert_eq!(value(&global, &none).as_deref(), Some("uk"));
        let game = BTreeMap::from([(key.to_owned(), "gr".to_owned())]);
        assert_eq!(value(&global, &game).as_deref(), Some("gr"));
    }
}
