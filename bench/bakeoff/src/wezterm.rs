use std::io;
use std::sync::{Arc, Mutex};

use termwiz::cell::{Blink, Intensity, Underline};
use termwiz::color::ColorAttribute;
use wezterm_term::color::ColorPalette;
use wezterm_term::{CellAttributes, Line, Terminal, TerminalConfiguration, TerminalSize};

use crate::{COLS, Cell, Color, Core, ROWS, Row, SCROLLBACK, SNAPSHOT_HISTORY, State, flags, trim_history};

#[derive(Debug)]
struct Config;

impl TerminalConfiguration for Config {
    fn scrollback_size(&self) -> usize {
        SCROLLBACK
    }
    fn color_palette(&self) -> ColorPalette {
        ColorPalette::default()
    }
}

#[derive(Clone, Default)]
struct Replies(Arc<Mutex<Vec<u8>>>);

impl io::Write for Replies {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

pub struct Wezterm {
    term: Terminal,
    replies: Replies,
}

impl Wezterm {
    pub fn new() -> Self {
        let size = TerminalSize { rows: ROWS as usize, cols: COLS as usize, ..TerminalSize::default() };
        let replies = Replies::default();
        let term = Terminal::new(size, Arc::new(Config), "glyphium-bakeoff", "0", Box::new(replies.clone()));
        Self { term, replies }
    }
}

impl Default for Wezterm {
    fn default() -> Self {
        Self::new()
    }
}

fn color(c: ColorAttribute) -> Color {
    match c {
        ColorAttribute::Default => Color::Default,
        ColorAttribute::PaletteIndex(i) => Color::Indexed(i),
        ColorAttribute::TrueColorWithPaletteFallback(t, _) | ColorAttribute::TrueColorWithDefaultFallback(t) => {
            let (r, g, b, _) = t.to_srgb_u8();
            Color::Rgb(r, g, b)
        }
    }
}

fn cell_flags(a: &CellAttributes) -> u16 {
    let mut out = 0;
    match a.intensity() {
        Intensity::Bold => out |= flags::BOLD,
        Intensity::Half => out |= flags::DIM,
        Intensity::Normal => {}
    }
    for (on, f) in [
        (a.italic(), flags::ITALIC),
        (a.underline() != Underline::None, flags::UNDERLINE),
        (a.reverse(), flags::INVERSE),
        (a.invisible(), flags::HIDDEN),
        (a.strikethrough(), flags::STRIKE),
        (a.blink() != Blink::None, flags::BLINK),
        (a.overline(), flags::OVERLINE),
    ] {
        if on {
            out |= f;
        }
    }
    out
}

fn row(line: &Line) -> Row {
    let mut row = Row { cells: vec![], wrapped: line.last_cell_was_wrapped() };
    for c in line.visible_cells() {
        if c.cell_index() >= COLS as usize {
            break;
        }
        let a = c.attrs();
        let cell = Cell {
            col: c.cell_index() as u16,
            text: c.str().to_string(),
            width: c.width() as u8,
            fg: color(a.foreground()),
            bg: color(a.background()),
            flags: cell_flags(a),
            hyperlink: a.hyperlink().map(|h| h.uri().to_string()),
        };
        if !cell.is_blank() {
            row.cells.push(cell);
        }
    }
    row
}

impl Core for Wezterm {
    fn name(&self) -> &'static str {
        "wezterm-term"
    }

    fn write(&mut self, bytes: &[u8]) {
        self.term.advance_bytes(bytes);
    }

    fn snapshot(&mut self) -> State {
        let screen = self.term.screen();
        let total = screen.scrollback_rows();
        let visible_start = screen.phys_row(0);
        let hist_start = visible_start.saturating_sub(SNAPSHOT_HISTORY);
        let hist: Vec<Row> = screen.lines_in_phys_range(hist_start..visible_start).iter().map(row).collect();
        let (_, hist) = trim_history(hist);
        let rows: Vec<Row> = screen.lines_in_phys_range(visible_start..total).iter().map(row).collect();
        let cursor = self.term.cursor_pos();
        State {
            cols: COLS,
            rows: ROWS,
            cursor: (cursor.x as u16, cursor.y as u16),
            cursor_visible: cursor.visibility == termwiz::surface::CursorVisibility::Visible,
            alt_screen: self.term.is_alt_screen_active(),
            title: self.term.get_title().to_string(),
            history_len: visible_start,
            history: hist,
            screen: rows,
            replies: self.replies.0.lock().unwrap().clone(),
        }
    }
}
