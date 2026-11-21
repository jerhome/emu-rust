//! `SET_CONTROLLER_INFO` (env 35): copy of the per-port device types declared by the core (design v7.0 §3.1).

use std::ffi::c_char;

use crate::sys;
use crate::types::ControllerType;

/// Ports read from the core's array; later entries are ignored.
pub(crate) const MAX_PORTS: usize = 8;
/// Types read per port; later entries are ignored.
pub(crate) const MAX_TYPES: usize = 32;
/// Characters kept from a description (after removing control characters).
const MAX_DESC_CHARS: usize = 64;
/// Bytes scanned for a description: enough for `MAX_DESC_CHARS` 4-byte characters, so a string the core forgot to
/// terminate is never scanned unboundedly.
const MAX_DESC_BYTES: usize = MAX_DESC_CHARS * 4;

/// Copies the array. Stops at the first entry whose `types` is null, after `MAX_PORTS` entries, or at once for a
/// null `info`. Types with a null `desc` are skipped.
///
/// # Safety
/// `info` is null or points to a `retro_controller_info` array, valid for the call, that is terminated by an entry
/// with a null `types` or holds at least `MAX_PORTS` entries; every non-null `types` points to `num_types`
/// readable descriptions (or at least `MAX_TYPES`), whose `desc` is null or a NUL-terminated string (or at least
/// `MAX_DESC_BYTES` readable bytes).
pub(crate) unsafe fn copy_controller_info(
    info: *const sys::retro_controller_info,
) -> Vec<Vec<ControllerType>> {
    let mut ports = Vec::new();
    if info.is_null() {
        return ports;
    }
    for port in 0..MAX_PORTS {
        // SAFETY: `port` < MAX_PORTS and no earlier entry was the terminator, so the entry is inside the array
        // per the contract; `retro_controller_info` is plain data (`Copy`, any bit pattern valid).
        let entry = unsafe { info.add(port).read() };
        if entry.types.is_null() {
            break;
        }
        let count = usize::try_from(entry.num_types)
            .unwrap_or(MAX_TYPES)
            .min(MAX_TYPES);
        let mut types = Vec::with_capacity(count);
        for index in 0..count {
            // SAFETY: `index` < min(num_types, MAX_TYPES): inside the `types` array per the contract; plain data.
            let raw = unsafe { entry.types.add(index).read() };
            if raw.desc.is_null() {
                continue;
            }
            // SAFETY: non-null `desc`, NUL-terminated or `MAX_DESC_BYTES` long per the contract.
            let description = unsafe { copy_description(raw.desc) };
            types.push(ControllerType {
                description,
                id: raw.id,
            });
        }
        ports.push(types);
    }
    ports
}

/// Reads at most `MAX_DESC_BYTES` bytes up to the NUL, decodes lossily (invalid UTF-8 becomes U+FFFD), removes
/// control characters and keeps `MAX_DESC_CHARS` characters.
///
/// # Safety
/// `desc` is non-null and readable up to its NUL terminator or for `MAX_DESC_BYTES` bytes, whichever comes first.
unsafe fn copy_description(desc: *const c_char) -> String {
    let mut bytes = Vec::new();
    for offset in 0..MAX_DESC_BYTES {
        // SAFETY: no NUL among the `offset` previous bytes, so `offset` is within the string or the
        // `MAX_DESC_BYTES` readable bytes per the contract.
        let byte = unsafe { desc.add(offset).cast::<u8>().read() };
        if byte == 0 {
            break;
        }
        bytes.push(byte);
    }
    String::from_utf8_lossy(&bytes)
        .chars()
        .filter(|c| !c.is_control())
        .take(MAX_DESC_CHARS)
        .collect()
}

/// Types declared for `port` in `ports` (empty when out of range).
pub(crate) fn types_for(ports: &[Vec<ControllerType>], port: u32) -> &[ControllerType] {
    usize::try_from(port)
        .ok()
        .and_then(|p| ports.get(p))
        .map_or(&[], Vec::as_slice)
}

/// `set_controller_device` rule: NONE and JOYPAD on any of the `MAX_PORTS` ports, otherwise an id declared for it.
pub(crate) fn device_allowed(ports: &[Vec<ControllerType>], port: u32, id: u32) -> bool {
    if usize::try_from(port).map_or(true, |p| p >= MAX_PORTS) {
        return false;
    }
    id == sys::RETRO_DEVICE_NONE
        || id == sys::RETRO_DEVICE_JOYPAD
        || types_for(ports, port).iter().any(|t| t.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CStr;

    fn desc(text: &'static CStr, id: u32) -> sys::retro_controller_description {
        sys::retro_controller_description {
            desc: text.as_ptr(),
            id,
        }
    }

    fn info(types: &[sys::retro_controller_description]) -> sys::retro_controller_info {
        sys::retro_controller_info {
            types: types.as_ptr(),
            num_types: u32::try_from(types.len()).unwrap(),
        }
    }

    const END: sys::retro_controller_info = sys::retro_controller_info {
        types: std::ptr::null(),
        num_types: 0,
    };

    #[test]
    fn null_array_is_empty() {
        // SAFETY: null is allowed by the contract.
        assert!(unsafe { copy_controller_info(std::ptr::null()) }.is_empty());
    }

    #[test]
    fn reads_ports_until_terminator() {
        let pad = [desc(c"Controller", 1), desc(c"DualShock", 517)];
        let none: [sys::retro_controller_description; 0] = [];
        let array = [info(&pad), info(&none), info(&pad), END, info(&pad)];
        // SAFETY: terminated array of valid entries.
        let ports = unsafe { copy_controller_info(array.as_ptr()) };
        assert_eq!(ports.len(), 3);
        assert_eq!(
            ports[0],
            [
                ControllerType {
                    description: "Controller".into(),
                    id: 1
                },
                ControllerType {
                    description: "DualShock".into(),
                    id: 517
                }
            ]
        );
        assert!(ports[1].is_empty());
        assert_eq!(types_for(&ports, 2).len(), 2);
        assert!(types_for(&ports, 3).is_empty());
        assert!(types_for(&ports, u32::MAX).is_empty());
    }

    #[test]
    fn null_types_with_count_is_terminator() {
        let pad = [desc(c"Pad", 1)];
        let array = [
            info(&pad),
            sys::retro_controller_info {
                types: std::ptr::null(),
                num_types: 1000,
            },
        ];
        // SAFETY: terminated array.
        let ports = unsafe { copy_controller_info(array.as_ptr()) };
        assert_eq!(ports.len(), 1);
    }

    #[test]
    fn ports_and_types_are_bounded() {
        let many: Vec<_> = (0..40).map(|i| desc(c"Pad", i)).collect();
        // No terminator: only MAX_PORTS entries may be read (the contract's "at least MAX_PORTS entries").
        let array = [info(&many); MAX_PORTS];
        // SAFETY: MAX_PORTS valid entries, each with 40 valid descriptions.
        let ports = unsafe { copy_controller_info(array.as_ptr()) };
        assert_eq!(ports.len(), MAX_PORTS);
        assert!(ports.iter().all(|p| p.len() == MAX_TYPES));
        assert_eq!(ports[0][MAX_TYPES - 1].id, 31);

        // A huge num_types reads only MAX_TYPES descriptions (all present here).
        let lying = [
            sys::retro_controller_info {
                types: many.as_ptr(),
                num_types: u32::MAX,
            },
            END,
        ];
        // SAFETY: the array behind `types` holds at least MAX_TYPES entries, as the contract allows.
        let ports = unsafe { copy_controller_info(lying.as_ptr()) };
        assert_eq!(ports[0].len(), MAX_TYPES);
    }

    #[test]
    fn null_desc_is_skipped() {
        let types = [
            sys::retro_controller_description {
                desc: std::ptr::null(),
                id: 7,
            },
            desc(c"Pad", 1),
        ];
        let array = [info(&types), END];
        // SAFETY: terminated array; null `desc` is allowed.
        let ports = unsafe { copy_controller_info(array.as_ptr()) };
        assert_eq!(
            ports[0],
            [ControllerType {
                description: "Pad".into(),
                id: 1
            }]
        );
    }

    #[test]
    fn descriptions_are_sanitised() {
        let types = [
            desc(c"Pad\x01\n\tAnalog\x7f", 1),
            desc(c"Caf\xe9 \xff\xfe", 2),
            desc(c"", 3),
        ];
        let long = [b'x'; 300];
        let mut long_nul = long.to_vec();
        long_nul.push(0);
        let long_types = [sys::retro_controller_description {
            desc: long_nul.as_ptr().cast(),
            id: 4,
        }];
        let array = [info(&types), info(&long_types), END];
        // SAFETY: terminated array of NUL-terminated strings.
        let ports = unsafe { copy_controller_info(array.as_ptr()) };
        assert_eq!(ports[0][0].description, "PadAnalog");
        assert_eq!(ports[0][1].description, "Caf\u{fffd} \u{fffd}\u{fffd}");
        assert_eq!(ports[0][2].description, "");
        assert_eq!(ports[1][0].description, "x".repeat(MAX_DESC_CHARS));

        // Unterminated: scanning stops after MAX_DESC_BYTES bytes.
        let unterminated = [b'y'; MAX_DESC_BYTES];
        // SAFETY: MAX_DESC_BYTES readable bytes, as the contract allows.
        let text = unsafe { copy_description(unterminated.as_ptr().cast()) };
        assert_eq!(text, "y".repeat(MAX_DESC_CHARS));

        // Multi-byte characters are counted as characters, not bytes.
        let wide = "é".repeat(100) + "\0";
        // SAFETY: NUL-terminated.
        let text = unsafe { copy_description(wide.as_ptr().cast()) };
        assert_eq!(text, "é".repeat(MAX_DESC_CHARS));
    }

    #[test]
    fn device_rule() {
        let ports = vec![
            vec![ControllerType {
                description: "DualShock".into(),
                id: 517,
            }],
            Vec::new(),
        ];
        assert!(device_allowed(&ports, 0, 517));
        assert!(device_allowed(&ports, 0, sys::RETRO_DEVICE_NONE));
        assert!(device_allowed(&ports, 1, sys::RETRO_DEVICE_JOYPAD));
        assert!(device_allowed(&ports, 7, sys::RETRO_DEVICE_JOYPAD));
        assert!(!device_allowed(&ports, 1, 517));
        assert!(!device_allowed(&ports, 0, 261));
        assert!(!device_allowed(&ports, 8, sys::RETRO_DEVICE_JOYPAD));
        assert!(!device_allowed(&[], u32::MAX, sys::RETRO_DEVICE_NONE));
    }
}
