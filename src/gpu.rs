//! Native GPU window via garasu — real winit + wgpu + glyphon.
//!
//! First visible face of namimado: open a window, render the substrate
//! report as text on a nord-palette surface. Every keystroke types into
//! the address bar; Enter dispatches `NamimadoService::navigate`, and
//! the fresh report re-renders on the next frame.
//!
//! This module is behind the `gpu-chrome` feature. When off, `main.rs`
//! falls back to the old `app::run` scaffold.

use std::sync::Arc;

use anyhow::Result;
use garasu::{GpuContext, TextRenderer};
use glyphon::{Attrs, Buffer, Color, Family, Shaping, TextArea, TextBounds};
use tracing::info;
use wgpu::TextureFormat;
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalSize};
use winit::event::{KeyEvent, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::keyboard::{Key, NamedKey};
use winit::window::{Window, WindowAttributes, WindowId};

use crate::api::NavigateRequest;
use crate::service::NamimadoService;
use crate::theme::current_scheme;

/// Convert an irodzuki Color (0.0..=1.0 f32 per channel) into a glyphon
/// Color (0..=255 u8). The GPU window reads every text role through the
/// scheme so a theme swap only needs the scheme reload.
fn scheme_glyph(c: irodzuki::scheme::Color) -> Color {
    let to_byte = |f: f32| (f.clamp(0.0, 1.0) * 255.0).round() as u8;
    Color::rgb(to_byte(c.r), to_byte(c.g), to_byte(c.b))
}

/// Launch the native GPU window. Blocks until the user closes it.
pub fn run(initial_url: &str) -> Result<()> {
    let event_loop = EventLoop::new()?;
    let service = NamimadoService::new();
    let mut app = GpuApp::new(service, initial_url.to_owned());
    event_loop.run_app(&mut app)?;
    Ok(())
}

pub struct GpuApp {
    service: NamimadoService,
    initial_url: String,
    window: Option<Arc<Window>>,
    gpu: Option<GpuContext>,
    surface: Option<wgpu::Surface<'static>>,
    surface_format: TextureFormat,
    text: Option<TextRenderer>,
    size: PhysicalSize<u32>,
    url_input: String,
    status: String,
    /// Left pane — rendered page body text (nami-core's text_render).
    body_text: String,
    /// Right pane — structured substrate inspector lines.
    inspector_text: String,
    /// Right pane alt — page as S-expression (Lisp space).
    dom_sexp: String,
    /// Which right-pane view is active.
    right_view: RightView,
    /// Page title shown above the body.
    page_title: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RightView {
    Substrate,
    Dom,
}

impl GpuApp {
    fn new(service: NamimadoService, initial_url: String) -> Self {
        let url_input = if initial_url == "about:blank" {
            String::new()
        } else {
            initial_url.clone()
        };
        Self {
            service,
            initial_url,
            window: None,
            gpu: None,
            surface: None,
            surface_format: TextureFormat::Bgra8UnormSrgb,
            text: None,
            size: PhysicalSize::new(1280, 800),
            url_input,
            status: "type a URL, press enter · Tab toggles dom·lisp · Esc quits".to_owned(),
            body_text: String::new(),
            inspector_text: String::new(),
            dom_sexp: String::new(),
            right_view: RightView::Substrate,
            page_title: String::new(),
        }
    }

    fn submit_navigate(&mut self) {
        let url = self.url_input.trim();
        if url.is_empty() {
            self.status = "url empty".to_owned();
            return;
        }
        self.status = format!("navigating {url} …");
        match self.service.navigate(NavigateRequest {
            url: url.to_owned(),
        }) {
            Ok(resp) => {
                self.status = format!(
                    "ok · {}B · {} effects · {} transforms",
                    resp.fetched_bytes, resp.report.effects_fired, resp.report.transforms_applied,
                );
                self.page_title = resp.title.clone().unwrap_or_default();
                self.body_text = clean_body(&resp.text_render);
                self.inspector_text = format_inspector(&resp);
                self.dom_sexp = resp.dom_sexp.clone();
            }
            Err(e) => {
                self.status = format!("error: {e}");
            }
        }
        if let Some(w) = &self.window {
            w.request_redraw();
        }
    }
}

impl ApplicationHandler for GpuApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let attrs = WindowAttributes::default()
            .with_title("namimado · substrate browser")
            .with_inner_size(LogicalSize::new(1280.0, 800.0));
        let window = match event_loop.create_window(attrs) {
            Ok(w) => Arc::new(w),
            Err(e) => {
                tracing::error!("window create failed: {e}");
                event_loop.exit();
                return;
            }
        };
        let size = window.inner_size();

        // Garasu owns the async wgpu init; block on it synchronously.
        let gpu = match pollster::block_on(GpuContext::new()) {
            Ok(g) => g,
            Err(e) => {
                tracing::error!("GPU init failed: {e}");
                event_loop.exit();
                return;
            }
        };

        let surface = match gpu.instance.create_surface(window.clone()) {
            Ok(s) => s,
            Err(e) => {
                tracing::error!("surface create failed: {e}");
                event_loop.exit();
                return;
            }
        };
        let format =
            gpu.configure_surface(&surface, size.width.max(1), size.height.max(1));
        let text = TextRenderer::new(&gpu.device, &gpu.queue, format);

        info!(format = ?format, w = size.width, h = size.height, "gpu surface ready");

        self.window = Some(window);
        self.gpu = Some(gpu);
        self.surface = Some(surface);
        self.surface_format = format;
        self.text = Some(text);
        self.size = size;

        // Auto-navigate if a starting URL was supplied.
        if !self.initial_url.is_empty() && self.initial_url != "about:blank" {
            self.submit_navigate();
        }

        if let Some(w) = &self.window {
            w.request_redraw();
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _id: WindowId,
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(new_size) => {
                self.size = new_size;
                if let (Some(gpu), Some(surface)) = (&self.gpu, &self.surface) {
                    gpu.configure_surface(surface, new_size.width.max(1), new_size.height.max(1));
                }
                if let Some(w) = &self.window {
                    w.request_redraw();
                }
            }
            WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        logical_key,
                        state: winit::event::ElementState::Pressed,
                        text,
                        ..
                    },
                ..
            } => match logical_key {
                Key::Named(NamedKey::Escape) => event_loop.exit(),
                Key::Named(NamedKey::Enter) => self.submit_navigate(),
                Key::Named(NamedKey::Tab) => {
                    self.right_view = match self.right_view {
                        RightView::Substrate => RightView::Dom,
                        RightView::Dom => RightView::Substrate,
                    };
                    if let Some(w) = &self.window {
                        w.request_redraw();
                    }
                }
                Key::Named(NamedKey::Backspace) => {
                    self.url_input.pop();
                    if let Some(w) = &self.window {
                        w.request_redraw();
                    }
                }
                Key::Character(_) | Key::Named(NamedKey::Space) => {
                    if let Some(t) = text {
                        self.url_input.push_str(&t);
                    }
                    if let Some(w) = &self.window {
                        w.request_redraw();
                    }
                }
                _ => {}
            },
            WindowEvent::RedrawRequested => {
                if let Err(e) = self.render() {
                    tracing::warn!("render failed: {e}");
                }
            }
            _ => {}
        }
    }
}

impl GpuApp {
    fn render(&mut self) -> Result<()> {
        let (Some(gpu), Some(surface), Some(text)) =
            (self.gpu.as_ref(), self.surface.as_ref(), self.text.as_mut())
        else {
            return Ok(());
        };
        let size = self.size;
        if size.width == 0 || size.height == 0 {
            return Ok(());
        }

        // Layout: top address bar (56px), bottom status bar (32px),
        // middle split 60/40 between page body (left) and inspector
        // (right) with a 24px gutter on each side + between panes.
        const TOP: f32 = 56.0;
        const BOTTOM: f32 = 32.0;
        const GUTTER: f32 = 24.0;
        let content_top = TOP + 16.0;
        let content_bot = size.height as f32 - BOTTOM - 8.0;
        let avail_w = size.width as f32 - GUTTER * 3.0;
        let left_w = (avail_w * 0.60).max(200.0);
        let right_w = avail_w - left_w;
        let left_x = GUTTER;
        let right_x = GUTTER + left_w + GUTTER;
        let content_h = (content_bot - content_top).max(64.0);

        // Address bar.
        let addr_text = if self.url_input.is_empty() {
            "▸ type a URL…".to_owned()
        } else {
            format!("▸ {}", self.url_input)
        };
        let addr_buffer = text.create_buffer(&addr_text, 20.0, 28.0);

        // Title (above body).
        let title = if self.page_title.is_empty() {
            String::new()
        } else {
            format!("# {}", self.page_title)
        };
        let title_buffer = text.create_buffer(&title, 16.0, 22.0);

        // Left pane — page body. Wrap to left_w.
        let body = if self.body_text.is_empty() {
            "(no navigate yet — type a URL and press enter)".to_owned()
        } else {
            self.body_text.clone()
        };
        let mut body_buffer = text.create_buffer(&body, 14.0, 20.0);
        configure_buffer(&mut body_buffer, &mut text.font_system, left_w, content_h);

        // Right pane — switchable between substrate inspector and
        // DOM-as-sexp (Tab toggles).
        let insp = match self.right_view {
            RightView::Substrate => {
                if self.inspector_text.is_empty() {
                    "substrate inspector\n───────────────\n(awaiting navigate)\n\n(Tab → dom·lisp)".to_owned()
                } else {
                    format!("{}\n\n(Tab → dom·lisp)", self.inspector_text)
                }
            }
            RightView::Dom => {
                if self.dom_sexp.is_empty() {
                    "dom · lisp space\n────────────────\n(awaiting navigate)\n\n(Tab → substrate)".to_owned()
                } else {
                    format!("dom · lisp space\n────────────────\n{}\n\n(Tab → substrate)", self.dom_sexp)
                }
            }
        };
        let mut insp_buffer = text.create_buffer(&insp, 13.0, 18.0);
        configure_buffer(&mut insp_buffer, &mut text.font_system, right_w, content_h);

        // Status.
        let status_buffer = text.create_buffer(&self.status, 12.0, 16.0);

        // Pull text colors from the current irodzuki scheme so the
        // GPU window auto-inherits any theme swap.
        let scheme = current_scheme();
        let addr_color   = scheme_glyph(scheme.base0c); // info cyan
        let title_color  = scheme_glyph(scheme.base0d); // link blue
        let body_color   = scheme_glyph(scheme.base05); // default fg
        let insp_color   = scheme_glyph(scheme.base0a); // accent yellow
        let status_color = scheme_glyph(scheme.base0b); // success green

        let addr_area = TextArea {
            buffer: &addr_buffer,
            left: GUTTER,
            top: 14.0,
            scale: 1.0,
            bounds: TextBounds {
                left: 0,
                top: 0,
                right: size.width as i32,
                bottom: TOP as i32,
            },
            default_color: addr_color,
            custom_glyphs: &[],
        };
        let title_area = TextArea {
            buffer: &title_buffer,
            left: left_x,
            top: TOP + 2.0,
            scale: 1.0,
            bounds: TextBounds {
                left: 0,
                top: TOP as i32,
                right: (left_x + left_w) as i32,
                bottom: content_top as i32 + 6,
            },
            default_color: title_color,
            custom_glyphs: &[],
        };
        let body_area = TextArea {
            buffer: &body_buffer,
            left: left_x,
            top: content_top,
            scale: 1.0,
            bounds: TextBounds {
                left: left_x as i32,
                top: content_top as i32,
                right: (left_x + left_w) as i32,
                bottom: content_bot as i32,
            },
            default_color: body_color,
            custom_glyphs: &[],
        };
        let insp_area = TextArea {
            buffer: &insp_buffer,
            left: right_x,
            top: content_top,
            scale: 1.0,
            bounds: TextBounds {
                left: right_x as i32,
                top: content_top as i32,
                right: (right_x + right_w) as i32,
                bottom: content_bot as i32,
            },
            default_color: insp_color,
            custom_glyphs: &[],
        };
        let status_area = TextArea {
            buffer: &status_buffer,
            left: GUTTER,
            top: size.height as f32 - BOTTOM + 6.0,
            scale: 1.0,
            bounds: TextBounds {
                left: 0,
                top: size.height as i32 - BOTTOM as i32,
                right: size.width as i32,
                bottom: size.height as i32,
            },
            default_color: status_color,
            custom_glyphs: &[],
        };

        text.prepare(
            &gpu.device,
            &gpu.queue,
            size.width,
            size.height,
            [addr_area, title_area, body_area, insp_area, status_area],
        )?;

        let frame = match surface.get_current_texture() {
            Ok(f) => f,
            Err(e) => {
                tracing::warn!("surface texture: {e}");
                return Ok(());
            }
        };
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("frame") });

        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        // Background = scheme.base00 (the "primary
                        // background" slot). f32 components are sRGB
                        // linear-ish; wgpu does its own conversion.
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: f64::from(scheme.base00.r).powf(2.2),
                            g: f64::from(scheme.base00.g).powf(2.2),
                            b: f64::from(scheme.base00.b).powf(2.2),
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                occlusion_query_set: None,
                timestamp_writes: None,
            });
            text.render(&mut pass)?;
        }

        gpu.queue.submit(std::iter::once(encoder.finish()));
        frame.present();
        Ok(())
    }
}

fn configure_buffer(
    buffer: &mut Buffer,
    fs: &mut glyphon::FontSystem,
    w: f32,
    h: f32,
) {
    buffer.set_size(fs, Some(w), Some(h));
    buffer.shape_until_scroll(fs, false);
}

/// Collapse runs of whitespace in nami-core's `text_render` so the
/// page reads as paragraphs rather than a sea of `\n\n\n`. Preserves
/// single blank lines as paragraph breaks.
fn clean_body(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut prev_blank = false;
    for line in raw.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            if !prev_blank && !out.is_empty() {
                out.push('\n');
                prev_blank = true;
            }
        } else {
            if !out.is_empty() && !prev_blank {
                out.push(' ');
            } else if prev_blank {
                out.push('\n');
            }
            out.push_str(trimmed);
            prev_blank = false;
        }
    }
    out
}

fn format_inspector(resp: &crate::api::NavigateResponse) -> String {
    let r = &resp.report;
    let mut out = String::new();
    out.push_str("SUBSTRATE\n─────────\n");
    out.push_str(&format!("effects     {}\n", r.effects_fired));
    out.push_str(&format!("agents      {}\n", r.agents_fired));
    out.push_str(&format!("transforms  {}\n", r.transforms_applied));
    out.push_str(&format!(
        "inline-lisp {} ok · {} err\n",
        r.inline_lisp_evaluated, r.inline_lisp_failed
    ));
    out.push_str(&format!("normalize   {}\n", r.normalize_applied));
    for hit in r.normalize_hits.iter().take(10) {
        out.push_str(&format!("  · {hit}\n"));
    }
    out.push_str(&format!("wasm-agents {}\n", r.wasm_agents_fired));
    for hit in r.wasm_agent_hits.iter().take(10) {
        out.push_str(&format!("  · {hit}\n"));
    }
    if let Some(route) = &r.routes_matched {
        out.push_str(&format!("route       {route}\n"));
    }
    if !r.queries_dispatched.is_empty() {
        out.push_str(&format!(
            "queries     {}\n",
            r.queries_dispatched.join(", ")
        ));
    }

    if !r.frameworks.is_empty() {
        out.push_str("\nFRAMEWORKS\n──────────\n");
        for f in &r.frameworks {
            out.push_str(&format!("  {} ({:.0}%)\n", f.name, f.confidence * 100.0));
        }
    }

    if !r.transform_hits.is_empty() {
        out.push_str("\nTRANSFORM HITS\n──────────────\n");
        for hit in r.transform_hits.iter().take(20) {
            out.push_str(&format!("  · {hit}\n"));
        }
        if r.transform_hits.len() > 20 {
            out.push_str(&format!("  … +{} more\n", r.transform_hits.len() - 20));
        }
    }

    if !r.state_snapshot.is_empty() {
        out.push_str("\nSTATE CELLS\n───────────\n");
        for cell in &r.state_snapshot {
            out.push_str(&format!("  {:<14} {}\n", cell.name, cell.value));
        }
    }

    if !r.derived_snapshot.is_empty() {
        out.push_str("\nDERIVED\n───────\n");
        for cell in &r.derived_snapshot {
            out.push_str(&format!("  {:<14} {}\n", cell.name, cell.value));
        }
    }

    // Reference so dead_code lint doesn't complain about the imports
    // we kept for possible per-span coloring later.
    let _ = (
        Attrs::new().family(Family::Monospace),
        Shaping::Advanced,
        Color::rgb(136, 192, 208),
    );
    out
}
