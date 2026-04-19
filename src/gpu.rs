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
    display_text: String,
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
            status: "type a URL, press enter".to_owned(),
            display_text: String::new(),
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
                    "ok · {}B · {}",
                    resp.fetched_bytes,
                    resp.title.as_deref().unwrap_or("—")
                );
                self.display_text = format_report(&resp);
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

        // Prepare text buffers.
        let addr_text = format!(
            "{} {}",
            if self.url_input.is_empty() {
                "▸"
            } else {
                "▸"
            },
            if self.url_input.is_empty() {
                "type a URL…"
            } else {
                self.url_input.as_str()
            }
        );
        let status = self.status.clone();
        let body = if self.display_text.is_empty() {
            "(no navigate yet — type a URL and press enter)".to_owned()
        } else {
            self.display_text.clone()
        };

        let max_body_width = size.width.saturating_sub(48) as f32;
        let addr_buffer = text.create_buffer(&addr_text, 18.0, 24.0);
        let status_buffer = text.create_buffer(&status, 12.0, 16.0);
        let body_buffer = text.create_buffer(&body, 14.0, 20.0);
        // Set sizes so wrapping kicks in for the body.
        configure_buffer(
            &mut body_buffer.clone(),
            &mut text.font_system,
            max_body_width,
            (size.height as f32) - 100.0,
        );

        let addr_area = TextArea {
            buffer: &addr_buffer,
            left: 24.0,
            top: 18.0,
            scale: 1.0,
            bounds: TextBounds {
                left: 0,
                top: 0,
                right: size.width as i32,
                bottom: 56,
            },
            default_color: Color::rgb(236, 239, 244), // nord-6
            custom_glyphs: &[],
        };
        let status_area = TextArea {
            buffer: &status_buffer,
            left: 24.0,
            top: size.height as f32 - 24.0,
            scale: 1.0,
            bounds: TextBounds {
                left: 0,
                top: size.height as i32 - 32,
                right: size.width as i32,
                bottom: size.height as i32,
            },
            default_color: Color::rgb(163, 190, 140), // nord-14
            custom_glyphs: &[],
        };
        let body_area = TextArea {
            buffer: &body_buffer,
            left: 24.0,
            top: 72.0,
            scale: 1.0,
            bounds: TextBounds {
                left: 0,
                top: 56,
                right: size.width as i32,
                bottom: size.height as i32 - 32,
            },
            default_color: Color::rgb(216, 222, 233), // nord-4
            custom_glyphs: &[],
        };

        text.prepare(
            &gpu.device,
            &gpu.queue,
            size.width,
            size.height,
            [addr_area, body_area, status_area],
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
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            // nord-0 #2e3440 in linear-ish rgb
                            r: 0.031,
                            g: 0.039,
                            b: 0.051,
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

fn format_report(resp: &crate::api::NavigateResponse) -> String {
    let r = &resp.report;
    let mut out = String::new();
    out.push_str(&format!("URL     {}\n", resp.final_url));
    out.push_str(&format!(
        "TITLE   {}\n",
        resp.title.as_deref().unwrap_or("—")
    ));
    out.push_str(&format!(
        "BYTES   {}\n\n",
        resp.fetched_bytes
    ));
    if !r.frameworks.is_empty() {
        let fws: Vec<String> = r
            .frameworks
            .iter()
            .map(|f| format!("{} ({:.0}%)", f.name, f.confidence * 100.0))
            .collect();
        out.push_str(&format!("FRAMEWORKS  {}\n", fws.join(", ")));
    }
    out.push_str(&format!("EFFECTS     {} fired\n", r.effects_fired));
    out.push_str(&format!("AGENTS      {} fired\n", r.agents_fired));
    out.push_str(&format!("TRANSFORMS  {} applied\n", r.transforms_applied));
    for hit in &r.transform_hits {
        out.push_str(&format!("  · {hit}\n"));
    }
    if !r.state_snapshot.is_empty() {
        out.push_str("\nSTATE CELLS\n");
        for cell in &r.state_snapshot {
            out.push_str(&format!("  {} = {}\n", cell.name, cell.value));
        }
    }
    if !r.derived_snapshot.is_empty() {
        out.push_str("\nDERIVED\n");
        for cell in &r.derived_snapshot {
            out.push_str(&format!("  {} = {}\n", cell.name, cell.value));
        }
    }
    let _ = Attrs::new().family(Family::Monospace).color(Color::rgb(136, 192, 208));
    let _ = Shaping::Advanced;
    out
}
