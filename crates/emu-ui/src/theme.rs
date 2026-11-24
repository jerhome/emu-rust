//! Single source of truth for colours, sizes and spacing (dark theme only, nothing costly: no blur, no shadows,
//! no animation).

use egui::{Color32, Context, CornerRadius, FontId, Stroke, TextStyle};

// Tokens are consumed progressively by the grid, manual and pad screens.
#[allow(dead_code)]
pub(crate) const BG: Color32 = Color32::from_rgb(0x14, 0x17, 0x1d);
pub(crate) const PANEL: Color32 = Color32::from_rgb(0x1b, 0x1f, 0x27);
pub(crate) const CARD: Color32 = Color32::from_rgb(0x23, 0x28, 0x34);
pub(crate) const CARD_HOVER: Color32 = Color32::from_rgb(0x2b, 0x31, 0x40);
pub(crate) const TEXT: Color32 = Color32::from_rgb(0xe6, 0xe9, 0xef);
pub(crate) const TEXT_WEAK: Color32 = Color32::from_rgb(0x9a, 0xa2, 0xb3);
pub(crate) const ACCENT: Color32 = Color32::from_rgb(0x4a, 0xa3, 0xff);
pub(crate) const ACCENT_DIM: Color32 = Color32::from_rgb(0x28, 0x50, 0x7f);
#[allow(dead_code)]
pub(crate) const OK: Color32 = Color32::from_rgb(0x5c, 0xc9, 0x8a);
pub(crate) const WARN: Color32 = Color32::from_rgb(0xe8, 0xb8, 0x4a);
pub(crate) const ERROR: Color32 = Color32::from_rgb(0xff, 0x6e, 0x6e);

#[allow(dead_code)]
pub(crate) const FONT_BODY: f32 = 15.0;
#[allow(dead_code)]
pub(crate) const FONT_SMALL: f32 = 12.0;
#[allow(dead_code)]
pub(crate) const FONT_TITLE: f32 = 22.0;
#[allow(dead_code)]
pub(crate) const FONT_SECTION: f32 = 18.0;

pub(crate) const SP_XS: f32 = 4.0;
pub(crate) const SP_S: f32 = 8.0;
pub(crate) const SP_M: f32 = 12.0;
#[allow(dead_code)]
pub(crate) const SP_L: f32 = 20.0;
pub(crate) const RADIUS: u8 = 6;
pub(crate) const FOCUS_WIDTH: f32 = 2.0;

/// Applies the theme. Focus stays the main signal (keyboard / pad navigation has no pointer to follow).
pub(crate) fn apply(ctx: &Context) {
    ctx.set_visuals(egui::Visuals::dark());
    ctx.all_styles_mut(|style| {
        // Screens appear at once (no fade): snappier with a pad, and `--screen` captures are deterministic.
        style.animation_time = 0.0;
        style.text_styles = [
            (TextStyle::Small, FontId::proportional(FONT_SMALL)),
            (TextStyle::Body, FontId::proportional(FONT_BODY)),
            (TextStyle::Button, FontId::proportional(FONT_BODY)),
            (TextStyle::Heading, FontId::proportional(FONT_TITLE)),
            (TextStyle::Monospace, FontId::monospace(FONT_BODY)),
        ]
        .into();
        let v = &mut style.visuals;
        v.panel_fill = PANEL;
        v.window_fill = PANEL;
        v.extreme_bg_color = BG;
        v.faint_bg_color = CARD;
        v.window_shadow = egui::Shadow::NONE;
        v.popup_shadow = egui::Shadow::NONE;
        v.override_text_color = Some(TEXT);
        v.hyperlink_color = ACCENT;
        v.warn_fg_color = WARN;
        v.error_fg_color = ERROR;
        v.selection.bg_fill = ACCENT_DIM;
        v.selection.stroke = Stroke::new(1.0, ACCENT);
        let radius = CornerRadius::same(RADIUS);
        v.window_corner_radius = radius;
        v.menu_corner_radius = radius;
        for w in [
            &mut v.widgets.noninteractive,
            &mut v.widgets.inactive,
            &mut v.widgets.hovered,
            &mut v.widgets.active,
            &mut v.widgets.open,
        ] {
            w.corner_radius = radius;
        }
        v.widgets.noninteractive.bg_fill = PANEL;
        v.widgets.noninteractive.fg_stroke.color = TEXT;
        v.widgets.inactive.bg_fill = CARD;
        v.widgets.inactive.weak_bg_fill = CARD;
        v.widgets.hovered.bg_fill = CARD_HOVER;
        v.widgets.hovered.weak_bg_fill = CARD_HOVER;
        let focus = &mut v.widgets.active;
        focus.bg_fill = ACCENT_DIM;
        focus.weak_bg_fill = ACCENT_DIM;
        focus.bg_stroke = Stroke::new(FOCUS_WIDTH, ACCENT);
        focus.fg_stroke = Stroke::new(1.5, Color32::WHITE);
    });
}

/// Fill of a game card: `CARD`, brighter when hovered or selected.
pub(crate) fn card_fill(highlight: bool) -> Color32 {
    if highlight { CARD_HOVER } else { CARD }
}

/// Stable, readable colour per system (hash -> hue), used for the placeholder badge.
pub(crate) fn system_color(id: &str) -> Color32 {
    let h = id
        .bytes()
        .fold(7u32, |a, b| a.wrapping_mul(31).wrapping_add(u32::from(b)));
    #[allow(clippy::cast_precision_loss)] // < 360
    let hue = (h % 360) as f32 / 360.0;
    let (r, g, b) = hsv(hue, 0.55, 0.85);
    Color32::from_rgb(r, g, b)
}

fn hsv(h: f32, s: f32, v: f32) -> (u8, u8, u8) {
    let f = |n: f32| {
        let k = (n + h * 6.0) % 6.0;
        v - v * s * k.min(4.0 - k).clamp(0.0, 1.0)
    };
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // 0..=255
    let q = |x: f32| (x * 255.0).round() as u8;
    (q(f(5.0)), q(f(3.0)), q(f(1.0)))
}

/// Round badge (system colour) at `center`.
pub(crate) fn badge(painter: &egui::Painter, center: egui::Pos2, radius: f32, color: Color32) {
    painter.circle_filled(center, radius, color);
}

const ARTICLES: &[&str] = &[
    "the", "a", "an", "le", "la", "les", "l", "un", "une", "der", "die", "das", "el", "los", "las",
    "il",
];

/// Up to two letters for a cover placeholder: skips `(...)`/`[...]` tags and leading articles.
pub(crate) fn initials(title: &str) -> String {
    let mut plain = String::new();
    let mut depth = 0u32;
    for c in title.chars() {
        match c {
            '(' | '[' => depth += 1,
            ')' | ']' => depth = depth.saturating_sub(1),
            _ if depth == 0 => plain.push(c),
            _ => {}
        }
    }
    let mut words: Vec<&str> = plain
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .collect();
    if words.len() > 1 && ARTICLES.contains(&words[0].to_lowercase().as_str()) {
        words.remove(0);
    }
    let out: String = match words.as_slice() {
        [] => String::new(),
        [one] => one.chars().take(2).collect(),
        [a, b, ..] => a.chars().take(1).chain(b.chars().take(1)).collect(),
    };
    if out.is_empty() {
        "?".to_owned()
    } else {
        out.to_uppercase()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lin(c: u8) -> f64 {
        let c = f64::from(c) / 255.0;
        if c <= 0.03928 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    }

    fn luminance(c: Color32) -> f64 {
        0.2126 * lin(c.r()) + 0.7152 * lin(c.g()) + 0.0722 * lin(c.b())
    }

    fn contrast(a: Color32, b: Color32) -> f64 {
        let (la, lb) = (luminance(a), luminance(b));
        (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
    }

    #[test]
    fn text_is_readable_on_backgrounds() {
        for bg in [BG, PANEL, CARD, CARD_HOVER] {
            for fg in [TEXT, TEXT_WEAK, ACCENT, OK, WARN, ERROR] {
                let c = contrast(fg, bg);
                assert!(c >= 4.5, "{fg:?} on {bg:?}: {c:.2}");
            }
            assert!(contrast(TEXT, bg) >= 7.0);
        }
        assert!(contrast(Color32::WHITE, ACCENT_DIM) >= 4.5);
    }

    #[test]
    fn apply_does_not_panic() {
        apply(&Context::default());
    }

    #[test]
    fn initials_cases() {
        assert_eq!(initials("The Legend of Zelda (USA)"), "LO");
        assert_eq!(initials("Super Mario World"), "SM");
        assert_eq!(initials("Tetris"), "TE");
        assert_eq!(initials("L'Aigle d'Or"), "AD");
        assert_eq!(initials("1942 (Europe) [!]"), "19");
        assert_eq!(initials("Out Run 2"), "OR");
        assert_eq!(initials(""), "?");
        assert_eq!(initials("(Demo)"), "?");
        assert_eq!(initials("The"), "TH");
    }

    #[test]
    fn system_color_is_stable_and_varied() {
        assert_eq!(system_color("snes"), system_color("snes"));
        assert_ne!(system_color("snes"), system_color("megadrive"));
        assert!(contrast(system_color("snes"), CARD) >= 3.0);
    }
}
