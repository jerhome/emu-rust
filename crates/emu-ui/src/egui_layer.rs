//! egui on the `GlWindow` context (ADR 0002, ADR 0008 as amended): `egui_winit` for input, `egui_glow` for
//! painting. Per frame: `run` (egui pass) → app applies actions → `GlWindow::draw_frame` → `paint` →
//! `swap_buffers`. `draw_frame` resets all the GL state it uses, so egui may leave anything behind.

use egui::{ClippedPrimitive, Context, TexturesDelta, ViewportId};
use emu_runtime::GlWindow;
use winit::event::WindowEvent;
use winit::window::Window;

pub struct EguiLayer {
    ctx: Context,
    state: egui_winit::State,
    painter: egui_glow::Painter,
    pending: Option<Pending>,
    destroyed: bool,
}

struct Pending {
    primitives: Vec<ClippedPrimitive>,
    textures: TexturesDelta,
    pixels_per_point: f32,
}

impl EguiLayer {
    pub fn new(window: &GlWindow) -> Result<Self, String> {
        window.make_current().map_err(|e| e.to_string())?;
        let painter = egui_glow::Painter::new(window.gl().clone(), "", None, true)
            .map_err(|e| e.to_string())?;
        let ctx = Context::default();
        crate::ui::apply_style(&ctx);
        let w = window.window();
        #[allow(clippy::cast_possible_truncation)] // scale factors are small
        let ppp = w.scale_factor() as f32;
        let state = egui_winit::State::new(
            ctx.clone(),
            ViewportId::ROOT,
            w,
            Some(ppp),
            None,
            Some(painter.max_texture_side()),
        );
        Ok(Self {
            ctx,
            state,
            painter,
            pending: None,
            destroyed: false,
        })
    }

    /// Feeds a window event; returns whether egui wants a repaint.
    pub fn on_window_event(&mut self, window: &Window, event: &WindowEvent) -> bool {
        self.state.on_window_event(window, event).repaint
    }

    /// Treats the window as focused. egui only shows keyboard focus in a focused window, and a window under a
    /// bare Xvfb (no window manager) never gets focus: used for `--screen` captures.
    pub fn assume_focused(&mut self, window: &Window) {
        let _repaint = self
            .state
            .on_window_event(window, &WindowEvent::Focused(true));
    }

    /// Runs one egui pass and keeps its output for `paint`. Returns egui's requested repaint delay.
    pub fn run(&mut self, window: &Window, ui: impl FnMut(&Context)) -> std::time::Duration {
        let raw = self.state.take_egui_input(window);
        let output = self.ctx.run_pass(raw, ui);
        self.state
            .handle_platform_output(window, output.platform_output);
        let primitives = self.ctx.tessellate(output.shapes, output.pixels_per_point);
        let delay = output
            .viewport_output
            .get(&ViewportId::ROOT)
            .map_or(std::time::Duration::MAX, |v| v.repaint_delay);
        // An unpainted previous pass still owes its texture updates: merge them.
        let mut textures = self.pending.take().map(|p| p.textures).unwrap_or_default();
        textures.append(output.textures_delta);
        self.pending = Some(Pending {
            primitives,
            textures,
            pixels_per_point: output.pixels_per_point,
        });
        delay
    }

    /// Paints the last pass over the back buffer (after `draw_frame`, before `swap_buffers`).
    pub fn paint(&mut self, window: &GlWindow) {
        let Some(mut p) = self.pending.take() else {
            return;
        };
        if let Err(e) = window.make_current() {
            tracing::warn!("egui non dessiné : {e}");
            return;
        }
        let size = window.window().inner_size();
        self.painter.paint_and_update_textures(
            [size.width, size.height],
            p.pixels_per_point,
            &p.primitives,
            &mut p.textures,
        );
    }

    /// Frees egui's GL objects; must run while `window` (its context) is alive.
    pub fn destroy(&mut self, window: &GlWindow) {
        if self.destroyed {
            return;
        }
        self.destroyed = true;
        if let Some(mut p) = self.pending.take() {
            p.textures.clear();
        }
        match window.make_current() {
            Ok(()) => self.painter.destroy(),
            Err(e) => tracing::warn!("ressources egui non libérées : {e}"),
        }
    }
}
