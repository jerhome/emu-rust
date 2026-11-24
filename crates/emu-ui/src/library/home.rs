//! Home screen: top bar (search, views, sort, system filters, actions), cover grid, status bar.
//! The grid is one focusable widget: arrows / Page / Home / End move the selection, Enter plays, F toggles the
//! favorite. Tab leaves it for the bar; `/` or Ctrl+F jumps to the search field, Échap clears the search.

use crate::theme;
use egui::{
    Align2, Color32, CornerRadius, EventFilter, FontId, Id, Key, Modifiers, Rect, RichText,
    ScrollArea, Sense, Stroke, StrokeKind, TextEdit, Ui, Vec2, pos2, vec2,
};
use emu_library::{SortOrder, systems};

use super::grid::{self, Nav};
use super::{Focus, LibraryEvent, LibraryUi, Page, View, text};

const CARD_W: f32 = 168.0;
const COVER_H: f32 = 210.0;
const TITLE_H: f32 = 44.0;
const CARD_H: f32 = COVER_H + TITLE_H;
const GAP: f32 = theme::SP_M;
impl LibraryUi {
    pub(super) fn home(&mut self, ui: &mut Ui) {
        let dialog_open = self.dialog.is_some()
            || self.cores.import.is_active()
            || self.snapshot_busy()
            || self.programs_open();
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
                if ui.button(text::OPEN_ROM_FOLDER).clicked() {
                    self.open_rom_folder();
                }
                if ui.button(text::DESKTOP_BUTTON).clicked() {
                    self.launch_desktop("atarist");
                }
                if ui.button(text::AMIGA_DESKTOP_BUTTON).clicked() {
                    self.launch_desktop("amiga");
                }
                if ui.button(text::CPC_BASIC_BUTTON).clicked() {
                    self.launch_desktop("cpc");
                }
                if ui.button(text::C64_BASIC_BUTTON).clicked() {
                    self.launch_desktop("c64");
                }
                if ui.button(text::CORES).clicked() {
                    self.set_page(Page::Cores);
                }
                if ui.button(crate::help::text::BUTTON).clicked() {
                    self.open_help(None);
                }
                if ui.button(text::SETTINGS).clicked() {
                    self.set_page(Page::Settings);
                }
                if ui.button(text::QUIT).clicked() {
                    self.events.push(LibraryEvent::Quit);
                }
            });
            ui.add_space(2.0);
        });

        let firmware = self.firmware_banner();
        egui::Panel::bottom("library-status").show(ui, |ui| {
            ui.add_space(2.0);
            if let Some(banner) = firmware {
                ui.horizontal_wrapped(|ui| {
                    let color = if banner.required {
                        super::ERROR
                    } else {
                        super::WARNING
                    };
                    ui.colored_label(color, &banner.text);
                    if ui
                        .add_enabled(!dialog_open, egui::Button::new(text::FIRMWARE_IMPORT))
                        .clicked()
                    {
                        self.cores.import.start(Some(banner.wanted));
                    }
                });
            }
            self.pad_notice_line(ui, !dialog_open);
            self.status_line(ui);
            if !self.unclassified.is_empty() && ui.small_button(text::UNCLASSIFIED_SHOW).clicked() {
                let list = self.unclassified.clone();
                self.open_dialog(super::Dialog::Unclassified(list));
            }
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
            if self.selected_game().is_some_and(|g| g.is_dir) {
                super::weak(ui, text::DOS_FOLDER_HINT);
            }
            super::weak(ui, text::footer());
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

        egui::Panel::left("library-systems")
            .exact_size(180.0)
            .resizable(false)
            .frame(
                egui::Frame::new()
                    .fill(theme::PANEL)
                    .inner_margin(theme::SP_S),
            )
            .show(ui, |ui| {
                ScrollArea::vertical()
                    .id_salt("library-systems-scroll")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = theme::SP_XS;
                        let all = self.games.len();
                        if system_entry(
                            ui,
                            self.filter.system.is_none(),
                            text::ALL_SYSTEMS,
                            all,
                            None,
                        )
                        .clicked()
                        {
                            self.filter.system = None;
                            refilter = true;
                        }
                        for (id, n) in grid::system_counts(&self.games, systems()) {
                            let name = emu_library::system(&id).map_or(id.as_str(), |s| &s.name);
                            let on = self.filter.system.as_deref() == Some(id.as_str());
                            if system_entry(ui, on, name, n, Some(theme::system_color(&id)))
                                .clicked()
                            {
                                self.filter.system = if on { None } else { Some(id.clone()) };
                                refilter = true;
                            }
                        }
                    });
            });

        egui::CentralPanel::default().show(ui, |ui| self.grid(ui, dialog_open));
    }

    /// Nothing to show: one short sentence + the action that fixes it, as buttons (keyboard reachable).
    fn empty_state(&mut self, ui: &mut Ui) {
        let filtered = !self.filter.search.is_empty() || self.filter.system.is_some();
        let (title, hint) = if self.games.is_empty() {
            if self.scan.is_some() {
                (text::EMPTY_LIBRARY, String::new())
            } else if self.config.library.roots.is_empty() {
                (
                    text::EMPTY_LIBRARY,
                    text::empty_hint(&self.dirs.root.join("roms")),
                )
            } else {
                (text::EMPTY_NO_FILES, text::EMPTY_NO_FILES_HINT.to_owned())
            }
        } else if filtered {
            (text::NO_MATCH, text::NO_MATCH_HINT.to_owned())
        } else if self.view == View::Favorites {
            (text::EMPTY_FAVORITES, text::EMPTY_FAVORITES_HINT.to_owned())
        } else {
            (text::EMPTY_RECENT, text::EMPTY_RECENT_HINT.to_owned())
        };
        let no_games = self.games.is_empty();
        let scanning = self.scan.is_some();
        let (mut clear, mut settings, mut scan, mut cores) = (false, false, false, false);
        ui.add_space(theme::SP_L * 2.0);
        ui.vertical_centered(|ui| {
            ui.label(
                RichText::new(title)
                    .size(theme::FONT_SECTION)
                    .color(theme::TEXT),
            );
            if scanning && no_games {
                ui.label(RichText::new(text::scan_running(0, 0)).color(theme::TEXT_WEAK));
            } else {
                ui.label(RichText::new(hint).color(theme::TEXT_WEAK));
                if no_games {
                    ui.label(RichText::new(text::EMPTY_CORES_HINT).color(theme::TEXT_WEAK));
                }
            }
            ui.add_space(theme::SP_M);
            ui.horizontal(|ui| {
                // Centre the button row without a fixed width.
                ui.add_space((ui.available_width() / 2.0 - 160.0).max(0.0));
                if no_games {
                    if self.config.library.roots.is_empty() {
                        settings = ui.button(text::EMPTY_ADD_FOLDER).clicked();
                    }
                    let label = if self.config.library.roots.is_empty() {
                        text::EMPTY_SCAN
                    } else {
                        text::EMPTY_RESCAN
                    };
                    scan = ui
                        .add_enabled(!scanning, egui::Button::new(label))
                        .clicked();
                    cores = ui.button(text::EMPTY_CORES_BUTTON).clicked();
                } else if filtered {
                    clear = ui.button(text::NO_MATCH_CLEAR).clicked();
                }
            });
        });
        if clear {
            self.filter.search.clear();
            self.filter.system = None;
            self.refilter(None);
        }
        if settings {
            self.set_page(Page::Settings);
        }
        if scan {
            self.start_scan(ui.ctx());
        }
        if cores {
            self.set_page(Page::Cores);
        }
    }

    fn grid(&mut self, ui: &mut Ui, dialog_open: bool) {
        if self.visible.is_empty() {
            self.empty_state(ui);
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
            let (navs, enter, fav, programs, reset) = ui.input(|i| {
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
                    focused && i.key_pressed(Key::P) && i.modifiers.is_none(),
                    focused && i.key_pressed(Key::R) && i.modifiers.is_none(),
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
            if programs {
                self.open_programs();
            }
            if reset {
                self.reset_selected_cover();
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
        let mut reset_pos = None;
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
                    resp.context_menu(|ui| {
                        if ui.button(text::COVER_RESET).clicked() {
                            reset_pos = Some(pos);
                            ui.close();
                        }
                    });
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
        if let Some(pos) = reset_pos {
            self.selected = pos;
            self.reset_selected_cover();
        }
        if let Some(pos) = clicked {
            self.selected = pos;
            focus_resp.request_focus();
            if double {
                self.launch_selected();
            }
        }
    }
}

/// One row of the systems column: name left, count right; focusable, so reachable with Tab.
fn system_entry(
    ui: &mut Ui,
    selected: bool,
    name: &str,
    count: usize,
    color: Option<Color32>,
) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(
        vec2(ui.available_width(), theme::FONT_BODY + 2.0 * theme::SP_S),
        Sense::click(),
    );
    let painter = ui.painter_at(rect.expand(theme::FOCUS_WIDTH));
    let radius = CornerRadius::same(theme::RADIUS);
    if selected {
        painter.rect_filled(rect, radius, theme::ACCENT_DIM);
    } else if resp.hovered() {
        painter.rect_filled(rect, radius, theme::CARD_HOVER);
    }
    if resp.has_focus() {
        painter.rect_stroke(
            rect,
            radius,
            Stroke::new(theme::FOCUS_WIDTH, theme::ACCENT),
            StrokeKind::Inside,
        );
    }
    let mid = rect.left_center() + vec2(theme::SP_S + 4.0, 0.0);
    if let Some(color) = color {
        theme::badge(&painter, mid, 4.0, color);
    }
    let fg = if selected {
        Color32::WHITE
    } else {
        theme::TEXT
    };
    let count_galley = painter.layout_no_wrap(
        count.to_string(),
        FontId::proportional(theme::FONT_SMALL),
        theme::TEXT_WEAK,
    );
    let count_w = count_galley.size().x;
    let name_x = rect.left() + theme::SP_S + 14.0;
    let mut job = egui::text::LayoutJob::simple(
        name.to_owned(),
        FontId::proportional(theme::FONT_BODY),
        fg,
        (rect.right() - theme::SP_S - count_w - theme::SP_XS - name_x).max(10.0),
    );
    job.wrap.max_rows = 1;
    let name_galley = painter.layout_job(job);
    painter.galley(
        pos2(name_x, mid.y - name_galley.size().y / 2.0),
        name_galley,
        fg,
    );
    painter.galley(
        pos2(
            rect.right() - theme::SP_S - count_w,
            mid.y - count_galley.size().y / 2.0,
        ),
        count_galley,
        theme::TEXT_WEAK,
    );
    resp
}

fn paint_card(
    ui: &Ui,
    rect: Rect,
    game: &emu_library::Game,
    texture: Option<&egui::TextureHandle>,
    selected: bool,
    focused: bool,
) {
    let painter = ui.painter_at(rect.expand(theme::FOCUS_WIDTH + 2.0));
    let cover = Rect::from_min_size(rect.min, vec2(rect.width(), COVER_H));
    let radius = CornerRadius::same(theme::RADIUS);
    let hovered = ui.rect_contains_pointer(rect);
    painter.rect_filled(rect, radius, theme::card_fill(selected || hovered));
    match texture {
        Some(t) => {
            painter.image(
                t.id(),
                fit_rect(cover, t.size_vec2()),
                Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                Color32::WHITE,
            );
        }
        None => {
            let galley = painter.layout_no_wrap(
                theme::initials(&game.title),
                FontId::proportional(36.0),
                theme::TEXT_WEAK,
            );
            painter.galley(
                cover.center() - galley.size() / 2.0,
                galley,
                theme::TEXT_WEAK,
            );
            let system =
                emu_library::system(&game.system).map_or(game.system.as_str(), |s| &s.name);
            let badge = pos2(cover.left() + theme::SP_M, cover.bottom() - theme::SP_M);
            theme::badge(&painter, badge, 5.0, theme::system_color(&game.system));
            let label = painter.layout_no_wrap(
                system.to_owned(),
                FontId::proportional(theme::FONT_SMALL),
                theme::TEXT_WEAK,
            );
            painter.galley(
                pos2(badge.x + 10.0, badge.y - label.size().y / 2.0),
                label,
                theme::TEXT_WEAK,
            );
        }
    }
    if game.favorite {
        painter.text(
            cover.right_top() + Vec2::new(-8.0, 6.0),
            Align2::RIGHT_TOP,
            text::FAVORITE_MARK,
            FontId::proportional(18.0),
            theme::WARN,
        );
    }
    let mut job = egui::text::LayoutJob::simple(
        text::game_title(&game.title),
        FontId::proportional(13.0),
        theme::TEXT,
        rect.width() - 2.0 * theme::SP_XS,
    );
    job.wrap.max_rows = 2;
    job.halign = egui::Align::Center;
    let galley = painter.layout_job(job);
    painter.galley(
        pos2(rect.center().x, cover.bottom() + theme::SP_XS),
        galley,
        theme::TEXT,
    );
    if selected {
        // Same accent whether or not the grid has focus; an unfocused grid gets the dimmer shade.
        let color = if focused {
            theme::ACCENT
        } else {
            theme::ACCENT_DIM
        };
        painter.rect_stroke(
            rect,
            radius,
            Stroke::new(theme::FOCUS_WIDTH, color),
            StrokeKind::Outside,
        );
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
