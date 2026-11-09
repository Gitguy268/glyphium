//! Fonts, shaping and the glyph atlases. Clusters are shaped once and cached, so a repeated
//! cluster costs one hash lookup. Each atlas is one bounded page: when it fills, the whole
//! cache is dropped and the frame is rebuilt from CPU state.

use std::collections::HashMap;

use cosmic_text::{Attrs, Buffer, Family, FontSystem, Metrics, Shaping, Style, SwashCache, SwashContent, Weight, Wrap, fontdb};

/// Mask page: room for about 16,000 cells of double-width glyphs at 32 px, more than a screen.
pub const MASK_ATLAS_SIZE: u32 = 4096;
/// Colour page: four bytes a texel, and emoji are few and scaled from one strike.
pub const COLOR_ATLAS_SIZE: u32 = 2048;
/// The bundled family, used when no other is chosen or the chosen one is not installed.
pub const DEFAULT_FAMILY: &str = "JetBrains Mono NL";
/// Cell height as a multiple of the font size, unless the config says otherwise.
pub const DEFAULT_LINE_HEIGHT: f32 = 1.2;

const BUNDLED: [&[u8]; 4] = [
    include_bytes!("../fonts/JetBrainsMonoNL-Regular.ttf"),
    include_bytes!("../fonts/JetBrainsMonoNL-Bold.ttf"),
    include_bytes!("../fonts/JetBrainsMonoNL-Italic.ttf"),
    include_bytes!("../fonts/JetBrainsMonoNL-BoldItalic.ttf"),
];

/// Which fonts to use. The bundled mono family is always present and comes first; browsers
/// have no font scan, so wasm builds pass any fallback fonts as bytes.
#[derive(Clone, Debug)]
pub struct FontConfig {
    pub size_px: f32,
    /// Font family for terminal text; `None` or a family that is not loaded uses the bundled one.
    pub family: Option<String>,
    /// Cell height as a multiple of `size_px`.
    pub line_height: f32,
    pub extra_fonts: Vec<Vec<u8>>,
    /// Scan installed fonts for fallback (native only). Off for deterministic output.
    pub system_fonts: bool,
    /// Overrides the page sizes, so tests can fill them quickly.
    pub atlas_sizes: Option<(u32, u32)>,
}

impl Default for FontConfig {
    fn default() -> Self {
        Self { size_px: 0.0, family: None, line_height: DEFAULT_LINE_HEIGHT, extra_fonts: vec![], system_fonts: false, atlas_sizes: None }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CellMetrics {
    pub width: u32,
    pub height: u32,
    /// Baseline offset from the top of the cell.
    pub baseline: i32,
}

#[derive(Clone, Copy, Debug)]
pub struct PlacedGlyph {
    pub left: f32,
    pub top: f32,
    pub width: f32,
    pub height: f32,
    pub uv: [f32; 4],
    pub color: bool,
}

/// Bytes to upload to an atlas. The renderer owns the GPU textures.
pub struct Upload {
    pub color: bool,
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
    pub data: Vec<u8>,
}

struct Shelf {
    size: u32,
    x: u32,
    y: u32,
    height: u32,
}

impl Shelf {
    fn new(size: u32) -> Self {
        Self { size, x: 0, y: 0, height: 0 }
    }

    fn fits_alone(&self, w: u32, h: u32) -> bool {
        w < self.size && h < self.size
    }

    /// One pixel of padding keeps linear sampling from bleeding across glyphs.
    fn alloc(&mut self, w: u32, h: u32) -> Option<(u32, u32)> {
        if self.x + w + 1 > self.size {
            self.y += self.height;
            self.x = 0;
            self.height = 0;
        }
        if self.y + h + 1 > self.size {
            return None;
        }
        let at = (self.x, self.y);
        self.x += w + 1;
        self.height = self.height.max(h + 1);
        Some(at)
    }
}

/// The atlas page is full: drop the cache and rebuild.
pub struct AtlasFull;

pub struct Glyphs {
    fonts: FontSystem,
    /// The family in use: the configured one if loaded, else the bundled one.
    family: String,
    line_height: f32,
    swash: SwashCache,
    buffer: Buffer,
    metrics: CellMetrics,
    clusters: HashMap<(String, u8), Vec<PlacedGlyph>>,
    mask_shelf: Shelf,
    color_shelf: Shelf,
    pub uploads: Vec<Upload>,
    /// One frame needs more glyphs than a page holds: leave the rest out instead of
    /// evicting for ever. Cleared by the renderer at the start of each update.
    pub draw_what_fits: bool,
}

pub const STYLE_BOLD: u8 = 1;
pub const STYLE_ITALIC: u8 = 2;

fn attrs(style: u8, family: &str) -> Attrs<'_> {
    let mut a = Attrs::new().family(Family::Name(family));
    if style & STYLE_BOLD != 0 {
        a = a.weight(Weight::BOLD);
    }
    if style & STYLE_ITALIC != 0 {
        a = a.style(Style::Italic);
    }
    a
}

/// The cell comes from the regular face: 'M' advance, the line height, whole pixels.
fn measure(fonts: &mut FontSystem, size_px: f32, family: &str, line_height: f32) -> (Buffer, CellMetrics) {
    let line = (size_px * line_height).ceil();
    let mut buffer = Buffer::new(fonts, Metrics::new(size_px, line));
    buffer.set_wrap(Wrap::None);
    buffer.set_size(None, Some(line * 2.0));
    buffer.set_text("M", &attrs(0, family), Shaping::Advanced, None);
    buffer.shape_until_scroll(fonts, false);
    let run = buffer.layout_runs().next().expect("bundled font shapes M");
    // Extra line height is shared above and below the text, as other terminals do.
    let metrics = CellMetrics { width: run.glyphs[0].w.ceil() as u32, height: line as u32, baseline: run.line_y.round() as i32 };
    (buffer, metrics)
}

/// The configured family when a loaded face has it (matching case-insensitively), else the
/// bundled one.
fn pick_family(fonts: &FontSystem, wanted: Option<&str>) -> String {
    wanted
        .and_then(|w| fonts.db().faces().flat_map(|f| f.families.iter()).find(|(n, _)| n.eq_ignore_ascii_case(w)).map(|(n, _)| n.clone()))
        .unwrap_or_else(|| DEFAULT_FAMILY.to_string())
}

fn sane_line_height(v: f32) -> f32 {
    if v.is_finite() && (0.8..=3.0).contains(&v) { v } else { DEFAULT_LINE_HEIGHT }
}

impl Glyphs {
    pub fn new(config: &FontConfig) -> Self {
        let mut db = fontdb::Database::new();
        for f in BUNDLED {
            db.load_font_data(f.to_vec());
        }
        for f in &config.extra_fonts {
            db.load_font_data(f.clone());
        }
        #[cfg(all(feature = "system-fonts", not(target_family = "wasm")))]
        if config.system_fonts {
            db.load_system_fonts();
        }
        let mut fonts = FontSystem::new_with_locale_and_db("en-US".into(), db);
        let family = pick_family(&fonts, config.family.as_deref());
        let line_height = sane_line_height(config.line_height);
        let (buffer, metrics) = measure(&mut fonts, config.size_px, &family, line_height);
        let (mask_size, color_size) = config.atlas_sizes.unwrap_or((MASK_ATLAS_SIZE, COLOR_ATLAS_SIZE));
        Self {
            fonts,
            family,
            line_height,
            swash: SwashCache::new(),
            buffer,
            metrics,
            clusters: HashMap::new(),
            mask_shelf: Shelf::new(mask_size),
            color_shelf: Shelf::new(color_size),
            uploads: vec![],
            draw_what_fits: false,
        }
    }

    pub fn set_size(&mut self, size_px: f32) {
        let (buffer, metrics) = measure(&mut self.fonts, size_px, &self.family, self.line_height);
        self.buffer = buffer;
        self.metrics = metrics;
        self.reset();
    }

    pub fn metrics(&self) -> CellMetrics {
        self.metrics
    }

    /// Switches family and line height at the current size. Returns whether the family was
    /// found; if not, the bundled family is used.
    pub fn set_family(&mut self, family: Option<&str>, line_height: f32, size_px: f32) -> bool {
        self.family = pick_family(&self.fonts, family);
        self.line_height = sane_line_height(line_height);
        self.set_size(size_px);
        family.is_none_or(|f| self.family.eq_ignore_ascii_case(f))
    }

    pub fn family(&self) -> &str {
        &self.family
    }

    /// Adds font data (a font file the caller read), so its families can be chosen.
    pub fn add_font(&mut self, data: Vec<u8>) {
        self.fonts.db_mut().load_font_data(data);
    }

    /// Monospaced families that are loaded, sorted, for a font picker.
    pub fn monospace_families(&self) -> Vec<String> {
        let mut names: Vec<String> = self.fonts.db().faces().filter(|f| f.monospaced).filter_map(|f| f.families.first().map(|(n, _)| n.clone())).filter(|n| !n.starts_with('.')).collect();
        names.sort_by_key(|n| n.to_lowercase());
        names.dedup();
        names
    }

    /// Drops every cached cluster and resets the atlases, for eviction and device loss.
    pub fn reset(&mut self) {
        self.clusters.clear();
        self.mask_shelf = Shelf::new(self.mask_shelf.size);
        self.color_shelf = Shelf::new(self.color_shelf.size);
        self.uploads.clear();
    }

    /// Glyphs of one cluster, relative to its cell's top left. `columns` is its cell width;
    /// colour bitmaps (emoji strikes come in one large size) are scaled to fit that box.
    pub fn cluster(&mut self, text: &str, style: u8, columns: u8) -> Result<&[PlacedGlyph], AtlasFull> {
        let key = (text.to_string(), style | (columns.min(2) << 4));
        if !self.clusters.contains_key(&key) {
            match self.rasterize(text, style, columns) {
                Ok(placed) => {
                    self.clusters.insert(key.clone(), placed);
                }
                Err(AtlasFull) if self.draw_what_fits => return Ok(&[]),
                Err(AtlasFull) => return Err(AtlasFull),
            }
        }
        Ok(&self.clusters[&key])
    }

    fn rasterize(&mut self, text: &str, style: u8, columns: u8) -> Result<Vec<PlacedGlyph>, AtlasFull> {
        self.buffer.set_text(text, &attrs(style, &self.family), Shaping::Advanced, None);
        self.buffer.shape_until_scroll(&mut self.fonts, false);
        let m = self.metrics;
        let box_w = (m.width * columns.max(1) as u32) as f32;
        let mut physical = vec![];
        for run in self.buffer.layout_runs() {
            for g in run.glyphs {
                physical.push(g.physical((0.0, 0.0), 1.0));
            }
        }
        let mut out = vec![];
        for p in physical {
            let Some(image) = self.swash.get_image(&mut self.fonts, p.cache_key).as_ref() else { continue };
            let (w, h) = (image.placement.width, image.placement.height);
            if w == 0 || h == 0 {
                continue;
            }
            let (shelf, color, data) = match image.content {
                SwashContent::Mask => (&mut self.mask_shelf, false, image.data.clone()),
                SwashContent::Color => (&mut self.color_shelf, true, image.data.clone()),
                // Format::Alpha is requested, so subpixel masks are not produced.
                SwashContent::SubpixelMask => continue,
            };
            let (left, top) = (image.placement.left, image.placement.top);
            if !shelf.fits_alone(w, h) {
                continue; // Larger than a whole page; retrying on an empty atlas cannot help.
            }
            let page = shelf.size as f32;
            let (x, y) = shelf.alloc(w, h).ok_or(AtlasFull)?;
            self.uploads.push(Upload { color, x, y, width: w, height: h, data });
            let uv = [x as f32 / page, y as f32 / page, w as f32 / page, h as f32 / page];
            let placed = if color {
                let scale = (box_w / w as f32).min(m.height as f32 / h as f32).min(1.0);
                let (sw, sh) = (w as f32 * scale, h as f32 * scale);
                PlacedGlyph { left: (box_w - sw) / 2.0, top: (m.height as f32 - sh) / 2.0, width: sw, height: sh, uv, color }
            } else {
                PlacedGlyph { left: (p.x + left) as f32, top: (m.baseline + p.y - top) as f32, width: w as f32, height: h as f32, uv, color }
            };
            out.push(placed);
        }
        Ok(out)
    }
}
