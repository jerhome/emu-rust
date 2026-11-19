use std::ffi::CStr;
use std::num::NonZeroU32;
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

/// A winit window with its OpenGL 3.3 core context and surface (ADR 0002, ADR 0008).
/// All GL `unsafe` stays in emu-runtime; callers only get safe handles.
pub struct GlWindow {
    // Field order is drop order: GL objects and surface go before the context, the window last.
    gl: Arc<glow::Context>,
    surface: Surface<WindowSurface>,
    context: PossiblyCurrentContext,
    window: Window,
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

        let (window, config) = DisplayBuilder::new()
            .with_window_attributes(Some(attributes))
            .build(event_loop, ConfigTemplateBuilder::new(), pick_config)
            .map_err(|e| gl_err("cannot create window or find a GL config", &e))?;
        let window =
            window.ok_or_else(|| RuntimeError::Gl("display builder returned no window".into()))?;
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

        Ok(Self {
            gl: Arc::new(gl),
            surface,
            context,
            window,
        })
    }

    pub fn window(&self) -> &Window {
        &self.window
    }

    /// Shared GL function table, for `egui_glow::Painter` in phase 2.
    pub fn gl(&self) -> &Arc<glow::Context> {
        &self.gl
    }

    /// Call on `WindowEvent::Resized`. Zero sizes are ignored (minimised window).
    pub fn resize(&self, size: PhysicalSize<u32>) {
        if let (Some(w), Some(h)) = (NonZeroU32::new(size.width), NonZeroU32::new(size.height)) {
            self.surface.resize(&self.context, w, h);
        }
    }

    pub fn swap_buffers(&self) -> Result<(), RuntimeError> {
        self.surface
            .swap_buffers(&self.context)
            .map_err(|e| RuntimeError::Gl(format!("swap_buffers failed: {e}")))
    }
}

/// Plain opaque window: no multisampling, no transparency needed.
fn pick_config(configs: Box<dyn Iterator<Item = Config> + '_>) -> Config {
    configs
        .min_by_key(|c| (c.num_samples(), c.supports_transparency().unwrap_or(false)))
        // The picker cannot report an error; an empty list means the system has no usable GL at all
        // (GLX errors out before calling us, EGL may not).
        .expect("no OpenGL framebuffer configuration available")
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

/// Draws the emulated frame as a textured quad with letterboxing. Must be dropped before its `GlWindow`.
pub struct FrameRenderer {
    gl: Arc<glow::Context>,
    program: glow::Program,
    vertex_array: glow::VertexArray,
    texture: glow::Texture,
    /// Size of the texture storage; `(0, 0)` until the first non-empty upload.
    texture_size: (u32, u32),
}

impl FrameRenderer {
    pub fn new(window: &GlWindow) -> Result<Self, RuntimeError> {
        let gl = Arc::clone(window.gl());
        // SAFETY: `window`'s context is current on this thread (made current in `GlWindow::new`, and the
        // window is !Send); every object created here is deleted in `Drop` with the same context.
        unsafe {
            let program = link_program(&gl)?;
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
                gl,
                program,
                vertex_array,
                texture,
                texture_size: (0, 0),
            })
        }
    }

    /// Uploads `frame` into the texture (reallocated only when the size changes).
    pub fn upload(&mut self, frame: &FrameRgba) {
        let (width, height) = (frame.width(), frame.height());
        if width == 0 || height == 0 {
            return;
        }
        let (Ok(w), Ok(h)) = (i32::try_from(width), i32::try_from(height)) else {
            return;
        };
        let gl = &self.gl;
        // SAFETY: the context is current (see `new`); `frame.pixels()` holds exactly `width * height * 4`
        // bytes (FrameRgba invariant), matching the RGBA/UNSIGNED_BYTE upload with unpack alignment 4 and
        // no row length override.
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

    /// Clears to black and draws the last uploaded frame; `aspect_ratio` comes from `GameGeometry::display_aspect`.
    pub fn draw(&mut self, viewport: PhysicalSize<u32>, aspect_ratio: f32, scaling: Scaling) {
        let full = (0, 0, clamp_i32(viewport.width), clamp_i32(viewport.height));
        let target = target_rect(viewport, self.texture_size, aspect_ratio, scaling);
        let filter = match scaling {
            Scaling::Integer => glow::NEAREST,
            Scaling::Fit => glow::LINEAR,
        } as i32;
        let gl = &self.gl;
        // SAFETY: the context is current (see `new`); all objects used were created by this renderer and are
        // alive until `Drop`. State this pass depends on is set explicitly, since egui shares the context.
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
}

impl Drop for FrameRenderer {
    fn drop(&mut self) {
        // SAFETY: objects were created with this context, which is still alive (the renderer must be dropped
        // before its `GlWindow`, documented on the type) and current on this thread.
        unsafe {
            self.gl.delete_texture(self.texture);
            self.gl.delete_vertex_array(self.vertex_array);
            self.gl.delete_program(self.program);
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
