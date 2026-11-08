//! Terminal state over libghostty-vt. Input: bytes and resizes. Output: [`Frame`]s for the
//! renderer and reply bytes for the application. No clock, filesystem, network or OS calls,
//! so the same bytes give the same state natively and on wasm32.
//!
//! All `unsafe` calls into the C API live in this crate. Frames are read through Ghostty's
//! render-state API, which reports dirty rows, so an unchanged row costs nothing.

pub mod input;
pub mod theme;

pub use theme::Theme;

use std::ffi::c_void;
use std::mem::{MaybeUninit, size_of};
use std::ptr;

use glyphium_core_ffi::*;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl From<GhosttyColorRgb> for Rgb {
    fn from(c: GhosttyColorRgb) -> Self {
        Rgb { r: c.r, g: c.g, b: c.b }
    }
}

pub mod attrs {
    pub const BOLD: u16 = 1;
    pub const ITALIC: u16 = 1 << 1;
    pub const FAINT: u16 = 1 << 2;
    pub const STRIKE: u16 = 1 << 3;
    pub const OVERLINE: u16 = 1 << 4;
    pub const BLINK: u16 = 1 << 5;
    pub const UNDERLINE: u16 = 1 << 6;
    pub const UNDERLINE_DOUBLE: u16 = 1 << 7;
    pub const UNDERLINE_CURLY: u16 = 1 << 8;
    pub const UNDERLINE_DOTTED: u16 = 1 << 9;
    pub const UNDERLINE_DASHED: u16 = 1 << 10;
    pub const SELECTED: u16 = 1 << 11;
}

/// One column of a row, with SGR already resolved to final colours: inverse is applied and
/// invisible text removed, so the renderer needs no escape-sequence semantics.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Cell {
    /// Whole grapheme cluster; empty for a blank cell or the tail of a wide character.
    pub text: String,
    /// Columns covered: 1, 2 for a wide character, 0 for its tail.
    pub width: u8,
    pub fg: Rgb,
    /// `None` is the default background.
    pub bg: Option<Rgb>,
    pub underline_color: Option<Rgb>,
    pub attrs: u16,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Row {
    pub cells: Vec<Cell>,
    /// Changed since the last frame was consumed.
    pub dirty: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CursorStyle {
    Bar,
    #[default]
    Block,
    Underline,
    BlockHollow,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Cursor {
    /// Position when the cursor is inside the viewport.
    pub pos: Option<(u16, u16)>,
    pub visible: bool,
    pub blinking: bool,
    pub style: CursorStyle,
    pub color: Option<Rgb>,
}

/// Everything the renderer needs for one frame. Kept between updates: only dirty rows are
/// rewritten, so holding on to a `Frame` is what makes updates incremental.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Frame {
    pub cols: u16,
    pub rows: Vec<Row>,
    pub cursor: Cursor,
    pub background: Rgb,
    pub foreground: Rgb,
    /// Background of selected text. `None` draws selections as inverted colours.
    pub selection: Option<Rgb>,
    /// Which `set_theme` call the cells were read under; a change forces a full redraw.
    pub theme_generation: u64,
    /// The whole frame must be redrawn (resize, screen switch, palette change).
    pub full_redraw: bool,
}

pub(crate) fn check(r: GhosttyResult, what: &str) {
    assert_eq!(r, GHOSTTY_SUCCESS, "{what} failed: {r}");
}

/// `ghostty_mode_new` is a C inline, so bindgen cannot see it: value in the low 15 bits, the
/// ANSI flag in the top one. See `ghostty/vt/modes.h`.
pub(crate) const fn mode(value: u16, ansi: bool) -> GhosttyMode {
    (value & 0x7fff) | ((ansi as u16) << 15)
}

const MODE_GRAPHEME_CLUSTER: GhosttyMode = mode(2027, false);
pub(crate) const MODE_BRACKETED_PASTE: GhosttyMode = mode(2004, false);

pub(crate) unsafe fn zeroed<T>() -> T {
    unsafe { MaybeUninit::zeroed().assume_init() }
}

unsafe extern "C" fn write_pty(_t: GhosttyTerminal, userdata: *mut c_void, data: *const u8, len: usize) {
    let out = unsafe { &mut *(userdata as *mut Vec<u8>) };
    out.extend_from_slice(unsafe { std::slice::from_raw_parts(data, len) });
}

pub struct Terminal {
    term: GhosttyTerminal,
    render: GhosttyRenderState,
    row_iter: GhosttyRenderStateRowIterator,
    row_cells: GhosttyRenderStateRowCells,
    // Boxed so the pointer handed to the write-pty callback stays put.
    #[allow(clippy::box_collection)]
    replies: Box<Vec<u8>>,
    encoders: input::Encoders,
    force_dirty: Vec<usize>,
    selection: Option<Rgb>,
    theme_generation: u64,
}

impl Terminal {
    /// `scrollback` is in lines. Ghostty's separate byte limit is removed so that the line
    /// limit governs; without this its default keeps only a few hundred lines.
    pub fn new(cols: u16, rows: u16, scrollback: usize) -> Self {
        let mut term: GhosttyTerminal = ptr::null_mut();
        let mut render: GhosttyRenderState = ptr::null_mut();
        let mut row_iter: GhosttyRenderStateRowIterator = ptr::null_mut();
        let mut row_cells: GhosttyRenderStateRowCells = ptr::null_mut();
        let mut replies = Box::new(Vec::new());
        unsafe {
            check(ghostty_terminal_new(ptr::null(), &mut term, cols, rows), "ghostty_terminal_new");
            check(ghostty_render_state_new(ptr::null(), &mut render), "ghostty_render_state_new");
            check(ghostty_render_state_row_iterator_new(ptr::null(), &mut row_iter), "row_iterator_new");
            check(ghostty_render_state_row_cells_new(ptr::null(), &mut row_cells), "row_cells_new");
            check(ghostty_terminal_set(term, GHOSTTY_TERMINAL_OPT_USERDATA, &mut *replies as *mut Vec<u8> as *const c_void), "userdata");
            let cb: GhosttyTerminalWritePtyFn = Some(write_pty);
            check(ghostty_terminal_set(term, GHOSTTY_TERMINAL_OPT_WRITE_PTY, cb.map_or(ptr::null(), |f| f as *const c_void)), "write_pty");
            check(ghostty_terminal_set(term, GHOSTTY_TERMINAL_OPT_SCROLLBACK_MAX_LINES, &scrollback as *const usize as *const c_void), "scrollback lines");
            check(ghostty_terminal_set(term, GHOSTTY_TERMINAL_OPT_SCROLLBACK_MAX_BYTES, ptr::null()), "scrollback bytes");
            // Grapheme clustering on by default and across resets, as in the Ghostty app.
            let cluster = GhosttyTerminalModeConfig { mode: MODE_GRAPHEME_CLUSTER, value: true };
            check(ghostty_terminal_set(term, GHOSTTY_TERMINAL_OPT_MODE_DEFAULT, &cluster as *const _ as *const c_void), "mode 2027");
        }
        Self { term, render, row_iter, row_cells, replies, encoders: input::Encoders::new(), force_dirty: vec![], selection: None, theme_generation: 0 }
    }

    /// Sets the default colours, or restores Ghostty's with `None`. Colours a program set
    /// through OSC stay until it resets them.
    pub fn set_theme(&mut self, theme: Option<&Theme>) {
        let raw = |c: Rgb| GhosttyColorRgb { r: c.r, g: c.g, b: c.b };
        let set = |opt, value: *const c_void| check(unsafe { ghostty_terminal_set(self.term, opt, value) }, "theme colour");
        match theme {
            Some(t) => {
                let mut palette: [GhosttyColorRgb; 256] = self.get(GHOSTTY_TERMINAL_DATA_COLOR_PALETTE_DEFAULT);
                for (slot, c) in palette.iter_mut().zip(t.ansi) {
                    *slot = raw(c);
                }
                set(GHOSTTY_TERMINAL_OPT_COLOR_FOREGROUND, &raw(t.foreground) as *const _ as *const c_void);
                set(GHOSTTY_TERMINAL_OPT_COLOR_BACKGROUND, &raw(t.background) as *const _ as *const c_void);
                set(GHOSTTY_TERMINAL_OPT_COLOR_CURSOR, &raw(t.cursor) as *const _ as *const c_void);
                set(GHOSTTY_TERMINAL_OPT_COLOR_PALETTE, &palette as *const _ as *const c_void);
            }
            None => {
                for opt in [GHOSTTY_TERMINAL_OPT_COLOR_FOREGROUND, GHOSTTY_TERMINAL_OPT_COLOR_BACKGROUND, GHOSTTY_TERMINAL_OPT_COLOR_CURSOR, GHOSTTY_TERMINAL_OPT_COLOR_PALETTE] {
                    set(opt, ptr::null());
                }
            }
        }
        self.selection = theme.map(|t| t.selection);
        self.theme_generation += 1;
    }

    /// Feeds application output. Any chunking gives the same state (checked by `bench/bakeoff`).
    pub fn write(&mut self, bytes: &[u8]) {
        unsafe { ghostty_terminal_vt_write(self.term, bytes.as_ptr(), bytes.len()) };
    }

    /// Bytes the terminal wants sent back to the application, in order.
    pub fn take_replies(&mut self) -> Vec<u8> {
        std::mem::take(&mut *self.replies)
    }

    pub fn resize(&mut self, cols: u16, rows: u16, cell_width_px: u32, cell_height_px: u32) {
        check(unsafe { ghostty_terminal_resize(self.term, cols, rows, cell_width_px, cell_height_px) }, "ghostty_terminal_resize");
    }

    fn get<T>(&self, key: GhosttyTerminalData) -> T {
        let mut out = MaybeUninit::<T>::zeroed();
        check(unsafe { ghostty_terminal_get(self.term, key, out.as_mut_ptr() as *mut c_void) }, "ghostty_terminal_get");
        unsafe { out.assume_init() }
    }

    pub(crate) fn mode(&self, mode: GhosttyMode) -> bool {
        let mut q = GhosttyTerminalModeConfig { mode, value: false };
        check(unsafe { ghostty_terminal_get(self.term, GHOSTTY_TERMINAL_DATA_MODE, &mut q as *mut _ as *mut c_void) }, "mode");
        q.value
    }

    pub fn title(&self) -> String {
        let t: GhosttyString = self.get(GHOSTTY_TERMINAL_DATA_TITLE);
        if t.len == 0 {
            return String::new();
        }
        String::from_utf8_lossy(unsafe { std::slice::from_raw_parts(t.ptr, t.len) }).into_owned()
    }

    fn render_get<T>(&self, key: GhosttyRenderStateData) -> T {
        let mut out = MaybeUninit::<T>::zeroed();
        check(unsafe { ghostty_render_state_get(self.render, key, out.as_mut_ptr() as *mut c_void) }, "ghostty_render_state_get");
        unsafe { out.assume_init() }
    }

    /// Brings `frame` up to date and clears the dirty state. Returns true if anything changed.
    pub fn update_frame(&mut self, frame: &mut Frame) -> bool {
        check(unsafe { ghostty_render_state_update(self.render, self.term) }, "ghostty_render_state_update");
        let dirty: GhosttyRenderStateDirty = self.render_get(GHOSTTY_RENDER_STATE_DATA_DIRTY);
        let cols: u16 = self.render_get(GHOSTTY_RENDER_STATE_DATA_COLS);
        let rows: u16 = self.render_get(GHOSTTY_RENDER_STATE_DATA_ROWS);

        let mut colors: GhosttyRenderStateColors = unsafe { zeroed() };
        colors.size = size_of::<GhosttyRenderStateColors>();
        check(unsafe { ghostty_render_state_get(self.render, GHOSTTY_RENDER_STATE_DATA_COLORS, &mut colors as *mut _ as *mut c_void) }, "colors");
        let (background, foreground) = (Rgb::from(colors.background), Rgb::from(colors.foreground));

        let mut c: GhosttyRenderStateCursor = unsafe { zeroed() };
        c.size = size_of::<GhosttyRenderStateCursor>();
        check(unsafe { ghostty_render_state_get(self.render, GHOSTTY_RENDER_STATE_DATA_CURSOR, &mut c as *mut _ as *mut c_void) }, "cursor");
        let cursor = Cursor {
            pos: c.viewport_has_value.then_some((c.viewport_x, c.viewport_y)),
            visible: c.visible,
            blinking: c.blinking,
            style: match c.visual_style {
                GHOSTTY_RENDER_STATE_CURSOR_VISUAL_STYLE_BAR => CursorStyle::Bar,
                GHOSTTY_RENDER_STATE_CURSOR_VISUAL_STYLE_UNDERLINE => CursorStyle::Underline,
                GHOSTTY_RENDER_STATE_CURSOR_VISUAL_STYLE_BLOCK_HOLLOW => CursorStyle::BlockHollow,
                _ => CursorStyle::Block,
            },
            color: colors.cursor_has_value.then(|| colors.cursor.into()),
        };

        let resized = frame.cols != cols || frame.rows.len() != rows as usize;
        let colors_changed = frame.background != background || frame.foreground != foreground;
        let theme_changed = frame.theme_generation != self.theme_generation;
        let full = dirty == GHOSTTY_RENDER_STATE_DIRTY_FULL || resized || colors_changed || theme_changed;
        let cursor_changed = frame.cursor != cursor;
        frame.cols = cols;
        frame.rows.resize_with(rows as usize, Row::default);
        frame.background = background;
        frame.foreground = foreground;
        frame.selection = self.selection;
        frame.theme_generation = self.theme_generation;
        frame.cursor = cursor;
        frame.full_redraw = full;
        for row in &mut frame.rows {
            row.dirty = false;
        }
        if dirty == GHOSTTY_RENDER_STATE_DIRTY_FALSE && !full && self.force_dirty.is_empty() {
            return cursor_changed;
        }

        check(unsafe { ghostty_render_state_get(self.render, GHOSTTY_RENDER_STATE_DATA_ROW_ITERATOR, &mut self.row_iter as *mut _ as *mut c_void) }, "row iterator");
        let mut y = 0usize;
        while unsafe { ghostty_render_state_row_iterator_next(self.row_iter) } && y < rows as usize {
            let mut row_dirty = false;
            check(unsafe { ghostty_render_state_row_get(self.row_iter, GHOSTTY_RENDER_STATE_ROW_DATA_DIRTY, &mut row_dirty as *mut _ as *mut c_void) }, "row dirty");
            if row_dirty || full || self.force_dirty.contains(&y) {
                self.read_row(&mut frame.rows[y], cols as usize, foreground, background);
                frame.rows[y].dirty = true;
                let clean = false;
                check(unsafe { ghostty_render_state_row_set(self.row_iter, GHOSTTY_RENDER_STATE_ROW_OPTION_DIRTY, &clean as *const _ as *const c_void) }, "row clean");
            }
            y += 1;
        }
        self.force_dirty.clear();
        let clean: GhosttyRenderStateDirty = GHOSTTY_RENDER_STATE_DIRTY_FALSE;
        check(unsafe { ghostty_render_state_set(self.render, GHOSTTY_RENDER_STATE_OPTION_DIRTY, &clean as *const _ as *const c_void) }, "state clean");
        true
    }

    /// Forces a row to be re-read on the next `update_frame`, for callers that drew over it.
    pub fn mark_row_dirty(&mut self, y: usize) {
        self.force_dirty.push(y);
    }

    fn read_row(&mut self, row: &mut Row, cols: usize, foreground: Rgb, background: Rgb) {
        row.cells.resize_with(cols, Cell::default);
        check(unsafe { ghostty_render_state_row_get(self.row_iter, GHOSTTY_RENDER_STATE_ROW_DATA_CELLS, &mut self.row_cells as *mut _ as *mut c_void) }, "row cells");
        let cells = self.row_cells;
        let cell_get = |key, out: *mut c_void| unsafe { ghostty_render_state_row_cells_get(cells, key, out) };
        let mut utf8 = [0u8; 256];
        for cell in row.cells.iter_mut() {
            if !unsafe { ghostty_render_state_row_cells_next(cells) } {
                *cell = Cell { width: 1, fg: foreground, ..Cell::default() };
                continue;
            }
            let mut raw: GhosttyCell = 0;
            check(cell_get(GHOSTTY_RENDER_STATE_ROW_CELLS_DATA_RAW, &mut raw as *mut _ as *mut c_void), "raw cell");
            let mut wide: GhosttyCellWide = 0;
            check(unsafe { ghostty_cell_get(raw, GHOSTTY_CELL_DATA_WIDE, &mut wide as *mut _ as *mut c_void) }, "cell wide");
            cell.width = match wide {
                GHOSTTY_CELL_WIDE_WIDE => 2,
                GHOSTTY_CELL_WIDE_SPACER_TAIL | GHOSTTY_CELL_WIDE_SPACER_HEAD => 0,
                _ => 1,
            };

            let mut buf = GhosttyBuffer { ptr: utf8.as_mut_ptr(), cap: utf8.len(), len: 0 };
            let r = cell_get(GHOSTTY_RENDER_STATE_ROW_CELLS_DATA_GRAPHEMES_UTF8, &mut buf as *mut _ as *mut c_void);
            cell.text.clear();
            if r == GHOSTTY_SUCCESS {
                cell.text.push_str(&String::from_utf8_lossy(&utf8[..buf.len]));
            } else {
                // Longer than any real cluster: a hostile stack of combining marks.
                assert_eq!(r, GHOSTTY_OUT_OF_SPACE, "graphemes");
                cell.text.push('\u{fffd}');
            }

            cell.fg = foreground;
            cell.bg = None;
            cell.underline_color = None;
            cell.attrs = 0;
            let mut styled = false;
            check(cell_get(GHOSTTY_RENDER_STATE_ROW_CELLS_DATA_HAS_STYLING, &mut styled as *mut _ as *mut c_void), "has styling");
            let mut bg: GhosttyColorRgb = unsafe { zeroed() };
            // Background-only cells carry their colour in the cell, without a style.
            if cell_get(GHOSTTY_RENDER_STATE_ROW_CELLS_DATA_BG_COLOR, &mut bg as *mut _ as *mut c_void) == GHOSTTY_SUCCESS {
                cell.bg = Some(bg.into());
            }
            if styled {
                let mut fg: GhosttyColorRgb = unsafe { zeroed() };
                if cell_get(GHOSTTY_RENDER_STATE_ROW_CELLS_DATA_FG_COLOR, &mut fg as *mut _ as *mut c_void) == GHOSTTY_SUCCESS {
                    cell.fg = fg.into();
                }
                let mut style: GhosttyStyle = unsafe { zeroed() };
                style.size = size_of::<GhosttyStyle>();
                check(cell_get(GHOSTTY_RENDER_STATE_ROW_CELLS_DATA_STYLE, &mut style as *mut _ as *mut c_void), "style");
                apply_style(cell, &style, background);
            }
            let mut selected = false;
            check(cell_get(GHOSTTY_RENDER_STATE_ROW_CELLS_DATA_SELECTED, &mut selected as *mut _ as *mut c_void), "selected");
            if selected {
                cell.attrs |= attrs::SELECTED;
            }
        }
    }
}

fn apply_style(cell: &mut Cell, s: &GhosttyStyle, background: Rgb) {
    for (on, a) in [
        (s.bold, attrs::BOLD),
        (s.italic, attrs::ITALIC),
        (s.faint, attrs::FAINT),
        (s.strikethrough, attrs::STRIKE),
        (s.overline, attrs::OVERLINE),
        (s.blink, attrs::BLINK),
    ] {
        if on {
            cell.attrs |= a;
        }
    }
    cell.attrs |= match s.underline as GhosttySgrUnderline {
        GHOSTTY_SGR_UNDERLINE_SINGLE => attrs::UNDERLINE,
        GHOSTTY_SGR_UNDERLINE_DOUBLE => attrs::UNDERLINE_DOUBLE,
        GHOSTTY_SGR_UNDERLINE_CURLY => attrs::UNDERLINE_CURLY,
        GHOSTTY_SGR_UNDERLINE_DOTTED => attrs::UNDERLINE_DOTTED,
        GHOSTTY_SGR_UNDERLINE_DASHED => attrs::UNDERLINE_DASHED,
        _ => 0,
    };
    if s.underline_color.tag == GHOSTTY_STYLE_COLOR_RGB {
        let c = unsafe { s.underline_color.value.rgb };
        cell.underline_color = Some(c.into());
    }
    if s.inverse {
        let fg = cell.fg;
        cell.fg = cell.bg.unwrap_or(background);
        cell.bg = Some(fg);
    }
    if s.invisible {
        cell.text.clear();
    }
}

impl Drop for Terminal {
    fn drop(&mut self) {
        unsafe {
            ghostty_render_state_row_cells_free(self.row_cells);
            ghostty_render_state_row_iterator_free(self.row_iter);
            ghostty_render_state_free(self.render);
            ghostty_terminal_free(self.term);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame_of(bytes: &[u8]) -> Frame {
        let mut t = Terminal::new(40, 5, 100);
        t.write(bytes);
        let mut f = Frame::default();
        t.update_frame(&mut f);
        f
    }

    fn line(f: &Frame, y: usize) -> String {
        f.rows[y].cells.iter().map(|c| if c.text.is_empty() && c.width != 0 { " " } else { &c.text }).collect::<String>().trim_end().to_string()
    }

    #[test]
    fn text_and_cursor() {
        let f = frame_of(b"hello\r\nworld");
        assert_eq!(line(&f, 0), "hello");
        assert_eq!(line(&f, 1), "world");
        assert_eq!(f.cursor.pos, Some((5, 1)));
        assert_eq!(f.cols, 40);
        assert_eq!(f.rows.len(), 5);
    }

    #[test]
    fn colours_resolve() {
        let f = frame_of(b"\x1b[38;2;1;2;3;48;2;4;5;6mX\x1b[0m\x1b[7mY");
        let x = &f.rows[0].cells[0];
        assert_eq!((x.fg, x.bg), (Rgb { r: 1, g: 2, b: 3 }, Some(Rgb { r: 4, g: 5, b: 6 })));
        let y = &f.rows[0].cells[1];
        assert_eq!((y.fg, y.bg), (f.background, Some(f.foreground)));
    }

    #[test]
    fn graphemes_cluster_by_default() {
        let f = frame_of("👨\u{200d}👩\u{200d}👧\u{200d}👦|❤\u{fe0f}|".as_bytes());
        let c = &f.rows[0].cells;
        assert_eq!(c[0].text, "👨\u{200d}👩\u{200d}👧\u{200d}👦");
        assert_eq!((c[0].width, c[1].width), (2, 0));
        assert_eq!(c[2].text, "|");
        assert_eq!((c[3].text.as_str(), c[3].width), ("❤\u{fe0f}", 2));
    }

    #[test]
    fn replies_are_ordered_and_drained() {
        let mut t = Terminal::new(40, 5, 100);
        t.write(b"\x1b[5n\x1b[6n");
        assert_eq!(t.take_replies(), b"\x1b[0n\x1b[1;1R");
        assert!(t.take_replies().is_empty());
    }

    #[test]
    fn only_changed_rows_are_dirty() {
        let mut t = Terminal::new(40, 5, 100);
        let mut f = Frame::default();
        t.write(b"one\r\ntwo\r\n");
        assert!(t.update_frame(&mut f));
        assert!(!t.update_frame(&mut f), "no change, no redraw");
        t.write(b"\x1b[2;1Hxx");
        assert!(t.update_frame(&mut f));
        assert!(!f.rows[0].dirty && f.rows[1].dirty && !f.full_redraw);
        assert_eq!(line(&f, 0), "one");
        assert_eq!(line(&f, 1), "xxo");
    }

    #[test]
    fn frames_do_not_depend_on_chunking() {
        let bytes = "\x1b[1;31mcafé\x1b[0m 👩\u{200d}👧 ok\r\n\x1b[?1049halt\x1b[?1049lback".as_bytes();
        let whole = frame_of(bytes);
        for split in 1..bytes.len() {
            let mut t = Terminal::new(40, 5, 100);
            t.write(&bytes[..split]);
            t.write(&bytes[split..]);
            let mut f = Frame::default();
            t.update_frame(&mut f);
            assert_eq!(f, whole, "split at {split}");
        }
    }

    fn press<'a>(key: &str, mods: u16, text: Option<&'a str>) -> input::KeyInput<'a> {
        input::KeyInput { action: input::KeyAction::Press, key: input::key_from_code(key).unwrap(), mods, consumed_mods: 0, text, unshifted: 0 }
    }

    #[test]
    fn keys_follow_terminal_modes() {
        use input::mods;
        let mut t = Terminal::new(40, 5, 100);
        assert_eq!(t.encode_key(&press("KeyA", 0, Some("a"))), b"a");
        assert_eq!(t.encode_key(&press("Enter", 0, None)), b"\r");
        assert_eq!(t.encode_key(&press("KeyC", mods::CTRL, None)), b"\x03");
        assert_eq!(t.encode_key(&press("ArrowUp", 0, None)), b"\x1b[A");
        t.write(b"\x1b[?1h"); // application cursor keys
        assert_eq!(t.encode_key(&press("ArrowUp", 0, None)), b"\x1bOA");
        assert_eq!(t.encode_key(&press("KeyE", 0, Some("é"))), "é".as_bytes());
    }

    #[test]
    fn paste_brackets_and_neutralises_the_end_marker() {
        let mut t = Terminal::new(40, 5, 100);
        assert!(t.paste_needs_confirmation("rm -rf x\n"));
        assert!(!t.paste_needs_confirmation("plain"));
        t.write(b"\x1b[?2004h");
        assert!(!t.paste_needs_confirmation("rm -rf x\n"));
        let out = t.encode_paste("hi\x1b[201~there");
        assert!(out.starts_with(b"\x1b[200~") && out.ends_with(b"\x1b[201~"));
        assert_eq!(out.windows(6).filter(|w| w == b"\x1b[201~").count(), 1, "only the real end marker");
    }

    #[test]
    fn mouse_reports_only_when_requested() {
        let mut t = Terminal::new(40, 5, 100);
        let click = input::MouseInput { action: input::MouseAction::Press, button: Some(1), mods: 0, x: 25.0, y: 45.0, any_button_pressed: true };
        assert!(!t.mouse_tracking());
        assert!(t.encode_mouse(&click, (400, 100), (10, 20)).is_empty());
        t.write(b"\x1b[?1000h\x1b[?1006h");
        assert!(t.mouse_tracking());
        assert_eq!(t.encode_mouse(&click, (400, 100), (10, 20)), b"\x1b[<0;3;3M");
    }

    #[test]
    fn focus_reports_only_when_requested() {
        let mut t = Terminal::new(40, 5, 100);
        assert!(t.encode_focus(true).is_empty());
        t.write(b"\x1b[?1004h");
        assert_eq!(t.encode_focus(true), b"\x1b[I");
        assert_eq!(t.encode_focus(false), b"\x1b[O");
    }

    #[test]
    fn selection_text_and_highlight() {
        let mut t = Terminal::new(40, 5, 100);
        t.write(b"hello world\r\nsecond");
        t.select((6, 0), (10, 0));
        assert_eq!(t.selection_text().as_deref(), Some("world"));
        let mut f = Frame::default();
        t.update_frame(&mut f);
        assert_eq!(f.rows[0].cells[6].attrs & attrs::SELECTED, attrs::SELECTED);
        assert_eq!(f.rows[0].cells[5].attrs & attrs::SELECTED, 0);
        t.select_word((2, 1));
        assert_eq!(t.selection_text().as_deref(), Some("second"));
        t.clear_selection();
        assert_eq!(t.selection_text(), None);
    }

    #[test]
    fn scrollback_scrolls_the_viewport() {
        let mut t = Terminal::new(40, 3, 100);
        for i in 0..10 {
            t.write(format!("line{i}\r\n").as_bytes());
        }
        let mut f = Frame::default();
        t.update_frame(&mut f);
        assert_eq!(line(&f, 0), "line8");
        t.scroll_viewport(-2);
        t.update_frame(&mut f);
        assert_eq!(line(&f, 0), "line6");
        t.scroll_to_bottom();
        t.update_frame(&mut f);
        assert_eq!(line(&f, 0), "line8");
    }

    #[test]
    fn theme_sets_default_and_ansi_colours() {
        let theme = Theme::by_name("nujabes").unwrap();
        let mut t = Terminal::new(40, 5, 100);
        t.write(b"plain\x1b[31mred\x1b[91mbright");
        t.set_theme(Some(&theme));
        let mut f = Frame::default();
        t.update_frame(&mut f);
        assert_eq!((f.background, f.foreground), (theme.background, theme.foreground));
        assert_eq!(f.cursor.color, Some(theme.cursor));
        assert_eq!(f.selection, Some(theme.selection));
        let c = &f.rows[0].cells;
        assert_eq!((c[0].fg, c[5].fg, c[8].fg), (theme.foreground, theme.ansi[1], theme.ansi[9]));
    }

    #[test]
    fn theme_keeps_the_rest_of_the_palette() {
        let mut plain = Terminal::new(40, 5, 100);
        let mut themed = Terminal::new(40, 5, 100);
        themed.set_theme(Some(&Theme::by_name("graduation").unwrap()));
        let bytes = b"\x1b[38;5;16ma\x1b[38;5;123mb\x1b[38;5;255mc";
        plain.write(bytes);
        themed.write(bytes);
        let (mut a, mut b) = (Frame::default(), Frame::default());
        plain.update_frame(&mut a);
        themed.update_frame(&mut b);
        for x in 0..3 {
            assert_eq!(a.rows[0].cells[x].fg, b.rows[0].cells[x].fg, "column {x}");
        }
    }

    #[test]
    fn palette_only_theme_change_redraws_everything() {
        let base = Theme::by_name("graduation").unwrap();
        let mut other = base;
        other.ansi[1] = Rgb { r: 1, g: 2, b: 3 };
        let mut t = Terminal::new(40, 5, 100);
        t.set_theme(Some(&base));
        t.write(b"\x1b[31mred");
        let mut f = Frame::default();
        t.update_frame(&mut f);
        assert!(!t.update_frame(&mut f), "no change, no redraw");
        t.set_theme(Some(&other));
        assert!(t.update_frame(&mut f));
        assert!(f.full_redraw);
        assert_eq!(f.rows[0].cells[0].fg, other.ansi[1]);
        assert!(!t.update_frame(&mut f), "the change is consumed once");
    }

    #[test]
    fn osc_palette_override_survives_a_theme_change() {
        let theme = Theme::by_name("nujabes").unwrap();
        let mut t = Terminal::new(40, 5, 100);
        t.write(b"\x1b]4;1;rgb:01/02/03\x07\x1b[31mr\x1b[32mg");
        t.set_theme(Some(&theme));
        let mut f = Frame::default();
        t.update_frame(&mut f);
        assert_eq!(f.rows[0].cells[0].fg, Rgb { r: 1, g: 2, b: 3 });
        assert_eq!(f.rows[0].cells[1].fg, theme.ansi[2]);
    }

    #[test]
    fn no_theme_restores_the_defaults() {
        let before = frame_of(b"\x1b[31mred");
        let mut t = Terminal::new(40, 5, 100);
        t.write(b"\x1b[31mred");
        t.set_theme(Some(&Theme::by_name("nujabes").unwrap()));
        t.set_theme(None);
        let mut f = Frame::default();
        t.update_frame(&mut f);
        assert_eq!((f.background, f.foreground, f.selection), (before.background, before.foreground, None));
        assert_eq!(f.rows[0].cells[0].fg, before.rows[0].cells[0].fg);
    }

    #[test]
    fn resize_keeps_content() {
        let mut t = Terminal::new(40, 5, 100);
        let mut f = Frame::default();
        t.write(b"keep me");
        t.update_frame(&mut f);
        t.resize(20, 3, 8, 16);
        assert!(t.update_frame(&mut f));
        assert!(f.full_redraw);
        assert_eq!((f.cols, f.rows.len()), (20, 3));
        assert_eq!(line(&f, 0), "keep me");
    }
}
