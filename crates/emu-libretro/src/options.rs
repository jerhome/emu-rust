//! Core options: legacy `SET_VARIABLES`, v1 `SET_CORE_OPTIONS[_INTL]`, v2 `SET_CORE_OPTIONS_V2[_INTL]`.
//!
//! Declarations are copied out of the core's structures (pointers never kept) into an `OptionStore` owned by
//! the environment state. `GET_VARIABLE` hands out pointers into `OptionEntry::values_c`, which only change when
//! the core declares its options again, so a value pointer stays valid across `set_option`.

use std::ffi::{CStr, CString, c_char};

use crate::error::LibretroError;
use crate::sys;
use crate::types::{CoreOption, CoreOptionCategory, CoreOptionValue};

/// One option as declared by the core, before merging with current values.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OptionDecl {
    pub key: String,
    pub label: String,
    pub info: Option<String>,
    pub category: Option<String>,
    pub values: Vec<CoreOptionValue>,
    /// Always one of `values`.
    pub default: String,
}

struct OptionEntry {
    decl: OptionDecl,
    key_c: CString,
    /// Same order as `decl.values`.
    values_c: Vec<CString>,
    current: usize,
    visible: bool,
}

#[derive(Default)]
pub(crate) struct OptionStore {
    entries: Vec<OptionEntry>,
    categories: Vec<CoreOptionCategory>,
    /// Answered (then cleared) by `GET_VARIABLE_UPDATE`.
    updated: bool,
}

/// Copies a possibly-null C string; `None` for null or empty strings.
///
/// # Safety
/// `ptr` is null or a valid NUL-terminated string.
unsafe fn opt_string(ptr: *const c_char) -> Option<String> {
    if ptr.is_null() {
        return None;
    }
    // SAFETY: non-null and NUL-terminated per contract; copied immediately.
    let s = unsafe { CStr::from_ptr(ptr) }.to_string_lossy();
    (!s.is_empty()).then(|| s.into_owned())
}

/// # Safety
/// Every non-null `value` / `label` pointer in `values` is a valid NUL-terminated string.
unsafe fn parse_values(
    values: &[sys::retro_core_option_value; sys::RETRO_NUM_CORE_OPTION_VALUES_MAX],
) -> Vec<CoreOptionValue> {
    values
        .iter()
        // The list ends at the first null value (or at the array bound).
        .take_while(|v| !v.value.is_null())
        .filter_map(|v| {
            // SAFETY: per this function's contract.
            let (value, label) = unsafe { (opt_string(v.value)?, opt_string(v.label)) };
            Some(CoreOptionValue {
                label: label.unwrap_or_else(|| value.clone()),
                value,
            })
        })
        .collect()
}

/// `default` if it names one of `values`, else the first value.
fn pick_default(values: &[CoreOptionValue], default: Option<String>) -> Option<String> {
    match default {
        Some(d) if values.iter().any(|v| v.value == d) => Some(d),
        _ => values.first().map(|v| v.value.clone()),
    }
}

/// Parses a legacy `SET_VARIABLES` array.
///
/// # Safety
/// `vars` is null or points to an array of `retro_variable` terminated by a null key, whose non-null strings are
/// valid NUL-terminated C strings for the duration of the call.
pub(crate) unsafe fn parse_legacy(mut vars: *const sys::retro_variable) -> Vec<OptionDecl> {
    let mut out = Vec::new();
    if vars.is_null() {
        return out;
    }
    loop {
        // SAFETY: within the null-key-terminated array per contract; we stop at the terminator.
        let var = unsafe { vars.read() };
        if var.key.is_null() {
            break;
        }
        // SAFETY: the terminator has not been reached, so the next element is still in the array.
        vars = unsafe { vars.add(1) };
        // SAFETY: non-null strings are valid per contract.
        let (Some(key), Some(raw)) = (unsafe { (opt_string(var.key), opt_string(var.value)) })
        else {
            continue;
        };
        let Some((label, values)) = parse_variable_value(&raw) else {
            tracing::debug!(key, value = raw, "malformed core variable declaration");
            continue;
        };
        let values: Vec<CoreOptionValue> = values
            .into_iter()
            .map(|v| CoreOptionValue {
                value: v.to_owned(),
                label: v.to_owned(),
            })
            .collect();
        out.push(OptionDecl {
            key,
            label: label.to_owned(),
            info: None,
            category: None,
            default: values[0].value.clone(),
            values,
        });
    }
    out
}

/// Splits a legacy `SET_VARIABLES` value `"Description; a|b|c"` into its description and values.
pub(crate) fn parse_variable_value(raw: &str) -> Option<(&str, Vec<&str>)> {
    let (description, options) = raw.split_once(';')?;
    let options: Vec<&str> = options.trim_start().split('|').collect();
    if options.iter().any(|o| o.is_empty()) {
        return None;
    }
    Some((description.trim(), options))
}

/// Parses a v1 definition array.
///
/// # Safety
/// `defs` is null or points to an array of `retro_core_option_definition` terminated by a null key, whose
/// non-null strings are valid NUL-terminated C strings for the duration of the call.
pub(crate) unsafe fn parse_v1(
    mut defs: *const sys::retro_core_option_definition,
) -> Vec<OptionDecl> {
    let mut out = Vec::new();
    if defs.is_null() {
        return out;
    }
    loop {
        // SAFETY: inside the null-key-terminated array per contract; the struct is only borrowed for this
        // iteration.
        let def = unsafe { &*defs };
        if def.key.is_null() {
            break;
        }
        // SAFETY: the terminator has not been reached.
        defs = unsafe { defs.add(1) };
        // SAFETY: non-null strings are valid per contract.
        let (key, label, info, values, default) = unsafe {
            (
                opt_string(def.key),
                opt_string(def.desc),
                opt_string(def.info),
                parse_values(&def.values),
                opt_string(def.default_value),
            )
        };
        let Some(key) = key else { continue };
        let Some(default) = pick_default(&values, default) else {
            tracing::debug!(key, "core option declared without values");
            continue;
        };
        out.push(OptionDecl {
            label: label.unwrap_or_else(|| key.clone()),
            key,
            info,
            category: None,
            values,
            default,
        });
    }
    out
}

/// Parses a v2 option set (categories + definitions).
///
/// # Safety
/// `options` is null or points to a valid `retro_core_options_v2` whose arrays are null or terminated by a null
/// key, and whose non-null strings are valid NUL-terminated C strings for the duration of the call.
pub(crate) unsafe fn parse_v2(
    options: *const sys::retro_core_options_v2,
) -> (Vec<CoreOptionCategory>, Vec<OptionDecl>) {
    let mut categories = Vec::new();
    let mut decls = Vec::new();
    if options.is_null() {
        return (categories, decls);
    }
    // SAFETY: non-null and valid per contract.
    let options = unsafe { options.read() };

    let mut cat = options.categories;
    while !cat.is_null() {
        // SAFETY: inside the null-key-terminated array per contract.
        let c = unsafe { cat.read() };
        if c.key.is_null() {
            break;
        }
        // SAFETY: the terminator has not been reached.
        cat = unsafe { cat.add(1) };
        // SAFETY: non-null strings are valid per contract.
        let (key, label, info) =
            unsafe { (opt_string(c.key), opt_string(c.desc), opt_string(c.info)) };
        if let Some(key) = key {
            categories.push(CoreOptionCategory {
                label: label.unwrap_or_else(|| key.clone()),
                key,
                info,
            });
        }
    }

    let mut defs = options.definitions;
    while !defs.is_null() {
        // SAFETY: inside the null-key-terminated array per contract; borrowed for this iteration only.
        let def = unsafe { &*defs };
        if def.key.is_null() {
            break;
        }
        // SAFETY: the terminator has not been reached.
        defs = unsafe { defs.add(1) };
        // SAFETY: non-null strings are valid per contract.
        let (key, desc, desc_cat, info, info_cat, category, values, default) = unsafe {
            (
                opt_string(def.key),
                opt_string(def.desc),
                opt_string(def.desc_categorized),
                opt_string(def.info),
                opt_string(def.info_categorized),
                opt_string(def.category_key),
                parse_values(&def.values),
                opt_string(def.default_value),
            )
        };
        let Some(key) = key else { continue };
        let Some(default) = pick_default(&values, default) else {
            tracing::debug!(key, "core option declared without values");
            continue;
        };
        // libretro.h: a category key that matches no declared category means "uncategorised".
        let category = category.filter(|k| categories.iter().any(|c| c.key == *k));
        let (label, info) = if category.is_some() {
            (desc_cat.or(desc), info_cat.or(info))
        } else {
            (desc, info)
        };
        decls.push(OptionDecl {
            label: label.unwrap_or_else(|| key.clone()),
            key,
            info,
            category,
            values,
            default,
        });
    }
    (categories, decls)
}

impl OptionStore {
    /// Replaces the declared options. The value of each option is, in order of preference: its current value
    /// if the key was already declared and the value is still allowed, the frontend override, the default.
    pub(crate) fn declare(
        &mut self,
        categories: Vec<CoreOptionCategory>,
        decls: Vec<OptionDecl>,
        overrides: &[(String, String)],
    ) {
        let previous = std::mem::take(&mut self.entries);
        for decl in decls {
            if self.entries.iter().any(|e| e.decl.key == decl.key) {
                tracing::debug!(key = decl.key, "duplicate core option ignored");
                continue;
            }
            let (Ok(key_c), Ok(values_c)) = (
                CString::new(decl.key.as_str()),
                decl.values
                    .iter()
                    .map(|v| CString::new(v.value.as_str()))
                    .collect::<Result<Vec<_>, _>>(),
            ) else {
                continue;
            };
            let index_of = |value: &str| decl.values.iter().position(|v| v.value == value);
            let old = previous.iter().find(|e| e.decl.key == decl.key);
            let kept = old.and_then(|e| index_of(&e.decl.values[e.current].value));
            let overridden = overrides
                .iter()
                .find(|(k, _)| *k == decl.key)
                .and_then(|(_, v)| {
                    let index = index_of(v);
                    if index.is_none() {
                        tracing::warn!(
                            key = decl.key,
                            value = v,
                            "core option override is not an allowed value; using the default"
                        );
                    }
                    index
                });
            let current = kept
                .or(overridden)
                .or_else(|| index_of(&decl.default))
                .unwrap_or(0);
            self.entries.push(OptionEntry {
                visible: old.is_none_or(|e| e.visible),
                key_c,
                values_c,
                current,
                decl,
            });
        }
        self.categories = categories;
    }

    /// Pointer answered to `GET_VARIABLE`; valid until the next `declare` or until the store is dropped.
    pub(crate) fn get(&self, key: &CStr) -> Option<*const c_char> {
        self.entries
            .iter()
            .find(|e| e.key_c.as_c_str() == key)
            .map(|e| e.values_c[e.current].as_ptr())
    }

    /// Frontend-side change (`Core::set_option`): flags `GET_VARIABLE_UPDATE`.
    pub(crate) fn set(&mut self, key: &str, value: &str) -> Result<(), LibretroError> {
        let entry = self
            .entries
            .iter_mut()
            .find(|e| e.decl.key == key)
            .ok_or_else(|| LibretroError::UnknownOption(key.to_owned()))?;
        let index = entry
            .decl
            .values
            .iter()
            .position(|v| v.value == value)
            .ok_or_else(|| LibretroError::InvalidOptionValue {
                key: key.to_owned(),
                value: value.to_owned(),
            })?;
        if entry.current != index {
            entry.current = index;
            self.updated = true;
        }
        Ok(())
    }

    /// Core-side change (`SET_VARIABLE`): no `GET_VARIABLE_UPDATE`, the core already knows. Returns whether the
    /// key and value were valid.
    pub(crate) fn set_from_core(&mut self, key: &CStr, value: &CStr) -> bool {
        let Some(entry) = self.entries.iter_mut().find(|e| e.key_c.as_c_str() == key) else {
            return false;
        };
        match entry.values_c.iter().position(|v| v.as_c_str() == value) {
            Some(index) => {
                entry.current = index;
                true
            }
            None => false,
        }
    }

    /// `SET_CORE_OPTIONS_DISPLAY`; returns whether the visibility changed.
    pub(crate) fn set_visible(&mut self, key: &CStr, visible: bool) -> bool {
        match self.entries.iter_mut().find(|e| e.key_c.as_c_str() == key) {
            Some(entry) if entry.visible != visible => {
                entry.visible = visible;
                true
            }
            _ => false,
        }
    }

    pub(crate) fn take_updated(&mut self) -> bool {
        std::mem::take(&mut self.updated)
    }

    pub(crate) fn snapshot(&self) -> Vec<CoreOption> {
        self.entries
            .iter()
            .map(|e| CoreOption {
                key: e.decl.key.clone(),
                label: e.decl.label.clone(),
                info: e.decl.info.clone(),
                category: e.decl.category.clone(),
                values: e.decl.values.clone(),
                default: e.decl.default.clone(),
                current: e.decl.values[e.current].value.clone(),
                visible: e.visible,
            })
            .collect()
    }

    pub(crate) fn categories(&self) -> Vec<CoreOptionCategory> {
        self.categories.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ptr::null;

    const NO_VALUE: sys::retro_core_option_value = sys::retro_core_option_value {
        value: null(),
        label: null(),
    };

    fn values(
        list: &[(&'static CStr, Option<&'static CStr>)],
    ) -> [sys::retro_core_option_value; 128] {
        let mut out = [NO_VALUE; 128];
        for (slot, (value, label)) in out.iter_mut().zip(list) {
            *slot = sys::retro_core_option_value {
                value: value.as_ptr(),
                label: label.map_or(null(), CStr::as_ptr),
            };
        }
        out
    }

    fn v(value: &str, label: &str) -> CoreOptionValue {
        CoreOptionValue {
            value: value.into(),
            label: label.into(),
        }
    }

    #[test]
    fn parse_variable_value_legacy_format() {
        assert_eq!(
            parse_variable_value("Region; auto|ntsc-u|pal"),
            Some(("Region", vec!["auto", "ntsc-u", "pal"]))
        );
        assert_eq!(parse_variable_value("Lone;x"), Some(("Lone", vec!["x"])));
        assert_eq!(
            parse_variable_value("Has spaces; a b|c"),
            Some(("Has spaces", vec!["a b", "c"]))
        );
        assert_eq!(parse_variable_value("No separator"), None);
        assert_eq!(parse_variable_value("Empty; "), None);
        assert_eq!(parse_variable_value("Hole; a||b"), None);
    }

    #[test]
    fn v1_definitions() {
        let defs = [
            sys::retro_core_option_definition {
                key: c"opt_a".as_ptr(),
                desc: c"Option A".as_ptr(),
                info: c"Info A".as_ptr(),
                values: values(&[(c"on", Some(c"On")), (c"off", None)]),
                default_value: c"off".as_ptr(),
            },
            // Default not among the values: first value is used.
            sys::retro_core_option_definition {
                key: c"opt_b".as_ptr(),
                desc: null(),
                info: null(),
                values: values(&[(c"1", None), (c"2", None)]),
                default_value: c"9".as_ptr(),
            },
            // No values: skipped.
            sys::retro_core_option_definition {
                key: c"opt_empty".as_ptr(),
                desc: c"Empty".as_ptr(),
                info: null(),
                values: values(&[]),
                default_value: null(),
            },
            sys::retro_core_option_definition {
                key: null(),
                desc: null(),
                info: null(),
                values: values(&[]),
                default_value: null(),
            },
        ];
        // SAFETY: null-key-terminated array of valid definitions with static strings.
        let decls = unsafe { parse_v1(defs.as_ptr()) };
        assert_eq!(
            decls,
            vec![
                OptionDecl {
                    key: "opt_a".into(),
                    label: "Option A".into(),
                    info: Some("Info A".into()),
                    category: None,
                    values: vec![v("on", "On"), v("off", "off")],
                    default: "off".into(),
                },
                OptionDecl {
                    key: "opt_b".into(),
                    label: "opt_b".into(),
                    info: None,
                    category: None,
                    values: vec![v("1", "1"), v("2", "2")],
                    default: "1".into(),
                },
            ]
        );
    }

    #[test]
    fn v2_definitions_and_categories() {
        let cats = [
            sys::retro_core_option_v2_category {
                key: c"video".as_ptr(),
                desc: c"Video".as_ptr(),
                info: c"Video settings".as_ptr(),
            },
            sys::retro_core_option_v2_category {
                key: null(),
                desc: null(),
                info: null(),
            },
        ];
        let def =
            |key: &'static CStr, cat: Option<&'static CStr>| sys::retro_core_option_v2_definition {
                key: key.as_ptr(),
                desc: c"Video > Color".as_ptr(),
                desc_categorized: c"Color".as_ptr(),
                info: c"Long info".as_ptr(),
                info_categorized: null(),
                category_key: cat.map_or(null(), CStr::as_ptr),
                values: values(&[(c"red", Some(c"Red")), (c"green", None)]),
                default_value: c"green".as_ptr(),
            };
        let defs = [
            def(c"color", Some(c"video")),
            def(c"orphan", Some(c"missing")),
            def(c"plain", None),
            sys::retro_core_option_v2_definition {
                key: null(),
                ..def(c"x", None)
            },
        ];
        let options = sys::retro_core_options_v2 {
            categories: cats.as_ptr(),
            definitions: defs.as_ptr(),
        };
        // SAFETY: valid v2 structure with null-key-terminated arrays and static strings.
        let (categories, decls) = unsafe { parse_v2(&raw const options) };
        assert_eq!(
            categories,
            vec![CoreOptionCategory {
                key: "video".into(),
                label: "Video".into(),
                info: Some("Video settings".into()),
            }]
        );
        assert_eq!(decls.len(), 3);
        assert_eq!(decls[0].label, "Color");
        assert_eq!(decls[0].category.as_deref(), Some("video"));
        // info_categorized is null: falls back to info.
        assert_eq!(decls[0].info.as_deref(), Some("Long info"));
        assert_eq!(decls[0].default, "green");
        assert_eq!(decls[0].values, vec![v("red", "Red"), v("green", "green")]);
        assert_eq!(decls[1].category, None);
        assert_eq!(decls[1].label, "Video > Color");
        assert_eq!(decls[2].category, None);

        // Null categories pointer is allowed.
        let options = sys::retro_core_options_v2 {
            categories: null(),
            definitions: defs.as_ptr(),
        };
        // SAFETY: as above.
        let (categories, decls) = unsafe { parse_v2(&raw const options) };
        assert!(categories.is_empty());
        assert!(decls.iter().all(|d| d.category.is_none()));
        // SAFETY: null is allowed.
        assert_eq!(unsafe { parse_v2(null()) }, (Vec::new(), Vec::new()));
    }

    fn decl(key: &str, values: &[&str], default: &str) -> OptionDecl {
        OptionDecl {
            key: key.into(),
            label: key.into(),
            info: None,
            category: None,
            values: values.iter().map(|x| v(x, x)).collect(),
            default: default.into(),
        }
    }

    #[test]
    fn store_merges_overrides_and_keeps_values_on_redeclare() {
        let mut store = OptionStore::default();
        let overrides = vec![
            ("a".to_owned(), "2".to_owned()),
            ("b".to_owned(), "bogus".to_owned()),
        ];
        store.declare(
            Vec::new(),
            vec![
                decl("a", &["1", "2"], "1"),
                decl("b", &["x", "y"], "y"),
                decl("a", &["9"], "9"),
            ],
            &overrides,
        );
        let snap = store.snapshot();
        assert_eq!(snap.len(), 2);
        assert_eq!(
            (snap[0].current.as_str(), snap[1].current.as_str()),
            ("2", "y")
        );
        let ptr = store.get(c"a").unwrap();
        // SAFETY: points into the store, alive.
        assert_eq!(unsafe { CStr::from_ptr(ptr) }, c"2");
        assert_eq!(store.get(c"zz"), None);

        assert!(!store.take_updated());
        store.set("a", "1").unwrap();
        // The pointer handed out before `set` is still valid (values are never reallocated by `set`).
        // SAFETY: as above.
        assert_eq!(unsafe { CStr::from_ptr(ptr) }, c"2");
        assert!(store.take_updated());
        assert!(!store.take_updated());
        store.set("a", "1").unwrap();
        assert!(
            !store.take_updated(),
            "unchanged value does not flag an update"
        );
        assert!(matches!(store.set("zz", "1"), Err(LibretroError::UnknownOption(k)) if k == "zz"));
        assert!(matches!(
            store.set("a", "3"),
            Err(LibretroError::InvalidOptionValue { key, value }) if key == "a" && value == "3"
        ));

        assert!(store.set_visible(c"b", false));
        assert!(!store.set_visible(c"b", false));
        assert!(!store.set_visible(c"zz", false));
        assert!(store.set_from_core(c"b", c"x"));
        assert!(!store.set_from_core(c"b", c"nope"));
        assert!(!store.take_updated());

        // Re-declaration keeps current values still allowed and visibility.
        store.declare(
            Vec::new(),
            vec![
                decl("a", &["1", "2", "3"], "3"),
                decl("b", &["y", "z"], "z"),
            ],
            &overrides,
        );
        let snap = store.snapshot();
        assert_eq!(snap[0].current, "1");
        // "x" no longer allowed, override invalid: default.
        assert_eq!(snap[1].current, "z");
        assert!(!snap[1].visible);
        assert!(snap[0].visible);
    }
}
