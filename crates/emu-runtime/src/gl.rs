use std::ffi::CStr;
use std::marker::PhantomData;
use std::num::NonZeroU32;
use std::panic::{self, AssertUnwindSafe};
use std::sync::Arc;

use glow::HasContext;
use glutin::config::{Config, ConfigTemplateBuilder};
use glutin::context::{
    ContextApi, ContextAttributesBuilder, GlProfile, NotCurrentContext, PossiblyCurrentContext,
    Version,
};
use glutin::display::{Display, GetGlDisplay};
use glutin::prelude::*;
use glutin::surface::{Surface, SurfaceAttributesBuilder, SwapInterval, WindowSurface};
use glutin_winit::DisplayBuilder;
use winit::dpi::PhysicalSize;
use winit::event_loop::ActiveEventLoop;
use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
use winit::window::{Window, WindowAttributes};

use crate::error::RuntimeError;
use crate::frame::FrameRgba;

const MIN_GL: (u8, u8) = (3, 3);

/// A winit window with its OpenGL 3.3 core context and surface (ADR 0002, ADR 0008), plus the renderer that
/// draws the emulated frame into it. All GL `unsafe` stays in emu-runtime; callers only get safe handles.
///
/// The renderer is owned here so its GL objects can never outlive the context nor be used with another
/// context current: every entry point makes this window's context current first.
///
/// `GlWindow` is neither `Send` nor `Sync`: a GL context is bound to the thread that made it current.
///
/// ```compile_fail
/// fn assert_send<T: Send>() {}
/// assert_send::<emu_runtime::GlWindow>();
/// ```
pub struct GlWindow {
    // Field order is drop order (after `Drop::drop` released the renderer's objects): surface before context,
    // the window last.
    renderer: FrameRenderer,
    gl: Arc<glow::Context>,
    surface: Surface<WindowSurface>,
    context: PossiblyCurrentContext,
    window: Window,
    _not_send: PhantomData<*const ()>,
}

impl GlWindow {
    /// Creates the window and makes its context current on the calling thread. Requests OpenGL 3.3 core,
    /// accepts anything up to 4.1 (macOS Intel); works on Mesa llvmpipe (Xvfb) and WSLg.
    pub fn new(
        event_loop: &ActiveEventLoop,
        attributes: WindowAttributes,
    ) -> Result<Self, RuntimeError> {
        let gl_err =
            |what: &str, e: &dyn std::fmt::Display| RuntimeError::Gl(format!("{what}: {e}"));

        let (window, config) = build_display(event_loop, attributes)?;
        let raw_window = window
            .window_handle()
            .map_err(|e| gl_err("window handle unavailable", &e))?
            .as_raw();
        let display = config.display();

        let context = create_context(&display, &config, raw_window)?;

        let size = window.inner_size();
        let surface_attributes = SurfaceAttributesBuilder::<WindowSurface>::new().build(
            raw_window,
            non_zero(size.width),
            non_zero(size.height),
        );
        // SAFETY: `raw_window` belongs to `window`, which is stored in `Self` and dropped after the surface
        // (field order), so the native window outlives the surface.
        let surface = unsafe { display.create_window_surface(&config, &surface_attributes) }
            .map_err(|e| gl_err("cannot create window surface", &e))?;
        let context = context
            .make_current(&surface)
            .map_err(|e| gl_err("cannot make GL context current", &e))?;

        // The frame pacer drives timing in phase 1; vsync is optional (ADR 0003). Failure is harmless.
        if let Err(e) = surface.set_swap_interval(&context, SwapInterval::DontWait) {
            tracing::debug!("cannot disable vsync: {e}");
        }

        // SAFETY: the context created from `display` is current on this thread, so the returned function
        // pointers are valid for it. `glow::Context` is stored next to the context and dropped first.
        let gl = unsafe {
            glow::Context::from_loader_function_cstr(|name: &CStr| {
                display.get_proc_address(name).cast()
            })
        };
        let version = gl.version();
        if version.is_embedded
            || (version.major, version.minor) < (u32::from(MIN_GL.0), u32::from(MIN_GL.1))
        {
            return Err(RuntimeError::Gl(format!(
                "OpenGL {}.{} core required, got {}.{}{}",
                MIN_GL.0,
                MIN_GL.1,
                version.major,
                version.minor,
                if version.is_embedded { " ES" } else { "" }
            )));
        }
        // SAFETY: the context is current and RENDERER is a valid string parameter in every GL version.
        let renderer = unsafe { gl.get_parameter_string(glow::RENDERER) };
        tracing::info!(
            "OpenGL {}.{} ({}) on {renderer}",
            version.major,
            version.minor,
            version.vendor_info
        );

        // SAFETY: the context is current on this thread (made current above, nothing else ran since).
        let renderer = unsafe { FrameRenderer::new(&gl)? };

        Ok(Self {
            renderer,
            gl: Arc::new(gl),
            surface,
            context,
            window,
            _not_send: PhantomData,
        })
    }

    pub fn window(&self) -> &Window {
        &self.window
    }

    /// Shared GL function table, for `egui_glow::Painter` in phase 2. The caller must call `make_current`
    /// first: the table is only valid while this window's context is current on the calling thread.
    pub fn gl(&self) -> &Arc<glow::Context> {
        &self.gl
    }

    /// Call on `WindowEvent::Resized`. Zero sizes are ignored (minimised window).
    pub fn resize(&self, size: PhysicalSize<u32>) {
        if let (Some(w), Some(h)) = (NonZeroU32::new(size.width), NonZeroU32::new(size.height)) {
            if let Err(e) = self.make_current() {
                tracing::warn!("resize skipped: {e}");
                return;
            }
            self.surface.resize(&self.context, w, h);
        }
    }

    pub fn swap_buffers(&self) -> Result<(), RuntimeError> {
        self.make_current()?;
        self.surface
            .swap_buffers(&self.context)
            .map_err(|e| RuntimeError::Gl(format!("swap_buffers failed: {e}")))
    }

    /// Uploads `frame` into the renderer's texture (reallocated only when the size changes).
    pub fn upload_frame(&mut self, frame: &FrameRgba) -> Result<(), RuntimeError> {
        self.make_current()?;
        // SAFETY: this window's context was just made current on this thread (`Self` is !Send, so it cannot
        // change thread), and the renderer's objects were created with that context.
        unsafe { self.renderer.upload(&self.gl, frame) };
        Ok(())
    }

    /// Clears to black and draws the last uploaded frame; `aspect_ratio` comes from
    /// `GameGeometry::display_aspect`. Call `swap_buffers` afterwards.
    pub fn draw_frame(
        &mut self,
        viewport: PhysicalSize<u32>,
        aspect_ratio: f32,
        scaling: Scaling,
    ) -> Result<(), RuntimeError> {
        self.make_current()?;
        // SAFETY: as in `upload_frame`.
        unsafe {
            self.renderer
                .draw(&self.gl, viewport, aspect_ratio, scaling)
        };
        Ok(())
    }

    /// No-op when already current (the common case: one window per thread). Needed because another context
    /// (a second `GlWindow`, a core's HW context later) may have been made current on this thread.
    pub fn make_current(&self) -> Result<(), RuntimeError> {
        if self.context.is_current() {
            return Ok(());
        }
        self.context
            .make_current(&self.surface)
            .map_err(|e| RuntimeError::Gl(format!("cannot make GL context current: {e}")))
    }
}

impl Drop for GlWindow {
    fn drop(&mut self) {
        match self.make_current() {
            // SAFETY: the context is alive (fields are dropped after this) and current on this thread; the
            // renderer's objects were created with it and are not used afterwards.
            Ok(()) => unsafe { self.renderer.delete(&self.gl) },
            // Destroying the context below frees its objects anyway.
            Err(e) => tracing::warn!("cannot release renderer GL objects: {e}"),
        }
    }
}

/// Unwind payload used to leave the config picker, which has no way to report an error.
struct NoGlConfig;

fn build_display(
    event_loop: &ActiveEventLoop,
    attributes: WindowAttributes,
) -> Result<(Window, Config), RuntimeError> {
    // `resume_unwind` skips the panic hook, so nothing is printed; only frames of glutin-winit (pure Rust) sit
    // between the picker and this `catch_unwind`, no FFI boundary is crossed.
    let built = panic::catch_unwind(AssertUnwindSafe(|| {
        DisplayBuilder::new()
            .with_window_attributes(Some(attributes))
            .build(event_loop, ConfigTemplateBuilder::new(), |configs| {
                pick_config(configs).unwrap_or_else(|| panic::resume_unwind(Box::new(NoGlConfig)))
            })
            .map_err(|e| RuntimeError::Gl(format!("cannot create window or find a GL config: {e}")))
    }));
    let (window, config) = match built {
        Ok(result) => result?,
        Err(payload) if payload.is::<NoGlConfig>() => {
            return Err(RuntimeError::Gl(
                "no OpenGL framebuffer configuration available".into(),
            ));
        }
        Err(payload) => panic::resume_unwind(payload),
    };
    let window =
        window.ok_or_else(|| RuntimeError::Gl("display builder returned no window".into()))?;
    Ok((window, config))
}

/// Plain opaque window: no multisampling, no transparency needed. `None` when the system has no usable GL
/// configuration (GLX errors out before calling the picker, EGL may not).
fn pick_config(configs: Box<dyn Iterator<Item = Config> + '_>) -> Option<Config> {
    configs.min_by_key(|c| (c.num_samples(), c.supports_transparency().unwrap_or(false)))
}

fn create_context(
    display: &Display,
    config: &Config,
    raw_window: RawWindowHandle,
) -> Result<NotCurrentContext, RuntimeError> {
    let version = Some(Version::new(MIN_GL.0, MIN_GL.1));
    let core = ContextAttributesBuilder::new()
        .with_context_api(ContextApi::OpenGl(version))
        .with_profile(GlProfile::Core)
        .build(Some(raw_window));
    // Some drivers reject an explicit core profile but still hand out a >= 3.3 context; the version is
    // checked after creation.
    let any_profile = ContextAttributesBuilder::new()
        .with_context_api(ContextApi::OpenGl(version))
        .build(Some(raw_window));

    // SAFETY: `raw_window` is a live window handle owned by the caller for the duration of this call and,
    // through `GlWindow`, for the lifetime of the context.
    unsafe { display.create_context(config, &core) }
        .or_else(|first| {
            tracing::debug!("OpenGL 3.3 core profile rejected ({first}), retrying without profile");
            // SAFETY: same as above.
            unsafe { display.create_context(config, &any_profile) }
        })
        .map_err(|e| RuntimeError::Gl(format!("cannot create an OpenGL 3.3 context: {e}")))
}

fn non_zero(v: u32) -> NonZeroU32 {
    NonZeroU32::new(v).unwrap_or(NonZeroU32::MIN)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Scaling {
    /// Largest integer multiple that fits; nearest-neighbour sampling.
    #[default]
    Integer,
    /// Fill the window keeping the aspect ratio; linear sampling.
    Fit,
}

const VERTEX_SHADER: &str = r"#version 330 core
out vec2 v_uv;
void main() {
    // Triangle strip covering the viewport, generated from the vertex id (no vertex buffer).
    vec2 pos = vec2(float(gl_VertexID & 1), float((gl_VertexID >> 1) & 1));
    v_uv = vec2(pos.x, 1.0 - pos.y);
    gl_Position = vec4(pos * 2.0 - 1.0, 0.0, 1.0);
}
";

const FRAGMENT_SHADER: &str = r"#version 330 core
in vec2 v_uv;
uniform sampler2D u_frame;
out vec4 frag_color;
void main() {
    frag_color = vec4(texture(u_frame, v_uv).rgb, 1.0);
}
";

/// Draws the emulated frame as a textured quad with letterboxing. Owned by `GlWindow`, which guarantees the
/// preconditions of every method: its context is alive and current on the calling thread.
struct FrameRenderer {
    program: glow::Program,
    vertex_array: glow::VertexArray,
    texture: glow::Texture,
    /// Size of the texture storage; `(0, 0)` until the first non-empty upload.
    texture_size: (u32, u32),
    _not_send: PhantomData<*const ()>,
}

impl FrameRenderer {
    /// # Safety
    /// A GL >= 3.3 context must be current on the calling thread, and stay alive and current for every later
    /// call on the returned renderer, including `delete`.
    unsafe fn new(gl: &glow::Context) -> Result<Self, RuntimeError> {
        // SAFETY: guaranteed by the caller; every object created here is either returned or deleted.
        unsafe {
            let program = link_program(gl)?;
            let vertex_array = gl.create_vertex_array().map_err(|e| {
                gl.delete_program(program);
                RuntimeError::Gl(format!("glGenVertexArrays: {e}"))
            })?;
            let texture = gl.create_texture().map_err(|e| {
                gl.delete_vertex_array(vertex_array);
                gl.delete_program(program);
                RuntimeError::Gl(format!("glGenTextures: {e}"))
            })?;
            gl.bind_texture(glow::TEXTURE_2D, Some(texture));
            for (param, value) in [
                (glow::TEXTURE_WRAP_S, glow::CLAMP_TO_EDGE),
                (glow::TEXTURE_WRAP_T, glow::CLAMP_TO_EDGE),
                (glow::TEXTURE_MIN_FILTER, glow::NEAREST),
                (glow::TEXTURE_MAG_FILTER, glow::NEAREST),
            ] {
                gl.tex_parameter_i32(glow::TEXTURE_2D, param, value as i32);
            }
            gl.bind_texture(glow::TEXTURE_2D, None);

            gl.use_program(Some(program));
            let sampler = gl.get_uniform_location(program, "u_frame");
            gl.uniform_1_i32(sampler.as_ref(), 0);
            gl.use_program(None);

            Ok(Self {
                program,
                vertex_array,
                texture,
                texture_size: (0, 0),
                _not_send: PhantomData,
            })
        }
    }

    /// # Safety
    /// The context this renderer was created with must be current on the calling thread.
    unsafe fn upload(&mut self, gl: &glow::Context, frame: &FrameRgba) {
        let (width, height) = (frame.width(), frame.height());
        if width == 0 || height == 0 {
            return;
        }
        let (Ok(w), Ok(h)) = (i32::try_from(width), i32::try_from(height)) else {
            return;
        };
        // SAFETY: context guaranteed by the caller; `frame.pixels()` holds exactly `width * height * 4` bytes
        // (FrameRgba invariant), matching the RGBA/UNSIGNED_BYTE upload with unpack alignment 4 and no row
        // length override.
        unsafe {
            gl.bind_texture(glow::TEXTURE_2D, Some(self.texture));
            gl.pixel_store_i32(glow::UNPACK_ALIGNMENT, 4);
            gl.pixel_store_i32(glow::UNPACK_ROW_LENGTH, 0);
            let pixels = glow::PixelUnpackData::Slice(Some(frame.pixels()));
            if self.texture_size == (width, height) {
                gl.tex_sub_image_2d(
                    glow::TEXTURE_2D,
                    0,
                    0,
                    0,
                    w,
                    h,
                    glow::RGBA,
                    glow::UNSIGNED_BYTE,
                    pixels,
                );
            } else {
                gl.tex_image_2d(
                    glow::TEXTURE_2D,
                    0,
                    glow::RGBA8 as i32,
                    w,
                    h,
                    0,
                    glow::RGBA,
                    glow::UNSIGNED_BYTE,
                    pixels,
                );
                self.texture_size = (width, height);
            }
            gl.bind_texture(glow::TEXTURE_2D, None);
        }
    }

    /// # Safety
    /// The context this renderer was created with must be current on the calling thread.
    unsafe fn draw(
        &self,
        gl: &glow::Context,
        viewport: PhysicalSize<u32>,
        aspect_ratio: f32,
        scaling: Scaling,
    ) {
        let full = (0, 0, clamp_i32(viewport.width), clamp_i32(viewport.height));
        let target = target_rect(viewport, self.texture_size, aspect_ratio, scaling);
        let filter = match scaling {
            Scaling::Integer => glow::NEAREST,
            Scaling::Fit => glow::LINEAR,
        } as i32;
        // SAFETY: context guaranteed by the caller; all objects used were created by this renderer and are
        // alive until `delete`. State this pass depends on is set explicitly, since egui shares the context.
        unsafe {
            gl.disable(glow::SCISSOR_TEST);
            gl.disable(glow::BLEND);
            gl.disable(glow::DEPTH_TEST);
            gl.disable(glow::CULL_FACE);
            gl.disable(glow::FRAMEBUFFER_SRGB);
            gl.bind_framebuffer(glow::FRAMEBUFFER, None);
            gl.viewport(full.0, full.1, full.2, full.3);
            gl.clear_color(0.0, 0.0, 0.0, 1.0);
            gl.clear(glow::COLOR_BUFFER_BIT);

            let Some((x, y, w, h)) = target else {
                return;
            };
            gl.viewport(x, y, w, h);
            gl.use_program(Some(self.program));
            gl.bind_vertex_array(Some(self.vertex_array));
            gl.active_texture(glow::TEXTURE0);
            gl.bind_texture(glow::TEXTURE_2D, Some(self.texture));
            gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MIN_FILTER, filter);
            gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MAG_FILTER, filter);
            gl.draw_arrays(glow::TRIANGLE_STRIP, 0, 4);
            gl.bind_texture(glow::TEXTURE_2D, None);
            gl.bind_vertex_array(None);
            gl.use_program(None);
        }
    }

    /// # Safety
    /// The context this renderer was created with must be current on the calling thread; the renderer must not
    /// be used afterwards.
    unsafe fn delete(&mut self, gl: &glow::Context) {
        // SAFETY: guaranteed by the caller.
        unsafe {
            gl.delete_texture(self.texture);
            gl.delete_vertex_array(self.vertex_array);
            gl.delete_program(self.program);
        }
    }
}

/// # Safety
/// A GL >= 3.3 context must be current on the calling thread.
unsafe fn link_program(gl: &glow::Context) -> Result<glow::Program, RuntimeError> {
    // SAFETY: guaranteed by the caller; every object created here is either returned or deleted.
    unsafe {
        let program = gl
            .create_program()
            .map_err(|e| RuntimeError::Gl(format!("glCreateProgram: {e}")))?;
        let mut shaders = Vec::with_capacity(2);
        let mut result = Ok(());
        for (kind, source) in [
            (glow::VERTEX_SHADER, VERTEX_SHADER),
            (glow::FRAGMENT_SHADER, FRAGMENT_SHADER),
        ] {
            let shader = match gl.create_shader(kind) {
                Ok(shader) => shader,
                Err(e) => {
                    result = Err(RuntimeError::Gl(format!("glCreateShader: {e}")));
                    break;
                }
            };
            gl.shader_source(shader, source);
            gl.compile_shader(shader);
            shaders.push(shader);
            if !gl.get_shader_compile_status(shader) {
                result = Err(RuntimeError::Gl(format!(
                    "shader compilation failed: {}",
                    gl.get_shader_info_log(shader)
                )));
                break;
            }
            gl.attach_shader(program, shader);
        }
        if result.is_ok() {
            gl.link_program(program);
            if !gl.get_program_link_status(program) {
                result = Err(RuntimeError::Gl(format!(
                    "shader link failed: {}",
                    gl.get_program_info_log(program)
                )));
            }
        }
        for shader in shaders {
            gl.detach_shader(program, shader);
            gl.delete_shader(shader);
        }
        match result {
            Ok(()) => Ok(program),
            Err(e) => {
                gl.delete_program(program);
                Err(e)
            }
        }
    }
}

fn clamp_i32(v: u32) -> i32 {
    i32::try_from(v).unwrap_or(i32::MAX)
}

/// Letterboxed destination rectangle `(x, y, w, h)` in GL viewport coordinates, or `None` if there is nothing
/// to draw. Integer scaling multiplies the height by an integer and derives the width from the aspect ratio;
/// it falls back to `Fit` when the window is smaller than one unscaled frame.
fn target_rect(
    viewport: PhysicalSize<u32>,
    frame: (u32, u32),
    aspect_ratio: f32,
    scaling: Scaling,
) -> Option<(i32, i32, i32, i32)> {
    let (fw, fh) = frame;
    if fw == 0 || fh == 0 || viewport.width == 0 || viewport.height == 0 {
        return None;
    }
    let (vw, vh) = (f64::from(viewport.width), f64::from(viewport.height));
    let aspect = if aspect_ratio.is_finite() && aspect_ratio > 0.0 {
        f64::from(aspect_ratio)
    } else {
        f64::from(fw) / f64::from(fh)
    };
    let fh = f64::from(fh);

    // Epsilon: an f32 aspect such as 320/224 must not turn an exact 2x fit into 1x.
    let integer_scale = ((vh / fh).min(vw / (fh * aspect)) + 1e-4).floor();
    let (w, h) = if scaling == Scaling::Integer && integer_scale >= 1.0 {
        let h = fh * integer_scale;
        ((h * aspect).round().min(vw), h)
    } else if vw / vh > aspect {
        (vh * aspect, vh)
    } else {
        (vw, vw / aspect)
    };
    let (w, h) = (w.round().max(1.0), h.round().max(1.0));
    let x = ((vw - w) / 2.0).floor();
    let y = ((vh - h) / 2.0).floor();
    // All values are within [0, viewport size] <= u32::MAX; GL takes i32, so saturate.
    let to_i32 = |v: f64| v.clamp(0.0, f64::from(i32::MAX)) as i32;
    Some((to_i32(x), to_i32(y), to_i32(w), to_i32(h)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(
        vw: u32,
        vh: u32,
        frame: (u32, u32),
        aspect: f32,
        s: Scaling,
    ) -> Option<(i32, i32, i32, i32)> {
        target_rect(PhysicalSize::new(vw, vh), frame, aspect, s)
    }

    #[test]
    fn integer_scaling() {
        // 320x224 shown at 4:3 in a 960x720 window: scale 3 on height (672), width 896.
        assert_eq!(
            rect(960, 720, (320, 224), 4.0 / 3.0, Scaling::Integer),
            Some((32, 24, 896, 672))
        );
        // Square pixels, exact fit.
        assert_eq!(
            rect(640, 448, (320, 224), 320.0 / 224.0, Scaling::Integer),
            Some((0, 0, 640, 448))
        );
    }

    #[test]
    fn integer_falls_back_to_fit_when_too_small() {
        assert_eq!(
            rect(160, 120, (320, 224), 4.0 / 3.0, Scaling::Integer),
            rect(160, 120, (320, 224), 4.0 / 3.0, Scaling::Fit)
        );
    }

    #[test]
    fn fit_letterboxes_and_pillarboxes() {
        assert_eq!(
            rect(1000, 600, (320, 240), 4.0 / 3.0, Scaling::Fit),
            Some((100, 0, 800, 600))
        );
        assert_eq!(
            rect(800, 1000, (320, 240), 4.0 / 3.0, Scaling::Fit),
            Some((0, 200, 800, 600))
        );
    }

    #[test]
    fn invalid_aspect_uses_frame_ratio() {
        assert_eq!(
            rect(640, 480, (320, 240), 0.0, Scaling::Fit),
            rect(640, 480, (320, 240), 4.0 / 3.0, Scaling::Fit)
        );
        assert_eq!(
            rect(640, 480, (320, 240), f32::NAN, Scaling::Integer),
            Some((0, 0, 640, 480))
        );
    }

    #[test]
    fn nothing_to_draw() {
        assert_eq!(rect(640, 480, (0, 0), 1.0, Scaling::Fit), None);
        assert_eq!(rect(0, 480, (320, 240), 1.0, Scaling::Fit), None);
    }
}
