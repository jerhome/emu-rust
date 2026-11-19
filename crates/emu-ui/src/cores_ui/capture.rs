//! Stand-alone window for the cores screen (`--screen cores`): no emulator, no network unless the user clicks.
//! With `dump_window` it renders a few passes, writes the PNG and quits (Xvfb captures); without, it is an
//! interactive window closed by « Retour » or the window button. The full library window is task U4.

use std::time::{Duration, Instant};

use emu_runtime::{GlWindow, Scaling};
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{StartCause, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{WindowAttributes, WindowId};

use super::{CoresEvent, CoresUi};
use crate::egui_layer::EguiLayer;
use crate::{LibraryOptions, UiError};

const CAPTURE_PASSES: u32 = 4;
const IDLE_POLL: Duration = Duration::from_millis(100);
const WINDOW: (u32, u32) = (1280, 720);

struct CaptureApp {
    // Drop order: egui painter before the GL window (it is also destroyed explicitly).
    egui: Option<EguiLayer>,
    window: Option<GlWindow>,
    cores: CoresUi,
    options: LibraryOptions,
    passes_left: u32,
    error: Option<UiError>,
}

pub(crate) fn run_capture(options: LibraryOptions) -> Result<(), UiError> {
    let cores = CoresUi::open(&options.dirs, &options.config);
    let event_loop = EventLoop::new().map_err(|e| UiError::EventLoop(e.to_string()))?;
    let mut app = CaptureApp {
        egui: None,
        window: None,
        cores,
        options,
        passes_left: CAPTURE_PASSES,
        error: None,
    };
    let result = event_loop.run_app(&mut app);
    app.teardown();
    result.map_err(|e| UiError::EventLoop(e.to_string()))?;
    app.error.map_or(Ok(()), Err)
}

impl CaptureApp {
    fn teardown(&mut self) {
        if let (Some(egui), Some(w)) = (&mut self.egui, &self.window) {
            egui.destroy(w);
        }
        self.egui = None;
    }

    fn render(&mut self, event_loop: &ActiveEventLoop) {
        let (Some(egui), Some(window)) = (&mut self.egui, &mut self.window) else {
            return;
        };
        let cores = &mut self.cores;
        let mut back = false;
        let delay = egui.run_ui(window.window(), |ui| {
            egui::CentralPanel::default().show(ui, |ui| {
                back = cores.show(ui) == Some(CoresEvent::Back);
            });
        });
        let size = window.physical_size();
        if size.width == 0 || size.height == 0 {
            return;
        }
        if let Err(e) = window.draw_frame(size, 4.0 / 3.0, Scaling::Fit) {
            tracing::warn!("draw failed: {e}");
            return;
        }
        egui.paint(window);
        let capturing = self.options.dump_window.is_some();
        if capturing {
            self.passes_left = self.passes_left.saturating_sub(1);
            if self.passes_left == 0
                && let Some(path) = &self.options.dump_window
            {
                match window.capture().and_then(|f| f.write_png(path)) {
                    Ok(()) => tracing::info!("fenêtre capturée : {}", path.display()),
                    Err(e) => tracing::warn!("capture de la fenêtre impossible : {e}"),
                }
                event_loop.exit();
            }
        }
        if let Err(e) = window.swap_buffers() {
            tracing::warn!("swap_buffers failed: {e}");
        }
        if back {
            event_loop.exit();
        } else if capturing || delay < Duration::from_secs(1) {
            window.window().request_redraw();
        }
    }
}

impl ApplicationHandler for CaptureApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let attrs = WindowAttributes::default()
            .with_title("emu-rust — cores")
            .with_inner_size(LogicalSize::new(WINDOW.0, WINDOW.1));
        let window = match GlWindow::new(event_loop, attrs) {
            Ok(w) => w,
            Err(e) => {
                self.error = Some(e.into());
                return event_loop.exit();
            }
        };
        match EguiLayer::new(&window) {
            Ok(mut layer) => {
                // A bare Xvfb has no window manager, hence no focus: egui would hide the focus ring.
                layer.assume_focused(window.window());
                self.egui = Some(layer);
            }
            Err(e) => {
                self.error = Some(UiError::EventLoop(format!("interface egui : {e}")));
                return event_loop.exit();
            }
        }
        window.window().request_redraw();
        self.window = Some(window);
        event_loop.set_control_flow(ControlFlow::Wait);
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match &event {
            WindowEvent::CloseRequested => return event_loop.exit(),
            WindowEvent::Resized(size) => {
                if let Some(w) = &self.window {
                    w.resize(*size);
                }
            }
            WindowEvent::ScaleFactorChanged { .. } => {
                if let Some(w) = &self.window {
                    w.scale_factor_changed();
                }
            }
            WindowEvent::RedrawRequested => return self.render(event_loop),
            _ => {}
        }
        if let (Some(egui), Some(w)) = (&mut self.egui, &self.window)
            && egui.on_window_event(w.window(), &event)
        {
            w.window().request_redraw();
        }
    }

    fn new_events(&mut self, _event_loop: &ActiveEventLoop, cause: StartCause) {
        if matches!(cause, StartCause::ResumeTimeReached { .. })
            && let Some(w) = &self.window
        {
            w.window().request_redraw();
        }
    }

    /// Job progress is signalled with `request_repaint`, which cannot wake winit: poll at a low rate instead.
    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if self.options.dump_window.is_none() {
            event_loop.set_control_flow(ControlFlow::WaitUntil(Instant::now() + IDLE_POLL));
        }
    }
}
