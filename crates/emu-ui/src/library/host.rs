//! The library window (ADR 0014 §8): one winit event loop and one GL window for the whole session. In library
//! mode it draws `LibraryUi`; a launched game borrows the window and egui layer (`App::attach`) and gives them
//! back when it ends (`App::detach`), so quitting a game returns to the library. Background jobs wake the loop
//! through egui's repaint callback → `EventLoopProxy` (no polling).

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use emu_runtime::{GlWindow, Scaling};
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{StartCause, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::window::{WindowAttributes, WindowId};

use super::{LaunchRequest, LibraryEvent, LibraryUi, Page, text};
use crate::app::App;
use crate::egui_layer::EguiLayer;
use crate::{LaunchOptions, LibraryOptions, LibraryScreen, UiError};

const WINDOW: (u32, u32) = (1280, 720);
/// egui passes before a `--screen` capture (layout settles in two or three).
const CAPTURE_PASSES: u32 = 4;
/// Frames emulated before the window is captured in `--screen library-launch`.
const LAUNCH_CAPTURE_FRAMES: u64 = 90;
/// A `--screen` capture waits this long at most for cached covers to be decoded.
const CAPTURE_COVER_WAIT: Duration = Duration::from_secs(5);

/// Wakes the event loop from another thread.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Wake;

struct Host {
    // Drop order: a game (which may hold the window) first, then the egui painter, then the window.
    game: Option<Box<App>>,
    egui: Option<EguiLayer>,
    window: Option<GlWindow>,
    library: LibraryUi,
    audio: bool,
    screen: Option<LibraryScreen>,
    dump_window: Option<std::path::PathBuf>,
    proxy: Arc<Mutex<EventLoopProxy<Wake>>>,
    passes_left: u32,
    launch_tried: bool,
    started: Instant,
    repaint_at: Option<Instant>,
    error: Option<UiError>,
}

pub(crate) fn run(options: LibraryOptions) -> Result<(), UiError> {
    let event_loop = EventLoop::<Wake>::with_user_event()
        .build()
        .map_err(|e| UiError::EventLoop(e.to_string()))?;
    let proxy = Arc::new(Mutex::new(event_loop.create_proxy()));
    let mut library = LibraryUi::open(options.dirs, options.config, options.screen.is_some());
    match options.screen {
        Some(LibraryScreen::Settings) => library.set_page(Page::Settings),
        Some(LibraryScreen::Cores) => library.set_page(Page::Cores),
        _ => {}
    }
    let mut host = Host {
        game: None,
        egui: None,
        window: None,
        library,
        audio: options.audio,
        screen: options.screen,
        dump_window: options.dump_window,
        proxy,
        passes_left: CAPTURE_PASSES,
        launch_tried: false,
        started: Instant::now(),
        repaint_at: None,
        error: None,
    };
    let result = event_loop.run_app(&mut host);
    host.end_game(None);
    if let (Some(egui), Some(w)) = (&mut host.egui, &host.window) {
        egui.destroy(w);
    }
    host.egui = None;
    host.window = None;
    result.map_err(|e| UiError::EventLoop(e.to_string()))?;
    host.error.map_or(Ok(()), Err)
}

impl Host {
    fn capturing(&self) -> bool {
        self.dump_window.is_some() && self.screen.is_some()
    }

    fn request_redraw(&self) {
        if let Some(w) = &self.window {
            w.window().request_redraw();
        }
    }

    fn start_game(&mut self, event_loop: &ActiveEventLoop, request: LaunchRequest) {
        let launch_capture = self.screen == Some(LibraryScreen::Launch);
        let options = LaunchOptions {
            core: request.core_path.clone(),
            rom: request.rom.clone(),
            dirs: self.library.dirs().clone(),
            config: self.library.config().clone(),
            scale: None,
            audio: self.audio,
            exit_after_frames: launch_capture.then_some(LAUNCH_CAPTURE_FRAMES),
            dump_last_frame: None,
            screen: None,
            dump_window: if launch_capture {
                self.dump_window.clone()
            } else {
                None
            },
        };
        tracing::info!(
            "lancement de {} avec {}",
            request.rom.display(),
            request.core
        );
        match App::build(options, true) {
            Ok(mut app) => {
                let Some(window) = self.window.take() else {
                    // Request dropped: its core was marked in use by `prepare_launch`.
                    self.library.game_over(None, None);
                    return;
                };
                self.library.game_started(&request);
                for w in &request.warnings {
                    app.warn(w.clone());
                }
                app.attach(event_loop, window, self.egui.take());
                self.game = Some(Box::new(app));
            }
            Err(e) => {
                tracing::warn!("lancement impossible : {e}");
                let msg = text::game_failed(&request.title, &e.to_string());
                self.library.game_over(None, Some(msg));
                self.request_redraw();
            }
        }
    }

    /// Ends the running game (if any): window and egui back, core released, library refreshed.
    fn end_game(&mut self, event_loop: Option<&ActiveEventLoop>) {
        let Some(mut game) = self.game.take() else {
            return;
        };
        let back = game.detach();
        drop(game);
        self.window = back.window;
        self.egui = back.egui;
        if let (Some(egui), Some(w)) = (&mut self.egui, &self.window) {
            egui.assume_focused(w.window());
        }
        let error = back.error.map(|e| e.to_string());
        self.library.game_over(Some(back.config), error);
        if let Some(event_loop) = event_loop {
            event_loop.set_control_flow(ControlFlow::Wait);
            if self.screen == Some(LibraryScreen::Launch) {
                event_loop.exit();
            }
        }
        self.request_redraw();
    }

    fn render(&mut self, event_loop: &ActiveEventLoop) {
        if self.screen == Some(LibraryScreen::Launch) && !self.launch_tried {
            self.launch_tried = true;
            self.library.wants_launch_capture();
        }
        let (Some(egui), Some(window)) = (&mut self.egui, &mut self.window) else {
            return;
        };
        if self.screen.is_some() {
            // Xvfb without a window manager may report the window unfocused: keep the focus ring visible.
            egui.assume_focused(window.window());
        }
        let library = &mut self.library;
        let mut events = Vec::new();
        let delay = egui.run_ui(window.window(), |ui| events = library.show(ui));
        self.repaint_at = (delay < Duration::from_secs(1)).then(|| Instant::now() + delay);
        let size = window.physical_size();
        if size.width > 0 && size.height > 0 {
            if let Err(e) = window.draw_frame(size, 4.0 / 3.0, Scaling::Fit) {
                tracing::warn!("draw failed: {e}");
            }
            egui.paint(window);
            if self.dump_window.is_some() && self.screen.is_some() && self.game.is_none() {
                let waiting_covers =
                    self.library.covers_busy() && self.started.elapsed() < CAPTURE_COVER_WAIT;
                if !waiting_covers {
                    self.passes_left = self.passes_left.saturating_sub(1);
                }
                if self.passes_left == 0
                    && let Some(path) = &self.dump_window
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
        }
        for event in events {
            match event {
                LibraryEvent::Quit => event_loop.exit(),
                LibraryEvent::Launch(request) => {
                    if self.game.is_none() && !event_loop.exiting() {
                        self.start_game(event_loop, request);
                    } else if self.game.is_none() {
                        // Exiting: the request is dropped, release its core.
                        self.library.game_over(None, None);
                    }
                    // A game already running keeps its core marked in use (one slot in the manager).
                }
            }
        }
        if self.game.is_none() && self.capturing() && !event_loop.exiting() {
            self.request_redraw();
        }
    }
}

impl ApplicationHandler<Wake> for Host {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() || self.game.is_some() {
            return;
        }
        let attrs = WindowAttributes::default()
            .with_title("emu-rust")
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
                let proxy = Arc::clone(&self.proxy);
                layer.context().set_request_repaint_callback(move |info| {
                    if info.delay.is_zero()
                        && let Ok(p) = proxy.lock()
                    {
                        // Only fails once the loop is gone: nothing left to wake.
                        let _ = p.send_event(Wake);
                    }
                });
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

    fn user_event(&mut self, _event_loop: &ActiveEventLoop, _event: Wake) {
        if self.game.is_none() {
            self.request_redraw();
        }
    }

    fn new_events(&mut self, _event_loop: &ActiveEventLoop, cause: StartCause) {
        if self.game.is_none() && matches!(cause, StartCause::ResumeTimeReached { .. }) {
            self.request_redraw();
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        if let Some(game) = &mut self.game {
            if matches!(event, WindowEvent::CloseRequested) {
                self.end_game(None);
                return event_loop.exit();
            }
            game.window_event(event_loop, id, event);
            if game.ended() {
                self.end_game(Some(event_loop));
            }
            return;
        }
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

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if let Some(game) = &mut self.game {
            game.about_to_wait(event_loop);
            if game.ended() {
                self.end_game(Some(event_loop));
            }
            return;
        }
        if event_loop.exiting() {
            return;
        }
        let flow = self
            .repaint_at
            .map_or(ControlFlow::Wait, ControlFlow::WaitUntil);
        event_loop.set_control_flow(flow);
    }
}
