use std::sync::{Arc, Mutex};
use std::time::Duration;

use alacritty_terminal::event::{Event, EventListener};
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Line};
use alacritty_terminal::term::cell::{Cell as ACell, Flags};
use alacritty_terminal::term::{Config, Term, TermMode};
use alacritty_terminal::vte::ansi::{self, NamedColor, Processor, Timeout};

use crate::{COLS, Cell, Color, Core, ROWS, Row, SCROLLBACK, State, flags, trim_history};

#[derive(Default)]
struct Sink {
    replies: Vec<u8>,
    title: String,
}

#[derive(Clone, Default)]
struct Listener(Arc<Mutex<Sink>>);

impl EventListener for Listener {
    fn send_event(&self, event: Event) {
        let mut sink = self.0.lock().unwrap();
        match event {
            Event::PtyWrite(s) => sink.replies.extend_from_slice(s.as_bytes()),
            Event::Title(t) => sink.title = t,
            Event::ResetTitle => sink.title.clear(),
            _ => {}
        }
    }
}

/// Synchronized updates (mode 2026) end only on the closing sequence or the buffer limit.
/// The default handler reads `Instant::now()`, which panics on wasm32 and would make
/// results depend on wall-clock time.
#[derive(Default)]
pub struct NoTimeout;

impl Timeout for NoTimeout {
    fn set_timeout(&mut self, _: Duration) {}
    fn clear_timeout(&mut self) {}
    fn pending_timeout(&self) -> bool {
        false
    }
}

struct Size;

impl Dimensions for Size {
    fn total_lines(&self) -> usize {
        ROWS as usize
    }
    fn screen_lines(&self) -> usize {
        ROWS as usize
    }
    fn columns(&self) -> usize {
        COLS as usize
    }
}

pub struct Alacritty {
    term: Term<Listener>,
    parser: Processor<NoTimeout>,
    sink: Listener,
}

impl Alacritty {
    pub fn new() -> Self {
        let sink = Listener::default();
        let config = Config { scrolling_history: SCROLLBACK, ..Config::default() };
        Self { term: Term::new(config, &Size, sink.clone()), parser: Processor::new(), sink }
    }

    fn row(&self, line: Line) -> Row {
        let grid = self.term.grid();
        let src = &grid[line];
        let mut row = Row { cells: vec![], wrapped: src[Column(COLS as usize - 1)].flags.contains(Flags::WRAPLINE) };
        for col in 0..COLS as usize {
            let c: &ACell = &src[Column(col)];
            if c.flags.intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER) {
                continue;
            }
            let mut text = String::new();
            text.push(c.c);
            if let Some(zw) = c.zerowidth() {
                text.extend(zw);
            }
            let cell = Cell {
                col: col as u16,
                text,
                width: if c.flags.contains(Flags::WIDE_CHAR) { 2 } else { 1 },
                fg: color(c.fg, NamedColor::Foreground),
                bg: color(c.bg, NamedColor::Background),
                flags: cell_flags(c.flags),
                hyperlink: c.hyperlink().map(|h| h.uri().to_string()),
            };
            if !cell.is_blank() {
                row.cells.push(cell);
            }
        }
        row
    }
}

impl Default for Alacritty {
    fn default() -> Self {
        Self::new()
    }
}

fn color(c: ansi::Color, default: NamedColor) -> Color {
    match c {
        ansi::Color::Named(n) if n == default => Color::Default,
        ansi::Color::Named(n) if (n as usize) < 16 => Color::Indexed(n as u8),
        // Dim/bright variants of the default colours.
        ansi::Color::Named(_) => Color::Default,
        ansi::Color::Indexed(i) => Color::Indexed(i),
        ansi::Color::Spec(rgb) => Color::Rgb(rgb.r, rgb.g, rgb.b),
    }
}

fn cell_flags(f: Flags) -> u16 {
    let mut out = 0;
    for (a, b) in [
        (Flags::BOLD, flags::BOLD),
        (Flags::DIM, flags::DIM),
        (Flags::ITALIC, flags::ITALIC),
        (Flags::ALL_UNDERLINES, flags::UNDERLINE),
        (Flags::INVERSE, flags::INVERSE),
        (Flags::HIDDEN, flags::HIDDEN),
        (Flags::STRIKEOUT, flags::STRIKE),
    ] {
        if f.intersects(a) {
            out |= b;
        }
    }
    out
}

impl Core for Alacritty {
    fn name(&self) -> &'static str {
        "alacritty_terminal"
    }

    fn write(&mut self, bytes: &[u8]) {
        self.parser.advance(&mut self.term, bytes);
    }

    fn snapshot(&mut self) -> State {
        let grid = self.term.grid();
        let history = grid.history_size();
        let start = history.saturating_sub(crate::SNAPSHOT_HISTORY);
        let hist: Vec<Row> = (start..history).map(|i| self.row(Line(i as i32 - history as i32))).collect();
        let (_, hist) = trim_history(hist);
        let screen = (0..ROWS as i32).map(|y| self.row(Line(y))).collect();
        let cursor = grid.cursor.point;
        let sink = self.sink.0.lock().unwrap();
        State {
            cols: COLS,
            rows: ROWS,
            cursor: (cursor.column.0 as u16, cursor.line.0 as u16),
            cursor_visible: self.term.mode().contains(TermMode::SHOW_CURSOR),
            alt_screen: self.term.mode().contains(TermMode::ALT_SCREEN),
            title: sink.title.clone(),
            history_len: history,
            history: hist,
            screen,
            replies: sink.replies.clone(),
        }
    }
}
