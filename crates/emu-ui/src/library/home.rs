//! Home screen: top bar (search, views, sort, system filters, actions), cover grid, status bar.
//! The grid is one focusable widget: arrows / Page / Home / End move the selection, Enter plays, F toggles the
//! favorite. Tab leaves it for the bar; `/` or Ctrl+F jumps to the search field, Échap clears the search.

use egui::{
    Align2, Color32, CornerRadius, EventFilter, FontId, Id, Key, Modifiers, Rect, ScrollArea,
    Sense, Stroke, StrokeKind, TextEdit, Ui, Vec2, pos2, vec2,
};
use emu_library::{SortOrder, systems};

use super::grid::{self, Nav};
use super::{Focus, LibraryEvent, LibraryUi, Page, View, text};

const CARD_W: f32 = 168.0;
const COVER_H: f32 = 210.0;
const TITLE_H: f32 = 44.0;
const CARD_H: f32 = COVER_H + TITLE_H;
const GAP: f32 = 12.0;
const FOCUS: Color32 = Color32::from_rgb(150, 200, 255);

/// Deterministic, muted colour per system for placeholder cards.
fn system_color(id: &str) -> Color32 {
    let h = id
        .bytes()
        .fold(7u32, |a, b| a.wrapping_mul(31).wrapping_add(u32::from(b)));
    let [r, g, b, _] = h.to_le_bytes();
    Color32::from_rgb(40 + r % 50, 40 + g % 50, 50 + b % 60)
}

impl LibraryUi {
    pub(super) fn home(&mut self, ui: &mut Ui) {
        let dialog_open = self.dialog.is_some();
        let mut refilter = false;
        let search_id = Id::unique("library-search");

        egui::Panel::top("library-top").show(ui, |ui| {
            ui.add_space(4.0);
            ui.horizontal_wrapped(|ui| {
                let search = ui.add(
                    TextEdit::singleline(&mut self.filter.search)
                        .id(search_id)
                        .hint_text(text::SEARCH_HINT)
                        .desired_width(240.0),
                );
                if self.focus == Some(Focus::Search) {
                    search.request_focus();
                    self.focus = None;
                }
                if search.changed() {
                    refilter = true;
                }
                if search.lost_focus()
                    && ui.input(|i| i.key_pressed(Key::Enter) || i.key_pressed(Key::ArrowDown))
                {
                    self.focus = Some(Focus::Grid);
                }
                ui.separator();
                for (view, label) in [
                    (View::All, text::VIEW_ALL),
                    (View::Favorites, text::VIEW_FAVORITES),
                    (View::Recent, text::VIEW_RECENT),
                ] {
                    if ui.selectable_label(self.view == view, label).clicked() {
                        self.view = view;
                        refilter = true;
                    }
                }
                let sort = if self.filter.sort == SortOrder::Title {
                    text::SORT_TITLE
                } else {
                    text::SORT_LAST_PLAYED
                };
                if ui.button(sort).clicked() {
                    self.filter.sort = match self.filter.sort {
                        SortOrder::Title => SortOrder::LastPlayed,
                        SortOrder::LastPlayed => SortOrder::Title,
                    };
                    refilter = true;
                }
                ui.separator();
                let scanning = self.scan.is_some();
                let scan_label = if scanning {
                    text::SCAN_CANCEL
                } else {
                    text::SCAN
                };
                if ui
                    .add_enabled(
                        self.db.is_some() && !self.capture,
                        egui::Button::new(scan_label),
                    )
                    .clicked()
                {
                    if let Some(t) = &self.scan {
                        t.cancel();
                    } else {
                        self.start_scan(ui.ctx());
                    }
                }
                if ui.button(text::CORES).clicked() {
                    self.set_page(Page::Cores);
                }
                if ui.button(text::SETTINGS).clicked() {
                    self.set_page(Page::Settings);
                }
                if ui.button(text::QUIT).clicked() {
                    self.events.push(LibraryEvent::Quit);
                }
            });
            ui.horizontal_wrapped(|ui| {
                let all = text::system_count(text::ALL_SYSTEMS, self.games.len());
                if ui
                    .selectable_label(self.filter.system.is_none(), all)
                    .clicked()
                {
                    self.filter.system = None;
                    refilter = true;
                }
                for (id, n) in grid::system_counts(&self.games, systems()) {
                    let name = emu_library::system(&id).map_or(id.as_str(), |s| &s.name);
                    let label = text::system_count(name, n);
                    let on = self.filter.system.as_deref() == Some(id.as_str());
                    if ui.selectable_label(on, label).clicked() {
                        self.filter.system = if on { None } else { Some(id.clone()) };
                        refilter = true;
                    }
                }
            });
            ui.add_space(2.0);
        });

        egui::Panel::bottom("library-status").show(ui, |ui| {
            ui.add_space(2.0);
            self.status_line(ui);
            if !self.has_dat && !self.games.is_empty() {
                super::weak(ui, text::NO_DAT);
            }
            if !self.config.covers.download && !self.capture {
                ui.horizontal(|ui| {
                    super::weak(ui, text::COVERS_DISABLED);
                    if ui.small_button(text::COVERS_ENABLE).clicked() {
                        self.enable_cover_downloads();
                    }
                });
            } else if let Some(status) = self.covers.status() {
                super::weak(ui, status);
            }
            super::weak(ui, text::HOME_HINT);
        });

        // Keys that work wherever the focus is (not while typing, not under a dialog).
        if !dialog_open {
            let typing = ui.ctx().text_edit_focused();
            let (slash, ctrl_f, escape) = ui.input_mut(|i| {
                (
                    !typing && i.consume_key(Modifiers::NONE, Key::Slash),
                    i.consume_key(Modifiers::COMMAND, Key::F),
                    i.key_pressed(Key::Escape),
                )
            });
            if slash || ctrl_f {
                self.focus = Some(Focus::Search);
            }
            if escape && !self.filter.search.is_empty() {
                self.filter.search.clear();
                self.focus = Some(Focus::Grid);
                refilter = true;
            }
        }
        if refilter {
            let keep = self.selected_game().map(|g| g.id);
            self.refilter(keep);
        }

        egui::CentralPanel::default().show(ui, |ui| self.grid(ui, dialog_open));
    }

    fn grid(&mut self, ui: &mut Ui, dialog_open: bool) {
        if self.visible.is_empty() {
            ui.add_space(24.0);
            ui.vertical_centered(|ui| {
                if self.games.is_empty() {
                    ui.heading(text::EMPTY_LIBRARY);
                    ui.label(text::empty_hint(&self.dirs.root.join("roms")));
                } else {
                    ui.heading(text::NO_MATCH);
                }
            });
            return;
        }
        let width = ui.available_width();
        self.columns = grid::columns_for(width, CARD_W, GAP);
        let rows = self.visible.len().div_ceil(self.columns);
        let row_step = CARD_H + ui.spacing().item_spacing.y;

        // Focus widget over the whole grid; it never takes clicks (cards do).
        let grid_id = Id::unique("library-grid-focus");
        let focus_resp = ui.interact(
            ui.available_rect_before_wrap(),
            grid_id,
            Sense::focusable_noninteractive(),
        );
        if self.focus == Some(Focus::Grid) && !dialog_open {
            focus_resp.request_focus();
            self.focus = None;
        }
        let nothing_focused = ui.memory(|m| m.focused().is_none());
        let focused = focus_resp.has_focus();
        if focused {
            ui.memory_mut(|m| {
                m.set_focus_lock_filter(
                    grid_id,
                    EventFilter {
                        horizontal_arrows: true,
                        vertical_arrows: true,
                        ..EventFilter::default()
                    },
                );
            });
        }
        if !dialog_open && (focused || nothing_focused) {
            let page = self.page_rows;
            let (navs, enter, fav) = ui.input(|i| {
                let mut navs = Vec::new();
                for (key, nav) in [
                    (Key::ArrowLeft, Nav::Left),
                    (Key::ArrowRight, Nav::Right),
                    (Key::ArrowUp, Nav::Up),
                    (Key::ArrowDown, Nav::Down),
                    (Key::PageUp, Nav::PageUp),
                    (Key::PageDown, Nav::PageDown),
                    (Key::Home, Nav::Home),
                    (Key::End, Nav::End),
                ] {
                    if i.key_pressed(key) {
                        navs.push(nav);
                    }
                }
                (
                    navs,
                    focused && (i.key_pressed(Key::Enter) || i.key_pressed(Key::Space)),
                    focused && i.key_pressed(Key::F) && i.modifiers.is_none(),
                )
            });
            if !navs.is_empty() && !focused {
                focus_resp.request_focus();
            }
            for nav in navs {
                self.selected =
                    grid::step(self.selected, self.visible.len(), self.columns, page, nav);
                self.scroll_to_selected = true;
            }
            if enter {
                self.launch_selected();
            }
            if fav {
                self.toggle_favorite();
            }
        }

        let mut area = ScrollArea::vertical()
            .id_salt("library-grid")
            .auto_shrink([false, false]);
        if std::mem::take(&mut self.scroll_to_selected) && self.grid_height > 0.0 {
            #[allow(clippy::cast_precision_loss)] // row counts are small
            let top = (self.selected / self.columns) as f32 * row_step;
            let mut offset = self.grid_offset;
            if top < offset {
                offset = top;
            } else if top + CARD_H > offset + self.grid_height {
                offset = top + CARD_H - self.grid_height;
            }
            area = area.vertical_scroll_offset(offset.max(0.0));
        }
        let mut clicked = None;
        let mut double = false;
        let mut on_screen = Vec::new();
        let out = area.show_rows(ui, CARD_H, rows, |ui, range| {
            for row in range {
                let (row_rect, _) =
                    ui.allocate_exact_size(vec2(ui.available_width(), CARD_H), Sense::hover());
                for col in 0..self.columns {
                    let pos = row * self.columns + col;
                    let Some(&gi) = self.visible.get(pos) else {
                        break;
                    };
                    #[allow(clippy::cast_precision_loss)]
                    let x = row_rect.left() + col as f32 * (CARD_W + GAP);
                    let rect = Rect::from_min_size(pos2(x, row_rect.top()), vec2(CARD_W, CARD_H));
                    let game = &self.games[gi];
                    on_screen.push(game.id);
                    let resp =
                        ui.interact(rect, Id::unique(("library-card", game.id)), Sense::click());
                    if resp.clicked() {
                        clicked = Some(pos);
                    }
                    if resp.double_clicked() {
                        clicked = Some(pos);
                        double = true;
                    }
                    let texture = self.covers.texture(ui.ctx(), game);
                    paint_card(
                        ui,
                        rect,
                        game,
                        texture.as_ref(),
                        pos == self.selected,
                        focused,
                    );
                }
            }
        });
        self.grid_offset = out.state.clamped_offset().y;
        self.grid_height = out.inner_rect.height();
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // small positive
        {
            self.page_rows = ((self.grid_height / row_step).floor() as usize).max(1);
        }
        self.covers.visible(&on_screen);
        if let Some(pos) = clicked {
            self.selected = pos;
            focus_resp.request_focus();
            if double {
                self.launch_selected();
            }
        }
    }
}

fn paint_card(
    ui: &Ui,
    rect: Rect,
    game: &emu_library::Game,
    texture: Option<&egui::TextureHandle>,
    selected: bool,
    focused: bool,
) {
    let painter = ui.painter_at(rect.expand(4.0));
    let cover = Rect::from_min_size(rect.min, vec2(rect.width(), COVER_H));
    let radius = CornerRadius::same(6);
    match texture {
        Some(t) => {
            painter.rect_filled(cover, radius, Color32::from_gray(24));
            painter.image(
                t.id(),
                fit_rect(cover, t.size_vec2()),
                Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                Color32::WHITE,
            );
        }
        None => {
            painter.rect_filled(cover, radius, system_color(&game.system));
            let system =
                emu_library::system(&game.system).map_or(game.system.as_str(), |s| &s.name);
            let galley = painter.layout(
                system.to_owned(),
                FontId::proportional(15.0),
                Color32::from_white_alpha(200),
                cover.width() - 16.0,
            );
            let pos = cover.center() - galley.size() / 2.0;
            painter.galley(pos, galley, Color32::WHITE);
        }
    }
    if game.favorite {
        painter.text(
            cover.right_top() + Vec2::new(-8.0, 6.0),
            Align2::RIGHT_TOP,
            text::FAVORITE_MARK,
            FontId::proportional(18.0),
            Color32::from_rgb(255, 210, 80),
        );
    }
    let mut job = egui::text::LayoutJob::simple(
        text::game_title(&game.title),
        FontId::proportional(13.0),
        ui.visuals().text_color(),
        rect.width(),
    );
    job.wrap.max_rows = 2;
    job.halign = egui::Align::Center;
    let galley = painter.layout_job(job);
    painter.galley(
        pos2(rect.center().x, cover.bottom() + 4.0),
        galley,
        Color32::WHITE,
    );
    if selected {
        let stroke = if focused {
            Stroke::new(3.0, FOCUS)
        } else {
            Stroke::new(2.0, Color32::from_gray(150))
        };
        painter.rect_stroke(rect.expand(2.0), radius, stroke, StrokeKind::Outside);
    }
}

/// Largest rectangle of `size`'s aspect ratio centred in `frame`.
pub(super) fn fit_rect(frame: Rect, size: Vec2) -> Rect {
    if size.x <= 0.0 || size.y <= 0.0 {
        return frame;
    }
    let scale = (frame.width() / size.x).min(frame.height() / size.y);
    Rect::from_center_size(frame.center(), size * scale)
}
