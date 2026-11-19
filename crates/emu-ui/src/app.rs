//! winit `ApplicationHandler` driving the emulator (phase 1: one window, no egui yet).

use std::time::Instant;

use emu_runtime::{
    Emulator, EmulatorOptions, FramePacer, FrameRenderer, GlWindow, NullSink, Scaling,
};
use winit::application::ApplicationHandler;
use winit::dpi::PhysicalSize;
use winit::event::{ElementState, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{WindowAttributes, WindowId};

use crate::{KeyMap, KeyboardInput, LaunchOptions, UiError};

// Field order is the drop order: renderer, then window/context, then the emulator (ADR 0008).
struct App {
    renderer: Option<FrameRenderer>,
    window: Option<GlWindow>,
    emulator: Emulator,
    pacer: Option<FramePacer>,
    keyboard: KeyboardInput,
    options: LaunchOptions,
    audio: NullSink,
    error: Option<UiError>,
}

pub(crate) fn run(options: LaunchOptions) -> Result<(), UiError> {
    let emulator = Emulator::load(
        &options.core,
        &options.rom,
        EmulatorOptions {
            system_dir: options.system_dir.clone(),
            save_dir: options.save_dir.clone(),
            core_variables: Vec::new(),
        },
    )?;
    let event_loop = EventLoop::new().map_err(|e| UiError::EventLoop(e.to_string()))?;
    let mut app = App {
        renderer: None,
        window: None,
        emulator,
        pacer: None,
        keyboard: KeyboardInput::new(KeyMap::genesis_default()),
        options,
        audio: NullSink,
        error: None,
    };
    let result = event_loop.run_app(&mut app);
    app.renderer = None;
    app.window = None;
    result.map_err(|e| UiError::EventLoop(e.to_string()))?;
    if let Some(e) = app.error.take() {
        return Err(e);
    }
    if let Some(path) = &app.options.dump_last_frame {
        app.emulator.frame().write_png(path)?;
    }
    Ok(())
}

impl App {
    fn fail(&mut self, event_loop: &ActiveEventLoop, error: impl Into<UiError>) {
        self.error = Some(error.into());
        event_loop.exit();
    }

    fn step(&mut self, event_loop: &ActiveEventLoop) {
        let status = match self.emulator.run_frame(&mut self.keyboard, &mut self.audio) {
            Ok(s) => s,
            Err(e) => return self.fail(event_loop, e),
        };
        if status.av_info_changed
            && let Some(pacer) = &mut self.pacer
        {
            pacer.set_fps(self.emulator.av_info().timing.fps);
        }
        if status.video_updated
            && let Some(r) = &mut self.renderer
        {
            r.upload(self.emulator.frame());
        }
        if let Some(w) = &self.window {
            w.window().request_redraw();
        }
        let done = self
            .options
            .exit_after_frames
            .is_some_and(|n| self.emulator.frame_count() >= n);
        if status.shutdown_requested || done {
            event_loop.exit();
        }
    }

    fn redraw(&mut self) {
        let (Some(w), Some(r)) = (&self.window, &mut self.renderer) else {
            return;
        };
        let size = w.window().inner_size();
        if size.width == 0 || size.height == 0 {
            return;
        }
        r.draw(
            size,
            self.emulator.av_info().geometry.display_aspect(),
            Scaling::Integer,
        );
        if let Err(e) = w.swap_buffers() {
            tracing::warn!("swap_buffers failed: {e}");
        }
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let av = self.emulator.av_info();
        let scale = self.options.scale.max(1);
        let size = PhysicalSize::new(
            (av.geometry.base_width * scale).max(1),
            (av.geometry.base_height * scale).max(1),
        );
        let attrs = WindowAttributes::default()
            .with_title("emu-rust")
            .with_inner_size(size);
        let fps = av.timing.fps;
        let window = match GlWindow::new(event_loop, attrs) {
            Ok(w) => w,
            Err(e) => return self.fail(event_loop, e),
        };
        match FrameRenderer::new(&window) {
            Ok(r) => self.renderer = Some(r),
            Err(e) => return self.fail(event_loop, e),
        }
        self.window = Some(window);
        let now = Instant::now();
        self.pacer = Some(FramePacer::new(fps, now));
        event_loop.set_control_flow(ControlFlow::WaitUntil(now));
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                if let Some(w) = &self.window {
                    w.resize(size);
                    w.window().request_redraw();
                }
            }
            WindowEvent::Focused(false) => self.keyboard.release_all(),
            WindowEvent::KeyboardInput { event, .. } => {
                if let PhysicalKey::Code(code) = event.physical_key {
                    let pressed = event.state == ElementState::Pressed;
                    if code == KeyCode::Escape && pressed {
                        event_loop.exit();
                    } else if !event.repeat || !pressed {
                        self.keyboard.handle_key(code, pressed);
                    }
                }
            }
            WindowEvent::RedrawRequested => self.redraw(),
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let Some(pacer) = &mut self.pacer else {
            return;
        };
        let now = Instant::now();
        if pacer.frame_due(now) {
            self.step(event_loop);
        }
        if let Some(pacer) = &self.pacer {
            event_loop.set_control_flow(ControlFlow::WaitUntil(pacer.next_deadline()));
        }
    }
}
