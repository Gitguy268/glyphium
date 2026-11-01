//! libghostty-vt through its C API (bindings in `glyphium-core-ffi`).

use std::ffi::c_void;
use std::mem::{MaybeUninit, size_of};
use std::ptr;

use glyphium_core_ffi::*;

use crate::{COLS, Cell, Color, Core, ROWS, Row, SCROLLBACK, SNAPSHOT_HISTORY, State, flags};

pub struct Ghostty {
    term: GhosttyTerminal,
    // Boxed so the pointer handed to the write-pty callback stays put.
    replies: Box<Vec<u8>>,
}

unsafe extern "C" fn write_pty(_t: GhosttyTerminal, userdata: *mut c_void, data: *const u8, len: usize) {
    let out = unsafe { &mut *(userdata as *mut Vec<u8>) };
    out.extend_from_slice(unsafe { std::slice::from_raw_parts(data, len) });
}

fn check(r: GhosttyResult, what: &str) {
    assert_eq!(r, GHOSTTY_SUCCESS, "{what} failed: {r}");
}

impl Ghostty {
    pub fn new() -> Self {
        let mut term: GhosttyTerminal = ptr::null_mut();
        check(unsafe { ghostty_terminal_new(ptr::null(), &mut term, COLS, ROWS) }, "ghostty_terminal_new");
        let mut replies = Box::new(Vec::new());
        let lines: usize = SCROLLBACK;
        unsafe {
            check(ghostty_terminal_set(term, GHOSTTY_TERMINAL_OPT_USERDATA, &mut *replies as *mut Vec<u8> as *const c_void), "userdata");
            let cb: GhosttyTerminalWritePtyFn = Some(write_pty);
            check(ghostty_terminal_set(term, GHOSTTY_TERMINAL_OPT_WRITE_PTY, cb.map_or(ptr::null(), |f| f as *const c_void)), "write_pty");
            check(ghostty_terminal_set(term, GHOSTTY_TERMINAL_OPT_SCROLLBACK_MAX_LINES, &lines as *const usize as *const c_void), "scrollback");
            // The default byte limit is reached first (about 565 lines of ascii-flood natively)
            // and its page granularity differs between 64- and 32-bit builds. Remove it so the
            // line limit governs, as it does for the other cores.
            check(ghostty_terminal_set(term, GHOSTTY_TERMINAL_OPT_SCROLLBACK_MAX_BYTES, ptr::null()), "scrollback bytes");
        }
        Self { term, replies }
    }

    fn get<T>(&self, key: GhosttyTerminalData) -> T {
        let mut out = MaybeUninit::<T>::zeroed();
        check(unsafe { ghostty_terminal_get(self.term, key, out.as_mut_ptr() as *mut c_void) }, "ghostty_terminal_get");
        unsafe { out.assume_init() }
    }

    fn grid_ref(&self, x: u16, y: u32) -> GhosttyGridRef {
        let mut r: GhosttyGridRef = unsafe { MaybeUninit::zeroed().assume_init() };
        r.size = size_of::<GhosttyGridRef>();
        let mut pt: GhosttyPoint = unsafe { MaybeUninit::zeroed().assume_init() };
        pt.tag = GHOSTTY_POINT_TAG_SCREEN;
        pt.value.coordinate.x = x;
        pt.value.coordinate.y = y;
        check(unsafe { ghostty_terminal_grid_ref(self.term, pt, &mut r) }, "ghostty_terminal_grid_ref");
        r
    }

    /// `y` counts from the top of the retained scrollback.
    fn row(&self, y: u32) -> Row {
        let first = self.grid_ref(0, y);
        let mut grow: GhosttyRow = unsafe { MaybeUninit::zeroed().assume_init() };
        check(unsafe { ghostty_grid_ref_row(&first, &mut grow) }, "row");
        let mut wrapped = false;
        check(unsafe { ghostty_row_get(grow, GHOSTTY_ROW_DATA_WRAP, &mut wrapped as *mut bool as *mut c_void) }, "row wrap");
        let mut row = Row { cells: vec![], wrapped };
        let mut cps = [0u32; 32];
        let mut uri = [0u8; 2048];
        for x in 0..COLS {
            let r = if x == 0 { first } else { self.grid_ref(x, y) };
            let mut cell: GhosttyCell = unsafe { MaybeUninit::zeroed().assume_init() };
            check(unsafe { ghostty_grid_ref_cell(&r, &mut cell) }, "cell");
            let mut wide: GhosttyCellWide = 0;
            check(unsafe { ghostty_cell_get(cell, GHOSTTY_CELL_DATA_WIDE, &mut wide as *mut _ as *mut c_void) }, "wide");
            if wide == GHOSTTY_CELL_WIDE_SPACER_TAIL || wide == GHOSTTY_CELL_WIDE_SPACER_HEAD {
                continue;
            }
            let mut n = 0usize;
            check(unsafe { ghostty_grid_ref_graphemes(&r, cps.as_mut_ptr(), cps.len(), &mut n) }, "graphemes");
            let text: String = cps[..n].iter().map(|&c| char::from_u32(c).unwrap_or('\u{fffd}')).collect();
            let mut style: GhosttyStyle = unsafe { MaybeUninit::zeroed().assume_init() };
            style.size = size_of::<GhosttyStyle>();
            check(unsafe { ghostty_grid_ref_style(&r, &mut style) }, "style");
            let mut ulen = 0usize;
            check(unsafe { ghostty_grid_ref_hyperlink_uri(&r, uri.as_mut_ptr(), uri.len(), &mut ulen) }, "hyperlink");
            let cell = Cell {
                col: x,
                text,
                width: if wide == GHOSTTY_CELL_WIDE_WIDE { 2 } else { 1 },
                fg: color(&style.fg_color),
                bg: color(&style.bg_color),
                flags: style_flags(&style),
                hyperlink: (ulen > 0).then(|| String::from_utf8_lossy(&uri[..ulen]).into_owned()),
            };
            if !cell.is_blank() {
                row.cells.push(cell);
            }
        }
        row
    }
}

impl Default for Ghostty {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for Ghostty {
    fn drop(&mut self) {
        unsafe { ghostty_terminal_free(self.term) };
    }
}

fn color(c: &GhosttyStyleColor) -> Color {
    unsafe {
        match c.tag {
            GHOSTTY_STYLE_COLOR_PALETTE => Color::Indexed(c.value.palette),
            GHOSTTY_STYLE_COLOR_RGB => Color::Rgb(c.value.rgb.r, c.value.rgb.g, c.value.rgb.b),
            _ => Color::Default,
        }
    }
}

fn style_flags(s: &GhosttyStyle) -> u16 {
    let mut out = 0;
    for (on, f) in [
        (s.bold, flags::BOLD),
        (s.faint, flags::DIM),
        (s.italic, flags::ITALIC),
        (s.underline != 0, flags::UNDERLINE),
        (s.inverse, flags::INVERSE),
        (s.invisible, flags::HIDDEN),
        (s.strikethrough, flags::STRIKE),
        (s.blink, flags::BLINK),
        (s.overline, flags::OVERLINE),
    ] {
        if on {
            out |= f;
        }
    }
    out
}

impl Core for Ghostty {
    fn name(&self) -> &'static str {
        "libghostty-vt"
    }

    fn write(&mut self, bytes: &[u8]) {
        unsafe { ghostty_terminal_vt_write(self.term, bytes.as_ptr(), bytes.len()) };
    }

    fn snapshot(&mut self) -> State {
        let history: usize = self.get(GHOSTTY_TERMINAL_DATA_SCROLLBACK_ROWS);
        let hist_start = history.saturating_sub(SNAPSHOT_HISTORY);
        let hist = (hist_start..history).map(|y| self.row(y as u32)).collect();
        let screen = (0..ROWS as usize).map(|y| self.row((history + y) as u32)).collect();
        let title: GhosttyString = self.get(GHOSTTY_TERMINAL_DATA_TITLE);
        let title = if title.len == 0 {
            String::new()
        } else {
            String::from_utf8_lossy(unsafe { std::slice::from_raw_parts(title.ptr, title.len) }).into_owned()
        };
        let screen_kind: GhosttyTerminalScreen = self.get(GHOSTTY_TERMINAL_DATA_ACTIVE_SCREEN);
        State {
            cols: COLS,
            rows: ROWS,
            cursor: (self.get::<u16>(GHOSTTY_TERMINAL_DATA_CURSOR_X), self.get::<u16>(GHOSTTY_TERMINAL_DATA_CURSOR_Y)),
            cursor_visible: self.get(GHOSTTY_TERMINAL_DATA_CURSOR_VISIBLE),
            alt_screen: screen_kind == GHOSTTY_TERMINAL_SCREEN_ALTERNATE,
            title,
            history_len: history,
            history: hist,
            screen,
            replies: (*self.replies).clone(),
        }
    }
}
