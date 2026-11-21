//! "Computed defaults" layer of core options (design v3.0 §3.5.4).
//!
//! Order, for every core: core defaults < `cores.toml` `options` < **computed defaults** < `config.toml`
//! `[core_options.<core>]` < per-game options. Computed defaults are never written anywhere, so an explicit user
//! setting always wins. 3.0: firmware fallback only; 4.0 adds `layout_option`, 5.0 `options_by_extension`.

use emu_assets::cores::{CoreId, CorePreferences, FirmwareCheck, FirmwareFallback, FirmwareState};

/// Everything the computed defaults depend on. `locale` and `extension` are read by later versions.
pub(crate) struct ComputedContext<'a> {
    pub prefs: &'a CorePreferences,
    pub core: &'a CoreId,
    /// Libretro database name of the system (`"Commodore - Amiga"`).
    pub system: &'a str,
    /// Result of the single firmware check of this launch, when one was made.
    pub firmware: Option<&'a FirmwareCheck>,
    // Read by the 4.0 (`layout_option`) and 5.0 (`options_by_extension`) layers; filled from 3.0 on.
    #[allow(dead_code)]
    pub locale: &'a str,
    #[allow(dead_code)]
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
    firmware_fallback_options(ctx.firmware, fallback)
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
}
