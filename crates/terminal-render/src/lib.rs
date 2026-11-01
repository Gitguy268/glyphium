//! Draws [`Frame`]s with wgpu: instanced quads for backgrounds, decorations and glyphs.
//!
//! The renderer holds no terminal state and knows no escape sequences. It keeps instance
//! lists per row and rebuilds only dirty rows; [`Renderer::update`] says whether anything
//! changed, so an idle terminal draws nothing. Lost GPU resources are rebuilt from the frame.

mod glyphs;

use glyphium_terminal_core::{Cell, CursorStyle, Frame, Rgb, attrs};
use wgpu::util::DeviceExt as _;

pub use glyphs::{ATLAS_SIZE, CellMetrics, FontConfig};
use glyphs::{AtlasFull, Glyphs, PlacedGlyph, STYLE_BOLD, STYLE_ITALIC};

/// Non-sRGB, so sRGB colour values pass through unchanged on every platform.
pub const OFFSCREEN_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct Instance {
    pos: [f32; 2],
    size: [f32; 2],
    uv: [f32; 4],
    color: [f32; 4],
    kind: u32,
}

const KIND_SOLID: u32 = 0;
const KIND_MASK: u32 = 1;
const KIND_COLOR: u32 = 2;

fn rgba(c: Rgb, a: f32) -> [f32; 4] {
    [c.r as f32 / 255.0, c.g as f32 / 255.0, c.b as f32 / 255.0, a]
}

fn solid(x: f32, y: f32, w: f32, h: f32, color: [f32; 4]) -> Instance {
    Instance { pos: [x, y], size: [w, h], uv: [0.0; 4], color, kind: KIND_SOLID }
}

#[derive(Default)]
struct RowInstances {
    back: Vec<Instance>,
    front: Vec<Instance>,
}

/// Counters for tests and benchmarks: they show how much work a frame really did.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Stats {
    pub rows_rebuilt: u64,
    pub buffer_uploads: u64,
    pub glyph_uploads: u64,
    pub atlas_evictions: u64,
    pub draws: u64,
}

pub struct Renderer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    pipeline: wgpu::RenderPipeline,
    bind_group: wgpu::BindGroup,
    globals: wgpu::Buffer,
    mask_atlas: wgpu::Texture,
    color_atlas: wgpu::Texture,
    instances: wgpu::Buffer,
    capacity: u64,
    glyphs: Glyphs,
    rows: Vec<RowInstances>,
    cursor: Vec<Instance>,
    scratch: Vec<Instance>,
    count: u32,
    clear: wgpu::Color,
    viewport: (u32, u32),
    /// Set by `invalidate`: rebuild every row whatever the frame says.
    rebuild_all: bool,
    pub stats: Stats,
}

fn atlas(device: &wgpu::Device, format: wgpu::TextureFormat, label: &str) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d { width: ATLAS_SIZE, height: ATLAS_SIZE, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    })
}

impl Renderer {
    pub fn new(device: wgpu::Device, queue: wgpu::Queue, format: wgpu::TextureFormat, fonts: &FontConfig) -> Self {
        let shader = device.create_shader_module(wgpu::include_wgsl!("shader.wgsl"));
        let texture_entry = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let sampler_entry = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
            count: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("glyphium"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
                    count: None,
                },
                texture_entry(1),
                texture_entry(2),
                sampler_entry(3),
                sampler_entry(4),
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("glyphium"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("glyphium"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: size_of::<Instance>() as u64,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &wgpu::vertex_attr_array![
                        0 => Float32x2, 1 => Float32x2, 2 => Float32x4, 3 => Float32x4, 4 => Uint32
                    ],
                })],
            },
            primitive: wgpu::PrimitiveState { topology: wgpu::PrimitiveTopology::TriangleStrip, ..Default::default() },
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let globals = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("globals"),
            contents: bytemuck::cast_slice(&[1.0f32, 1.0, 0.0, 0.0]),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let mask_atlas = atlas(&device, wgpu::TextureFormat::R8Unorm, "mask atlas");
        let color_atlas = atlas(&device, wgpu::TextureFormat::Rgba8Unorm, "color atlas");
        let nearest = device.create_sampler(&wgpu::SamplerDescriptor::default());
        let linear = device.create_sampler(&wgpu::SamplerDescriptor {
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("glyphium"),
            layout: &layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: globals.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&mask_atlas.create_view(&Default::default())) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&color_atlas.create_view(&Default::default())) },
                wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::Sampler(&nearest) },
                wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::Sampler(&linear) },
            ],
        });
        let capacity = 4096;
        let instances = instance_buffer(&device, capacity);
        Self {
            device,
            queue,
            pipeline,
            bind_group,
            globals,
            mask_atlas,
            color_atlas,
            instances,
            capacity,
            glyphs: Glyphs::new(fonts),
            rows: vec![],
            cursor: vec![],
            scratch: vec![],
            count: 0,
            clear: wgpu::Color::BLACK,
            viewport: (0, 0),
            rebuild_all: true,
            stats: Stats::default(),
        }
    }

    pub fn metrics(&self) -> CellMetrics {
        self.glyphs.metrics()
    }

    /// Pixel size that fits a whole frame.
    pub fn pixel_size(&self, cols: u16, rows: u16) -> (u32, u32) {
        let m = self.metrics();
        (m.width * cols as u32, m.height * rows as u32)
    }

    /// The next `update` rebuilds every row and re-uploads the instance buffer.
    pub fn invalidate(&mut self) {
        self.rebuild_all = true;
    }

    /// Brings GPU state up to date with `frame`. False means nothing changed since the last
    /// call, so the caller can skip drawing and presenting.
    pub fn update(&mut self, frame: &Frame) -> bool {
        let full = frame.full_redraw || self.rebuild_all || self.rows.len() != frame.rows.len();
        let any_dirty = frame.rows.iter().any(|r| r.dirty);
        let mut cursor = vec![];
        loop {
            let built = self.build_rows(frame, full).and_then(|()| {
                cursor.clear();
                self.build_cursor(frame, &mut cursor)
            });
            match built {
                Ok(()) => break,
                Err(AtlasFull) => {
                    // One attempt on an empty atlas per frame; `full` is rebuilt below.
                    self.glyphs.reset();
                    self.glyphs.evictions += 1;
                    self.stats.atlas_evictions += 1;
                    self.rows.clear();
                    self.rebuild_all = true;
                    if self.glyphs.evictions > 64 && self.stats.rows_rebuilt == 0 {
                        break;
                    }
                }
            }
        }
        let cursor_changed = !bytes_eq(&cursor, &self.cursor);
        let changed = full || any_dirty || cursor_changed || self.rebuild_all;
        self.rebuild_all = false;
        self.cursor = cursor;
        self.clear = wgpu::Color {
            r: frame.background.r as f64 / 255.0,
            g: frame.background.g as f64 / 255.0,
            b: frame.background.b as f64 / 255.0,
            a: 1.0,
        };
        self.flush_glyphs();
        if changed {
            self.upload_instances();
        }
        changed
    }

    fn flush_glyphs(&mut self) {
        for u in std::mem::take(&mut self.glyphs.uploads) {
            let (texture, bytes) = if u.color { (&self.color_atlas, 4) } else { (&self.mask_atlas, 1) };
            self.queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d { x: u.x, y: u.y, z: 0 },
                    aspect: wgpu::TextureAspect::All,
                },
                &u.data,
                wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(u.width * bytes), rows_per_image: None },
                wgpu::Extent3d { width: u.width, height: u.height, depth_or_array_layers: 1 },
            );
            self.stats.glyph_uploads += 1;
        }
    }

    fn upload_instances(&mut self) {
        self.scratch.clear();
        for r in &self.rows {
            self.scratch.extend_from_slice(&r.back);
        }
        for r in &self.rows {
            self.scratch.extend_from_slice(&r.front);
        }
        self.scratch.extend_from_slice(&self.cursor);
        let bytes: &[u8] = bytemuck::cast_slice(&self.scratch);
        if bytes.len() as u64 > self.capacity {
            self.capacity = (bytes.len() as u64).next_power_of_two();
            self.instances = instance_buffer(&self.device, self.capacity);
        }
        self.queue.write_buffer(&self.instances, 0, bytes);
        self.count = self.scratch.len() as u32;
        self.stats.buffer_uploads += 1;
    }

    fn build_rows(&mut self, frame: &Frame, full: bool) -> Result<(), AtlasFull> {
        let rebuild_everything = full || self.rows.len() != frame.rows.len();
        self.rows.resize_with(frame.rows.len(), RowInstances::default);
        let m = self.glyphs.metrics();
        let mut rows = std::mem::take(&mut self.rows);
        let mut result = Ok(());
        for (y, (row, out)) in frame.rows.iter().zip(rows.iter_mut()).enumerate() {
            if !(rebuild_everything || row.dirty) {
                continue;
            }
            out.back.clear();
            out.front.clear();
            let top = (y as u32 * m.height) as f32;
            for (x, cell) in row.cells.iter().enumerate() {
                if let Err(e) = self.push_cell(cell, frame, x as u32, top, m, out) {
                    result = Err(e);
                    break;
                }
            }
            self.stats.rows_rebuilt += 1;
        }
        self.rows = rows;
        result
    }

    fn push_cell(&mut self, cell: &Cell, frame: &Frame, x: u32, top: f32, m: CellMetrics, out: &mut RowInstances) -> Result<(), AtlasFull> {
        if cell.width == 0 {
            return Ok(());
        }
        let left = (x * m.width) as f32;
        let span = (m.width * cell.width as u32) as f32;
        let selected = cell.attrs & attrs::SELECTED != 0;
        let (fg, bg) = if selected { (frame.background, Some(frame.foreground)) } else { (cell.fg, cell.bg) };
        if let Some(bg) = bg {
            out.back.push(solid(left, top, span, m.height as f32, rgba(bg, 1.0)));
        }
        let alpha = if cell.attrs & attrs::FAINT != 0 { 0.5 } else { 1.0 };
        if !cell.text.is_empty() && cell.text != " " {
            let color = rgba(fg, alpha);
            if !push_cell_art(&cell.text, left, top, span, m, color, &mut out.front) {
                let style = (if cell.attrs & attrs::BOLD != 0 { STYLE_BOLD } else { 0 }) | (if cell.attrs & attrs::ITALIC != 0 { STYLE_ITALIC } else { 0 });
                for g in self.glyphs.cluster(&cell.text, style, cell.width)? {
                    out.front.push(glyph_instance(g, left, top, color));
                }
            }
        }
        push_decorations(cell, fg, alpha, left, top, span, m, &mut out.front);
        Ok(())
    }

    fn build_cursor(&mut self, frame: &Frame, out: &mut Vec<Instance>) -> Result<(), AtlasFull> {
        let c = frame.cursor;
        let (true, Some((cx, cy))) = (c.visible, c.pos) else { return Ok(()) };
        let Some(cell) = frame.rows.get(cy as usize).and_then(|r| r.cells.get(cx as usize)) else { return Ok(()) };
        let m = self.glyphs.metrics();
        let (left, top) = ((cx as u32 * m.width) as f32, (cy as u32 * m.height) as f32);
        let span = (m.width * cell.width.max(1) as u32) as f32;
        let (w, h) = (span, m.height as f32);
        let color = rgba(c.color.unwrap_or(frame.foreground), 1.0);
        match c.style {
            CursorStyle::Block => {
                out.push(solid(left, top, w, h, color));
                // The character under a block cursor is drawn again in the background colour.
                if !cell.text.is_empty() && cell.text != " " {
                    let under = rgba(frame.background, 1.0);
                    if !push_cell_art(&cell.text, left, top, span, m, under, out) {
                        let style = (if cell.attrs & attrs::BOLD != 0 { STYLE_BOLD } else { 0 }) | (if cell.attrs & attrs::ITALIC != 0 { STYLE_ITALIC } else { 0 });
                        for g in self.glyphs.cluster(&cell.text, style, cell.width.max(1))? {
                            out.push(glyph_instance(g, left, top, under));
                        }
                    }
                }
            }
            CursorStyle::BlockHollow => {
                out.push(solid(left, top, w, 1.0, color));
                out.push(solid(left, top + h - 1.0, w, 1.0, color));
                out.push(solid(left, top, 1.0, h, color));
                out.push(solid(left + w - 1.0, top, 1.0, h, color));
            }
            CursorStyle::Bar => out.push(solid(left, top, 2.0, h, color)),
            CursorStyle::Underline => out.push(solid(left, top + h - 2.0, w, 2.0, color)),
        }
        Ok(())
    }

    /// Records the frame's draw into `view`, which must be `viewport` pixels large.
    pub fn draw(&mut self, encoder: &mut wgpu::CommandEncoder, view: &wgpu::TextureView, viewport: (u32, u32)) {
        if self.viewport != viewport {
            self.viewport = viewport;
            self.queue.write_buffer(&self.globals, 0, bytemuck::cast_slice(&[viewport.0 as f32, viewport.1 as f32, 0.0, 0.0]));
        }
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("glyphium"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Clear(self.clear), store: wgpu::StoreOp::Store },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.bind_group, &[]);
        pass.set_vertex_buffer(0, self.instances.slice(..));
        pass.draw(0..4, 0..self.count);
        self.stats.draws += 1;
    }

    /// Renders to a texture and reads it back as tightly packed RGBA. For tests and
    /// screenshots; awaits the GPU, so it works natively and in the browser.
    pub async fn render_to_rgba(&mut self, viewport: (u32, u32)) -> Vec<u8> {
        let (w, h) = viewport;
        let size = wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 };
        let target = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("offscreen"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: OFFSCREEN_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let stride = (w * 4).next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
        let readback = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("readback"),
            size: (stride * h) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = self.device.create_command_encoder(&Default::default());
        self.draw(&mut encoder, &target.create_view(&Default::default()), viewport);
        encoder.copy_texture_to_buffer(
            target.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &readback,
                layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(stride), rows_per_image: None },
            },
            size,
        );
        self.queue.submit([encoder.finish()]);
        let (tx, rx) = futures_channel::oneshot::channel();
        let slice = readback.slice(..);
        slice.map_async(wgpu::MapMode::Read, move |r| {
            let _ = tx.send(r);
        });
        // A no-op on WebGPU, where the callback arrives from the browser's event loop.
        let _ = self.device.poll(wgpu::PollType::wait_indefinitely());
        rx.await.expect("map callback dropped").expect("map readback");
        let mapped = slice.get_mapped_range().expect("mapped range");
        let mut out = Vec::with_capacity((w * h * 4) as usize);
        for row in mapped.chunks(stride as usize) {
            out.extend_from_slice(&row[..(w * 4) as usize]);
        }
        drop(mapped);
        readback.unmap();
        out
    }
}

fn bytes_eq(a: &[Instance], b: &[Instance]) -> bool {
    bytemuck::cast_slice::<_, u8>(a) == bytemuck::cast_slice::<_, u8>(b)
}

fn instance_buffer(device: &wgpu::Device, capacity: u64) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("instances"),
        size: capacity,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

fn glyph_instance(g: &PlacedGlyph, left: f32, top: f32, color: [f32; 4]) -> Instance {
    Instance {
        pos: [left + g.left, top + g.top],
        size: [g.width, g.height],
        uv: g.uv,
        color: if g.color { [1.0, 1.0, 1.0, color[3]] } else { color },
        kind: if g.color { KIND_COLOR } else { KIND_MASK },
    }
}

#[allow(clippy::too_many_arguments)]
fn push_decorations(cell: &Cell, fg: Rgb, alpha: f32, left: f32, top: f32, span: f32, m: CellMetrics, out: &mut Vec<Instance>) {
    let a = cell.attrs;
    let line = rgba(cell.underline_color.unwrap_or(fg), alpha);
    let color = rgba(fg, alpha);
    let under = top + (m.baseline as f32 + 2.0).min(m.height as f32 - 1.0);
    if a & attrs::UNDERLINE != 0 {
        out.push(solid(left, under, span, 1.0, line));
    }
    if a & attrs::UNDERLINE_DOUBLE != 0 {
        out.push(solid(left, under - 1.0, span, 1.0, line));
        out.push(solid(left, (under + 1.0).min(top + m.height as f32 - 1.0), span, 1.0, line));
    }
    if a & attrs::UNDERLINE_DOTTED != 0 {
        let mut x = 0.0;
        while x < span {
            out.push(solid(left + x, under, 1.0, 1.0, line));
            x += 2.0;
        }
    }
    if a & attrs::UNDERLINE_DASHED != 0 {
        let dash = (span / 3.0).ceil();
        out.push(solid(left, under, dash, 1.0, line));
        out.push(solid(left + dash * 2.0, under, (span - dash * 2.0).max(0.0), 1.0, line));
    }
    if a & attrs::UNDERLINE_CURLY != 0 {
        // A stepped wave, continuous across cells because it is keyed to the column.
        let mut x = 0.0;
        while x < span {
            let phase = (left + x) as u32 / 2 % 4;
            let dy = [0.0, -1.0, 0.0, 1.0][phase as usize];
            out.push(solid(left + x, under + dy, 2.0f32.min(span - x), 1.0, line));
            x += 2.0;
        }
    }
    if a & attrs::STRIKE != 0 {
        out.push(solid(left, top + (m.height / 2) as f32, span, 1.0, color));
    }
    if a & attrs::OVERLINE != 0 {
        out.push(solid(left, top, span, 1.0, color));
    }
}

/// Block elements and light box lines are drawn as rectangles, so they join across cells
/// whatever the font's metrics. Returns false for anything else.
fn push_cell_art(text: &str, left: f32, top: f32, span: f32, m: CellMetrics, color: [f32; 4], out: &mut Vec<Instance>) -> bool {
    let mut chars = text.chars();
    let (Some(ch), None) = (chars.next(), chars.next()) else { return false };
    let (w, h) = (span, m.height as f32);
    let (cx, cy) = ((span / 2.0).floor(), (m.height / 2) as f32);
    match ch {
        '█' => out.push(solid(left, top, w, h, color)),
        '▀' => out.push(solid(left, top, w, cy, color)),
        '▄' => out.push(solid(left, top + cy, w, h - cy, color)),
        '▌' => out.push(solid(left, top, cx, h, color)),
        '▐' => out.push(solid(left + cx, top, w - cx, h, color)),
        '░' | '▒' | '▓' => {
            let a = match ch {
                '░' => 0.25,
                '▒' => 0.5,
                _ => 0.75,
            };
            out.push(solid(left, top, w, h, [color[0], color[1], color[2], color[3] * a]));
        }
        _ => {
            // (up, down, left, right) arms of the light box-drawing set.
            let arms = match ch {
                '─' => (false, false, true, true),
                '│' => (true, true, false, false),
                '┌' => (false, true, false, true),
                '┐' => (false, true, true, false),
                '└' => (true, false, false, true),
                '┘' => (true, false, true, false),
                '├' => (true, true, false, true),
                '┤' => (true, true, true, false),
                '┬' => (false, true, true, true),
                '┴' => (true, false, true, true),
                '┼' => (true, true, true, true),
                _ => return false,
            };
            if arms.0 {
                out.push(solid(left + cx, top, 1.0, cy + 1.0, color));
            }
            if arms.1 {
                out.push(solid(left + cx, top + cy, 1.0, h - cy, color));
            }
            if arms.2 {
                out.push(solid(left, top + cy, cx + 1.0, 1.0, color));
            }
            if arms.3 {
                out.push(solid(left + cx, top + cy, w - cx, 1.0, color));
            }
        }
    }
    true
}
