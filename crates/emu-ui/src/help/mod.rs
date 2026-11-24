//! Built-in manual: `docs/manuel.md` embedded at build time, parsed once, shown as a page.

mod markdown;

use std::sync::OnceLock;

use egui::text::{LayoutJob, TextFormat};
use egui::{FontFamily, FontId, Key, Modifiers, RichText, ScrollArea, Ui};

use crate::theme;
use markdown::{Block, Span};

pub(crate) mod text {
    pub const BUTTON: &str = "Aide";
    pub const CLOSE: &str = "Fermer";
    pub const CONTENTS: &str = "Sommaire";
    pub const KEYS_HINT: &str = "Flèches Haut/Bas : défiler · Gauche/Droite : changer de section · Tab : sommaire · Échap : fermer";
}

const MANUAL: &str = include_str!("../../../../docs/manuel.md");
const SCROLL_STEP: f32 = 60.0;
const PAGE_STEP: f32 = 420.0;

pub(crate) struct Section {
    pub title: String,
    pub anchor: String,
    /// Anchors of the `###` titles inside the section.
    pub sub_anchors: Vec<String>,
    pub blocks: Vec<Block>,
}

pub(crate) struct Doc {
    pub sections: Vec<Section>,
}

impl Doc {
    /// Splits at `##` titles; what precedes the first one (the `#` title and intro) becomes "Introduction".
    pub(crate) fn parse(src: &str) -> Self {
        let mut sections = vec![Section {
            title: "Introduction".into(),
            anchor: "introduction".into(),
            sub_anchors: Vec::new(),
            blocks: Vec::new(),
        }];
        for block in markdown::parse(src) {
            match &block {
                Block::Heading { level: 2, spans } => {
                    let title = markdown::plain(spans);
                    sections.push(Section {
                        anchor: markdown::anchor(&title),
                        title,
                        sub_anchors: Vec::new(),
                        blocks: Vec::new(),
                    });
                    continue;
                }
                Block::Heading { level: 3, spans } => {
                    if let Some(last) = sections.last_mut() {
                        last.sub_anchors
                            .push(markdown::anchor(&markdown::plain(spans)));
                    }
                }
                _ => {}
            }
            if let Some(last) = sections.last_mut() {
                last.blocks.push(block);
            }
        }
        if sections[0].blocks.is_empty() {
            sections.remove(0);
        }
        Self { sections }
    }

    /// Section by anchor (`"clavier"`), by `###` anchor, or by its number prefix ("4").
    pub(crate) fn find(&self, key: &str) -> Option<usize> {
        let key = key.trim().to_lowercase();
        self.sections
            .iter()
            .position(|s| s.anchor == key)
            .or_else(|| {
                self.sections
                    .iter()
                    .position(|s| s.sub_anchors.contains(&key))
            })
            .or_else(|| {
                let n: usize = key.parse().ok()?;
                self.sections
                    .iter()
                    .position(|s| s.title.starts_with(&format!("{n}.")))
            })
    }
}

/// The embedded manual, parsed on first use.
pub(crate) fn manual() -> &'static Doc {
    static DOC: OnceLock<Doc> = OnceLock::new();
    DOC.get_or_init(|| Doc::parse(MANUAL))
}

#[derive(Debug, Default)]
pub(crate) struct HelpState {
    pub section: usize,
    scroll: f32,
}

impl HelpState {
    /// Jumps to the section named by `anchor` (unknown or `None`: first section).
    pub(crate) fn open(&mut self, anchor: Option<&str>) {
        self.section = anchor.and_then(|a| manual().find(a)).unwrap_or(0);
        self.scroll = 0.0;
    }

    fn select(&mut self, index: usize) {
        if index != self.section {
            self.section = index;
            self.scroll = 0.0;
        }
    }
}

/// True for the key presses that open the manual (F1, `?` typed, Ctrl+H). Never Cmd+H (macOS hides the app).
/// Call only when no text field has the focus and no dialog is open.
pub(crate) fn open_requested(ui: &Ui) -> bool {
    ui.input_mut(|i| {
        let question = i
            .events
            .iter()
            .any(|e| matches!(e, egui::Event::Text(t) if t == "?"));
        question
            || i.consume_key(Modifiers::NONE, Key::F1)
            || i.consume_key(Modifiers::CTRL, Key::H)
    })
}

/// One frame of the manual page. Returns `true` when the user closes it.
pub(crate) fn show(ui: &mut Ui, doc: &Doc, state: &mut HelpState) -> bool {
    let mut close = false;
    let last = doc.sections.len().saturating_sub(1);
    state.section = state.section.min(last);

    let (esc, left, right, up, down, pgup, pgdn) = ui.input_mut(|i| {
        (
            i.consume_key(Modifiers::NONE, Key::Escape),
            i.consume_key(Modifiers::NONE, Key::ArrowLeft),
            i.consume_key(Modifiers::NONE, Key::ArrowRight),
            i.consume_key(Modifiers::NONE, Key::ArrowUp),
            i.consume_key(Modifiers::NONE, Key::ArrowDown),
            i.consume_key(Modifiers::NONE, Key::PageUp),
            i.consume_key(Modifiers::NONE, Key::PageDown),
        )
    });
    close |= esc;
    if left && state.section > 0 {
        state.select(state.section - 1);
    }
    if right && state.section < last {
        state.select(state.section + 1);
    }
    if up {
        state.scroll = (state.scroll - SCROLL_STEP).max(0.0);
    }
    if down {
        state.scroll += SCROLL_STEP;
    }
    if pgup {
        state.scroll = (state.scroll - PAGE_STEP).max(0.0);
    }
    if pgdn {
        state.scroll += PAGE_STEP;
    }

    egui::Panel::left("help-toc")
        .exact_size(220.0)
        .frame(
            egui::Frame::NONE
                .fill(theme::PANEL)
                .inner_margin(theme::SP_M),
        )
        .show(ui, |ui| {
            ui.label(
                RichText::new(text::CONTENTS)
                    .size(theme::FONT_SECTION)
                    .color(theme::TEXT),
            );
            ui.add_space(theme::SP_S);
            for (i, s) in doc.sections.iter().enumerate() {
                let r = ui.selectable_label(i == state.section, &s.title);
                if r.clicked() {
                    state.select(i);
                }
            }
            ui.add_space(theme::SP_L);
            if ui.button(text::CLOSE).clicked() {
                close = true;
            }
        });

    egui::CentralPanel::default()
        .frame(egui::Frame::NONE.fill(theme::BG).inner_margin(theme::SP_L))
        .show(ui, |ui| {
            ui.label(
                RichText::new(text::KEYS_HINT)
                    .size(theme::FONT_SMALL)
                    .color(theme::TEXT_WEAK),
            );
            ui.add_space(theme::SP_S);
            let out = ScrollArea::vertical()
                .id_salt("help-scroll")
                .auto_shrink([false, false])
                .vertical_scroll_offset(state.scroll)
                .show(ui, |ui| {
                    ui.set_max_width(ui.available_width().min(760.0));
                    if let Some(section) = doc.sections.get(state.section) {
                        render_blocks(ui, &section.blocks);
                    }
                });
            state.scroll = out.state.clamped_offset().y;
        });
    close
}

fn body_format(color: egui::Color32, bold: bool, code: bool) -> TextFormat {
    let (family, size) = if code {
        (FontFamily::Monospace, theme::FONT_BODY - 1.0)
    } else {
        (FontFamily::Proportional, theme::FONT_BODY)
    };
    TextFormat {
        font_id: FontId::new(size, family),
        color: if bold { egui::Color32::WHITE } else { color },
        background: if code {
            theme::CARD
        } else {
            egui::Color32::TRANSPARENT
        },
        ..Default::default()
    }
}

fn job(
    ui: &Ui,
    prefix: &str,
    spans: &[Span],
    size_color: Option<(f32, egui::Color32)>,
) -> LayoutJob {
    let mut job = LayoutJob::default();
    job.wrap.max_width = ui.available_width();
    let base = |bold, code| match size_color {
        Some((size, color)) => {
            let mut f = body_format(color, bold, code);
            if !code {
                f.font_id.size = size;
            }
            f
        }
        None => body_format(theme::TEXT, bold, code),
    };
    if !prefix.is_empty() {
        job.append(prefix, 0.0, base(false, false));
    }
    for s in spans {
        match s {
            Span::Text(t) => job.append(t, 0.0, base(false, false)),
            Span::Bold(t) => job.append(t, 0.0, base(true, false)),
            Span::Code(t) => job.append(t, 0.0, base(false, true)),
        }
    }
    job
}

fn render_blocks(ui: &mut Ui, blocks: &[Block]) {
    for block in blocks {
        match block {
            Block::Heading { level, spans } => {
                ui.add_space(theme::SP_M);
                let (size, color) = match level {
                    1 | 2 => (theme::FONT_TITLE, theme::TEXT),
                    _ => (theme::FONT_SECTION, theme::ACCENT),
                };
                ui.label(job(ui, "", spans, Some((size, color))));
                ui.add_space(theme::SP_XS);
            }
            Block::Paragraph(spans) => {
                ui.label(job(ui, "", spans, None));
                ui.add_space(theme::SP_S);
            }
            Block::List { ordered, items } => {
                for (n, item) in items.iter().enumerate() {
                    let marker = if *ordered {
                        format!("{}.  ", n + 1)
                    } else {
                        "•  ".to_string()
                    };
                    ui.label(job(ui, &marker, item, None));
                }
                ui.add_space(theme::SP_S);
            }
            Block::Code(code) => {
                egui::Frame::NONE
                    .fill(theme::CARD)
                    .corner_radius(theme::RADIUS)
                    .inner_margin(theme::SP_S)
                    .show(ui, |ui| {
                        ui.label(RichText::new(code).monospace().color(theme::TEXT));
                    });
                ui.add_space(theme::SP_S);
            }
            Block::Table { header, rows } => {
                let id = ui.next_auto_id();
                egui::Grid::new(id)
                    .striped(true)
                    .spacing([theme::SP_L, theme::SP_XS])
                    .show(ui, |ui| {
                        for cell in header {
                            ui.label(job(ui, "", cell, Some((theme::FONT_BODY, theme::ACCENT))));
                        }
                        ui.end_row();
                        for row in rows {
                            for cell in row {
                                ui.label(job(ui, "", cell, None));
                            }
                            ui.end_row();
                        }
                    });
                ui.add_space(theme::SP_S);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manual_parses_entirely() {
        let doc = manual();
        assert!(doc.sections.len() >= 7, "sections : {}", doc.sections.len());
        assert!(doc.sections.iter().all(|s| !s.blocks.is_empty()));
        assert!(doc.sections.iter().all(|s| !s.anchor.is_empty()));
        // No raw Markdown markers left over from a parser gap.
        let has_table = doc
            .sections
            .iter()
            .flat_map(|s| &s.blocks)
            .any(|b| matches!(b, Block::Table { .. }));
        assert!(has_table);
        for s in &doc.sections {
            for b in &s.blocks {
                if let Block::Paragraph(spans) = b {
                    let p = markdown::plain(spans);
                    assert!(
                        !p.starts_with('|') && !p.starts_with("- "),
                        "non analysé : {p}"
                    );
                }
            }
        }
    }

    #[test]
    fn anchors_resolve() {
        let doc = manual();
        assert_eq!(
            doc.sections[doc.find("clavier").unwrap()].title,
            "4. Clavier"
        );
        assert_eq!(doc.sections[doc.find("4").unwrap()].title, "4. Clavier");
        assert!(doc.find("la-touche-hote-systemes-ordinateur").is_some());
        assert!(doc.find("inconnu").is_none());
    }

    #[test]
    fn open_unknown_goes_to_first() {
        let mut s = HelpState::default();
        s.open(Some("clavier"));
        assert!(s.section > 0);
        s.open(Some("nope"));
        assert_eq!(s.section, 0);
        s.open(None);
        assert_eq!(s.section, 0);
    }

    #[test]
    fn empty_doc_is_safe() {
        let doc = Doc::parse("");
        assert!(doc.sections.is_empty());
        assert!(doc.find("x").is_none());
    }

    #[test]
    fn renders_without_panic_and_escape_closes() {
        let ctx = egui::Context::default();
        let doc = manual();
        let mut state = HelpState::default();
        let mut closed = false;
        let mut input = egui::RawInput::default();
        input.events.push(egui::Event::Key {
            key: Key::Escape,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::NONE,
        });
        ctx.run_ui(input, |ui| closed = show(ui, doc, &mut state))
            .textures_delta
            .clear();
        assert!(closed);
        ctx.run_ui(egui::RawInput::default(), |ui| {
            show(ui, doc, &mut state);
        })
        .textures_delta
        .clear();
    }
}
