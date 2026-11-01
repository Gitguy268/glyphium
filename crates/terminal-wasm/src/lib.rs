//! The web client's Rust side. JavaScript passes bytes in and gets frames, replies and titles
//! out; everything else (sockets, the v86 console, input events) stays in JavaScript.
#![cfg(target_arch = "wasm32")]

use glyphium_terminal_core::{Frame, Terminal};
use glyphium_terminal_render::{FontConfig, OFFSCREEN_FORMAT, PaneView, Renderer};
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = console)]
    fn error(s: &str);
}

#[wasm_bindgen(start)]
pub fn start() {
    std::panic::set_hook(Box::new(|info| error(&info.to_string())));
}

#[wasm_bindgen]
pub struct WebTerminal {
    term: Terminal,
    frame: Frame,
    renderer: Renderer,
    view: PaneView,
    adapter: String,
    cols: u16,
    rows: u16,
}

#[wasm_bindgen]
impl WebTerminal {
    /// Rejects when WebGPU is unavailable, so the caller can keep its xterm.js fallback.
    pub async fn create(cols: u16, rows: u16, size_px: f32, extra_fonts: Vec<js_sys::Uint8Array>) -> Result<WebTerminal, JsError> {
        let instance = wgpu::Instance::default();
        let adapter = instance.request_adapter(&Default::default()).await.map_err(|e| JsError::new(&format!("no WebGPU adapter: {e}")))?;
        let info = adapter.get_info();
        let (device, queue) = adapter.request_device(&Default::default()).await.map_err(|e| JsError::new(&format!("no WebGPU device: {e}")))?;
        let fonts = FontConfig { size_px, extra_fonts: extra_fonts.iter().map(|f| f.to_vec()).collect(), ..Default::default() };
        let renderer = Renderer::new(device, queue, OFFSCREEN_FORMAT, &fonts);
        let view = renderer.new_view();
        Ok(WebTerminal {
            term: Terminal::new(cols, rows, 5000),
            frame: Frame::default(),
            renderer,
            view,
            adapter: format!("{} ({:?})", info.name, info.backend),
            cols,
            rows,
        })
    }

    pub fn write(&mut self, bytes: &[u8]) {
        self.term.write(bytes);
    }

    pub fn take_replies(&mut self) -> Vec<u8> {
        self.term.take_replies()
    }

    pub fn title(&self) -> String {
        self.term.title()
    }

    pub fn adapter(&self) -> String {
        self.adapter.clone()
    }

    pub fn pixel_width(&self) -> u32 {
        self.renderer.pixel_size(self.cols, self.rows).0
    }

    pub fn pixel_height(&self) -> u32 {
        self.renderer.pixel_size(self.cols, self.rows).1
    }

    /// Renders the current state offscreen and returns tightly packed RGBA.
    pub async fn render_rgba(&mut self) -> Vec<u8> {
        self.term.update_frame(&mut self.frame);
        self.renderer.update(&mut self.view, &self.frame);
        let size = self.renderer.pixel_size(self.cols, self.rows);
        self.renderer.render_to_rgba(&self.view, self.frame.background, size).await
    }
}
