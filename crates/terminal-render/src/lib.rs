//! Draws [`Frame`]s with wgpu: instanced quads for backgrounds, decorations and glyphs.
//!
//! [`Renderer`] holds what panes share: the pipeline, fonts and glyph atlases. Each pane has a
//! [`PaneView`] with its instance lists per row, rebuilt only for dirty rows. Neither holds
//! terminal state or knows escape sequences. [`Renderer::update`] says whether anything
//! changed, so an idle terminal draws nothing, and lost GPU resources can be rebuilt from
//! frames at any time.

mod glyphs;

use glyphium_terminal_core::{Cell, CursorStyle, Frame, Rgb, attrs};
use wgpu::util::DeviceExt as _;

pub use glyphs::{CellMetrics, DEFAULT_FAMILY, DEFAULT_LINE_HEIGHT, FontConfig, ui_line_height};
use glyphs::{AtlasFull, COLOR_ATLAS_SIZE, Glyphs, MASK_ATLAS_SIZE, PlacedGlyph, STYLE_BOLD, STYLE_ITALIC};

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
const KIND_ROUNDED: u32 = 3;

/// Something drawn by an overlay view (window chrome, panels), in the view's own pixels.
/// Shapes draw in order, so later ones cover earlier ones. Colours are straight RGBA, 0 to 1.
#[derive(Clone, Debug, PartialEq)]
pub enum Shape {
    /// A filled rectangle with rounded corners (radius 0 for square).
    Rect { x: f32, y: f32, w: f32, h: f32, radius: f32, color: [f32; 4] },
    /// Only the outline of a rounded rectangle, `width` pixels wide, inside its bounds.
    Ring { x: f32, y: f32, w: f32, h: f32, radius: f32, width: f32, color: [f32; 4] },
    /// One line of text; `y` is the top of its line box ([`ui_line_height`] tall). `ui` uses
    /// the system interface font, otherwise the terminal font.
    Text { x: f32, y: f32, text: String, size_px: f32, bold: bool, ui: bool, color: [f32; 4] },
}

/// Straight RGBA from a colour and an alpha.
pub fn color(c: Rgb, a: f32) -> [f32; 4] {
    rgba(c, a)
}

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
    /// Frames that needed more glyphs than one page holds.
    pub atlas_overflows: u64,
    pub draws: u64,
}

pub struct Renderer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    mask_view: wgpu::TextureView,
    color_view: wgpu::TextureView,
    nearest: wgpu::Sampler,
    linear: wgpu::Sampler,
    mask_atlas: wgpu::Texture,
    color_atlas: wgpu::Texture,
    glyphs: Glyphs,
    /// Bumped when cached glyph positions die (eviction, font change): views rebuild.
    glyph_generation: u64,
    /// What the pipeline draws to; offscreen targets must match it.
    format: wgpu::TextureFormat,
    /// Opacity of each pane's default background, for translucent windows.
    background_alpha: f32,
    /// Opacity of the colour `draw` clears to.
    clear_alpha: f32,
    pub stats: Stats,
}

/// One pane's GPU state: instance lists and buffers, positioned by its own origin.
pub struct PaneView {
    instances: wgpu::Buffer,
    capacity: u64,
    globals: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    rows: Vec<RowInstances>,
    cursor: Vec<Instance>,
    scratch: Vec<Instance>,
    count: u32,
    background: [f32; 4],
    glyph_generation: u64,
    /// Set by `invalidate`: rebuild every row whatever the frame says.
    rebuild_all: bool,
    /// Pane views fill their whole rectangle with the background; overlays draw only shapes.
    fill: bool,
    /// Distance from the drawn rectangle's corner to the first cell: padding around the text,
    /// filled with the pane's background.
    inset: (f32, f32),
    /// The overlay's last shapes, so unchanged ones cost nothing.
    shapes: Vec<Shape>,
}

impl PaneView {
    /// The next `update` rebuilds every row and re-uploads the instance buffer.
    pub fn invalidate(&mut self) {
        self.rebuild_all = true;
    }

    /// Sets the padding between the drawn rectangle's corner and the first cell.
    pub fn set_inset(&mut self, x: f32, y: f32) {
        self.inset = (x, y);
    }
}

/// Where a pane appears in the target, in pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PaneRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

fn atlas(device: &wgpu::Device, size: u32, format: wgpu::TextureFormat, label: &str) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d { width: size, height: size, depth_or_array_layers: 1 },
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
        let (mask_size, color_size) = fonts.atlas_sizes.unwrap_or((MASK_ATLAS_SIZE, COLOR_ATLAS_SIZE));
        let mask_atlas = atlas(&device, mask_size, wgpu::TextureFormat::R8Unorm, "mask atlas");
        let color_atlas = atlas(&device, color_size, wgpu::TextureFormat::Rgba8Unorm, "color atlas");
        let nearest = device.create_sampler(&wgpu::SamplerDescriptor::default());
        let linear = device.create_sampler(&wgpu::SamplerDescriptor {
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let mask_view = mask_atlas.create_view(&Default::default());
        let color_view = color_atlas.create_view(&Default::default());
        Self {
            device,
            queue,
            pipeline,
            layout,
            mask_view,
            color_view,
            nearest,
            linear,
            mask_atlas,
            color_atlas,
            glyphs: Glyphs::new(fonts),
            glyph_generation: 0,
            format,
            background_alpha: 1.0,
            clear_alpha: 1.0,
            stats: Stats::default(),
        }
    }

    pub fn new_view(&self) -> PaneView {
        let capacity = 4096;
        let globals = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("globals"),
            contents: bytemuck::cast_slice(&[1.0f32, 1.0, 0.0, 0.0]),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("glyphium pane"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: globals.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&self.mask_view) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&self.color_view) },
                wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::Sampler(&self.nearest) },
                wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::Sampler(&self.linear) },
            ],
        });
        PaneView {
            instances: instance_buffer(&self.device, capacity),
            capacity,
            globals,
            bind_group,
            rows: vec![],
            cursor: vec![],
            scratch: vec![],
            count: 0,
            background: [0.0, 0.0, 0.0, 1.0],
            glyph_generation: self.glyph_generation,
            rebuild_all: true,
            fill: true,
            inset: (0.0, 0.0),
            shapes: vec![],
        }
    }

    /// A view for [`Renderer::update_overlay`]: shapes and text, no background fill.
    pub fn new_overlay(&self) -> PaneView {
        let mut view = self.new_view();
        view.fill = false;
        view
    }

    /// Opacity of the panes' default background (1 is opaque). Cells with their own
    /// background, the selection and the cursor stay opaque. Views rebuild on their next update.
    pub fn set_background_alpha(&mut self, alpha: f32) {
        let alpha = alpha.clamp(0.0, 1.0);
        if alpha != self.background_alpha {
            self.background_alpha = alpha;
            self.glyph_generation += 1;
        }
    }

    /// Opacity of the colour `draw` clears to, for a window the desktop shows through.
    pub fn set_clear_alpha(&mut self, alpha: f32) {
        self.clear_alpha = alpha.clamp(0.0, 1.0);
    }

    /// Advance width of a line of text, for laying out shapes.
    pub fn text_width(&mut self, text: &str, size_px: f32, bold: bool, ui: bool) -> f32 {
        loop {
            match self.glyphs.line(text, size_px, bold, ui) {
                Ok((_, w)) => return w,
                Err(AtlasFull) => {
                    self.glyphs.reset();
                    self.glyph_generation += 1;
                    self.stats.atlas_evictions += 1;
                }
            }
        }
    }

    /// The interface font family in use for `ui` text.
    pub fn ui_family(&self) -> &str {
        self.glyphs.ui_family()
    }

    /// Brings an overlay view up to date with `shapes`. False when nothing changed, so an idle
    /// window still draws nothing. Overlay text shares the glyph atlas, so it rebuilds after
    /// an eviction or font change too.
    pub fn update_overlay(&mut self, view: &mut PaneView, shapes: &[Shape]) -> bool {
        if view.glyph_generation == self.glyph_generation && !view.rebuild_all && view.shapes == shapes {
            return false;
        }
        self.glyphs.draw_what_fits = false;
        let mut out = vec![];
        loop {
            out.clear();
            match self.build_shapes(shapes, &mut out) {
                Ok(()) => break,
                Err(AtlasFull) => {
                    // A second failure after a reset draws what fits rather than looping.
                    self.glyphs.draw_what_fits = true;
                    self.glyphs.reset();
                    self.glyph_generation += 1;
                    self.stats.atlas_evictions += 1;
                }
            }
        }
        view.rows.clear();
        view.rows.push(RowInstances { back: out, front: vec![] });
        view.cursor.clear();
        view.shapes = shapes.to_vec();
        view.glyph_generation = self.glyph_generation;
        view.rebuild_all = false;
        self.flush_glyphs();
        self.upload_instances(view);
        true
    }

    fn build_shapes(&mut self, shapes: &[Shape], out: &mut Vec<Instance>) -> Result<(), AtlasFull> {
        for shape in shapes {
            match shape {
                Shape::Rect { x, y, w, h, radius, color } => {
                    out.push(Instance { pos: [*x, *y], size: [*w, *h], uv: [*radius, 0.0, 0.0, 0.0], color: *color, kind: KIND_ROUNDED });
                }
                Shape::Ring { x, y, w, h, radius, width, color } => {
                    out.push(Instance { pos: [*x, *y], size: [*w, *h], uv: [*radius, width.max(0.01), 0.0, 0.0], color: *color, kind: KIND_ROUNDED });
                }
                Shape::Text { x, y, text, size_px, bold, ui, color } => {
                    let (glyphs, _) = self.glyphs.line(text, *size_px, *bold, *ui)?;
                    let (left, top) = (x.round(), y.round());
                    for g in glyphs {
                        out.push(glyph_instance(g, left, top, *color));
                    }
                }
            }
        }
        Ok(())
    }

    /// Changes the font size (zoom, DPI). Views rebuild on their next `update`; cell metrics
    /// change, so the caller also resizes its terminals.
    pub fn set_font_size(&mut self, size_px: f32) {
        self.glyphs.set_size(size_px);
        self.glyph_generation += 1;
    }

    /// Changes the terminal font family and line height at `size_px`. False if the family is
    /// not loaded and the bundled one is used instead. Views rebuild as for a size change.
    pub fn set_font(&mut self, family: Option<&str>, line_height: f32, size_px: f32) -> bool {
        let found = self.glyphs.set_family(family, line_height, size_px);
        self.glyph_generation += 1;
        found
    }

    /// The family actually in use.
    pub fn font_family(&self) -> &str {
        self.glyphs.family()
    }

    /// Makes the families in a font file available to [`Renderer::set_font`].
    pub fn add_font(&mut self, data: Vec<u8>) {
        self.glyphs.add_font(data);
    }

    pub fn monospace_families(&self) -> Vec<String> {
        self.glyphs.monospace_families()
    }

    pub fn device(&self) -> &wgpu::Device {
        &self.device
    }

    pub fn queue(&self) -> &wgpu::Queue {
        &self.queue
    }

    pub fn metrics(&self) -> CellMetrics {
        self.glyphs.metrics()
    }

    /// Pixel size that fits a whole frame.
    pub fn pixel_size(&self, cols: u16, rows: u16) -> (u32, u32) {
        let m = self.metrics();
        (m.width * cols as u32, m.height * rows as u32)
    }

    /// Brings a view up to date with `frame`. False means nothing changed since the last call,
    /// so the caller can skip drawing and presenting.
    pub fn update(&mut self, view: &mut PaneView, frame: &Frame) -> bool {
        let mut stale = view.rebuild_all || view.glyph_generation != self.glyph_generation;
        let any_dirty = frame.rows.iter().any(|r| r.dirty);
        let mut full = frame.full_redraw || stale || view.rows.len() != frame.rows.len();
        let mut cursor = vec![];
        // A full page empties and the frame retries once. If the frame alone needs more than a
        // page, it draws what fits: a bounded picture beats a hang, whatever the output is.
        self.glyphs.draw_what_fits = false;
        loop {
            let built = self.build_rows(view, frame, full).and_then(|()| {
                cursor.clear();
                self.build_cursor(frame, &mut cursor)
            });
            match built {
                Ok(()) => break,
                Err(AtlasFull) => {
                    self.glyphs.draw_what_fits = full;
                    if full {
                        self.stats.atlas_overflows += 1;
                    }
                    self.glyphs.reset();
                    self.glyph_generation += 1;
                    self.stats.atlas_evictions += 1;
                    stale = true;
                    full = true;
                }
            }
        }
        view.glyph_generation = self.glyph_generation;
        let changed = full || any_dirty || stale || !bytes_eq(&cursor, &view.cursor);
        view.rebuild_all = false;
        view.cursor = cursor;
        view.background = rgba(frame.background, self.background_alpha);
        self.flush_glyphs();
        if changed {
            self.upload_instances(view);
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

    fn upload_instances(&mut self, view: &mut PaneView) {
        view.scratch.clear();
        // Fills the pane, including any pixels left over below or beside the last cell; the
        // draw's scissor rectangle clips it to the pane.
        // Starts left of and above the origin to cover the padding too.
        if view.fill {
            view.scratch.push(solid(-1024.0, -1024.0, 65536.0, 65536.0, view.background));
        }
        for r in &view.rows {
            view.scratch.extend_from_slice(&r.back);
        }
        for r in &view.rows {
            view.scratch.extend_from_slice(&r.front);
        }
        view.scratch.extend_from_slice(&view.cursor);
        let bytes: &[u8] = bytemuck::cast_slice(&view.scratch);
        if bytes.len() as u64 > view.capacity {
            view.capacity = (bytes.len() as u64).next_power_of_two();
            view.instances = instance_buffer(&self.device, view.capacity);
        }
        self.queue.write_buffer(&view.instances, 0, bytes);
        view.count = view.scratch.len() as u32;
        self.stats.buffer_uploads += 1;
    }

    fn build_rows(&mut self, view: &mut PaneView, frame: &Frame, full: bool) -> Result<(), AtlasFull> {
        let rebuild_everything = full || view.rows.len() != frame.rows.len();
        view.rows.resize_with(frame.rows.len(), RowInstances::default);
        let m = self.glyphs.metrics();
        for (y, (row, out)) in frame.rows.iter().zip(view.rows.iter_mut()).enumerate() {
            if !(rebuild_everything || row.dirty) {
                continue;
            }
            out.back.clear();
            out.front.clear();
            let top = (y as u32 * m.height) as f32;
            for (x, cell) in row.cells.iter().enumerate() {
                self.push_cell(cell, frame, x as u32, top, m, out)?;
            }
            self.stats.rows_rebuilt += 1;
        }
        Ok(())
    }

    fn push_cell(&mut self, cell: &Cell, frame: &Frame, x: u32, top: f32, m: CellMetrics, out: &mut RowInstances) -> Result<(), AtlasFull> {
        if cell.width == 0 {
            return Ok(());
        }
        let left = (x * m.width) as f32;
        let span = (m.width * cell.width as u32) as f32;
        let selected = cell.attrs & attrs::SELECTED != 0;
        let (fg, bg) = match (selected, frame.selection) {
            (true, Some(selection)) => (cell.fg, Some(selection)),
            (true, None) => (frame.background, Some(frame.foreground)),
            (false, _) => (cell.fg, cell.bg),
        };
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

    /// Records a draw of `panes` into `target` (`viewport` pixels): clears to `clear`, then
    /// fills and draws each pane in its rectangle. Hidden panes are simply not listed.
    pub fn draw(&mut self, encoder: &mut wgpu::CommandEncoder, target: &wgpu::TextureView, viewport: (u32, u32), clear: Rgb, panes: &[(&PaneView, PaneRect)]) {
        for (view, rect) in panes {
            // Offset through the projection: x and y in a pane's own pixels stay valid.
            let globals = [viewport.0 as f32, viewport.1 as f32, rect.x as f32 + view.inset.0, rect.y as f32 + view.inset.1];
            self.queue.write_buffer(&view.globals, 0, bytemuck::cast_slice(&globals));
        }
        // The clear colour skips the shader, so it is premultiplied here.
        let a = self.clear_alpha;
        let c = [clear.r as f32 / 255.0 * a, clear.g as f32 / 255.0 * a, clear.b as f32 / 255.0 * a, a];
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("glyphium"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color { r: c[0] as f64, g: c[1] as f64, b: c[2] as f64, a: c[3] as f64 }),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&self.pipeline);
        for (view, rect) in panes {
            let x2 = (rect.x + rect.width).min(viewport.0);
            let y2 = (rect.y + rect.height).min(viewport.1);
            if rect.x >= x2 || rect.y >= y2 {
                continue;
            }
            pass.set_scissor_rect(rect.x, rect.y, x2 - rect.x, y2 - rect.y);
            pass.set_bind_group(0, &view.bind_group, &[]);
            pass.set_vertex_buffer(0, view.instances.slice(..));
            pass.draw(0..4, 0..view.count);
        }
        self.stats.draws += 1;
    }

    /// Renders one pane to a texture and reads it back as tightly packed RGBA. For tests and
    /// screenshots; awaits the GPU, so it works natively and in the browser.
    pub async fn render_to_rgba(&mut self, view: &PaneView, background: Rgb, viewport: (u32, u32)) -> Vec<u8> {
        let rect = PaneRect { x: 0, y: 0, width: viewport.0, height: viewport.1 };
        self.render_panes_to_rgba(background, viewport, &[(view, rect)]).await
    }

    pub async fn render_panes_to_rgba(&mut self, clear: Rgb, viewport: (u32, u32), panes: &[(&PaneView, PaneRect)]) -> Vec<u8> {
        let (w, h) = viewport;
        let size = wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 };
        let target = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("offscreen"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: self.format,
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
        self.draw(&mut encoder, &target.create_view(&Default::default()), viewport, clear, panes);
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
        if matches!(self.format, wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Bgra8UnormSrgb) {
            out.as_chunks_mut::<4>().0.iter_mut().for_each(|px| px.swap(0, 2));
        }
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
