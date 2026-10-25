//! M1 core bake-off: one thin adapter per terminal core, a normalized state snapshot and the
//! chunking strategies used by the chunk-invariance gate.
//!
//! The gate compares each core with itself: a fixture fed in any chunking must leave the
//! same state (cells, styles, wraps, cursor, active screen, title and every reply byte) as
//! the same fixture fed in one write. Cross-core differences are reported, not gated.

use sha2::{Digest, Sha256};

#[cfg(feature = "alacritty")]
pub mod alacritty;
#[cfg(feature = "ghostty")]
pub mod ghostty;
#[cfg(feature = "wezterm")]
pub mod wezterm;
#[cfg(target_family = "wasm")]
pub mod wasm;

/// Matches the xterm.js baseline in `bench/web`.
pub const COLS: u16 = 120;
pub const ROWS: u16 = 40;
pub const SCROLLBACK: usize = 5000;
/// Scrollback lines included in a snapshot. Every core is configured to keep more than
/// this, so the hashed region is the same size for all three.
pub const SNAPSHOT_HISTORY: usize = 1000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Color {
    Default,
    Indexed(u8),
    Rgb(u8, u8, u8),
}

pub mod flags {
    pub const BOLD: u16 = 1;
    pub const DIM: u16 = 1 << 1;
    pub const ITALIC: u16 = 1 << 2;
    pub const UNDERLINE: u16 = 1 << 3;
    pub const INVERSE: u16 = 1 << 4;
    pub const HIDDEN: u16 = 1 << 5;
    pub const STRIKE: u16 = 1 << 6;
    pub const BLINK: u16 = 1 << 7;
    pub const OVERLINE: u16 = 1 << 8;
}

/// One visible cell. Spacer halves of wide characters are not stored; `width` says how many
/// columns the cell covers, and the column index keeps positions exact.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cell {
    pub col: u16,
    pub text: String,
    pub width: u8,
    pub fg: Color,
    pub bg: Color,
    pub flags: u16,
    pub hyperlink: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Row {
    /// Only cells that differ from a blank default cell.
    pub cells: Vec<Cell>,
    pub wrapped: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct State {
    pub cols: u16,
    pub rows: u16,
    pub cursor: (u16, u16),
    pub cursor_visible: bool,
    pub alt_screen: bool,
    pub title: String,
    /// Retained scrollback before trimming to `SNAPSHOT_HISTORY` (reported, not hashed).
    pub history_len: usize,
    /// Last `SNAPSHOT_HISTORY` scrollback rows, oldest first.
    pub history: Vec<Row>,
    pub screen: Vec<Row>,
    /// Every byte the core asked to send back to the application so far.
    pub replies: Vec<u8>,
}

impl Cell {
    pub fn is_blank(&self) -> bool {
        (self.text.is_empty() || self.text == " ")
            && self.width <= 1
            && self.bg == Color::Default
            && self.flags & (flags::INVERSE | flags::UNDERLINE | flags::STRIKE | flags::OVERLINE) == 0
            && self.hyperlink.is_none()
    }
}

fn put_color(h: &mut Sha256, c: Color) {
    match c {
        Color::Default => h.update([0]),
        Color::Indexed(i) => h.update([1, i]),
        Color::Rgb(r, g, b) => h.update([2, r, g, b]),
    }
}

fn put_str(h: &mut Sha256, s: &str) {
    h.update((s.len() as u64).to_le_bytes());
    h.update(s.as_bytes());
}

fn put_row(h: &mut Sha256, row: &Row) {
    h.update([row.wrapped as u8]);
    h.update((row.cells.len() as u64).to_le_bytes());
    for c in &row.cells {
        h.update(c.col.to_le_bytes());
        put_str(h, &c.text);
        h.update([c.width]);
        put_color(h, c.fg);
        put_color(h, c.bg);
        h.update(c.flags.to_le_bytes());
        match &c.hyperlink {
            Some(u) => {
                h.update([1]);
                put_str(h, u);
            }
            None => h.update([0]),
        }
    }
}

impl State {
    /// Full-detail hash: the chunk-invariance gate compares these within one core.
    pub fn hash(&self) -> String {
        let mut h = Sha256::new();
        h.update(self.cols.to_le_bytes());
        h.update(self.rows.to_le_bytes());
        h.update(self.cursor.0.to_le_bytes());
        h.update(self.cursor.1.to_le_bytes());
        h.update([self.cursor_visible as u8, self.alt_screen as u8]);
        put_str(&mut h, &self.title);
        for rows in [&self.history, &self.screen] {
            h.update((rows.len() as u64).to_le_bytes());
            for r in rows {
                put_row(&mut h, r);
            }
        }
        h.update((self.replies.len() as u64).to_le_bytes());
        h.update(&self.replies);
        hex(&h.finalize())
    }

    /// Visible screen as text, one line per row, padded by column: for cross-core diffs.
    pub fn screen_text(&self) -> Vec<String> {
        self.screen.iter().map(row_text).collect()
    }
}

pub fn row_text(row: &Row) -> String {
    let mut s = String::new();
    let mut col = 0u16;
    for c in &row.cells {
        while col < c.col {
            s.push(' ');
            col += 1;
        }
        s.push_str(if c.text.is_empty() { " " } else { &c.text });
        col += c.width.max(1) as u16;
    }
    s.trim_end().to_string()
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// A terminal core behind the bake-off's thin interface.
pub trait Core {
    fn name(&self) -> &'static str;
    fn write(&mut self, bytes: &[u8]);
    fn snapshot(&mut self) -> State;
}

/// Splits `len` bytes into chunk lengths.
#[derive(Clone, Debug)]
pub enum Chunking {
    Whole,
    Fixed(usize),
    /// Seeded random lengths in 1..=max.
    Random { seed: u64, max: usize },
    /// Exactly two pieces, split at this offset (the exhaustive tier).
    SplitAt(usize),
}

impl Chunking {
    pub fn label(&self) -> String {
        match self {
            Chunking::Whole => "whole".into(),
            Chunking::Fixed(n) => format!("fixed-{n}"),
            Chunking::Random { seed, max } => format!("random-{seed}-max{max}"),
            Chunking::SplitAt(k) => format!("split-{k}"),
        }
    }

    /// Chunk boundaries for `len` bytes. `forced` offsets are always boundaries, so
    /// snapshots taken there are comparable across chunkings.
    pub fn boundaries(&self, len: usize, forced: &[usize]) -> Vec<usize> {
        let mut cuts: Vec<usize> = match *self {
            Chunking::Whole => vec![],
            Chunking::Fixed(n) => (n..len).step_by(n).collect(),
            Chunking::Random { seed, max } => {
                let mut rng = seed.max(1);
                let mut v = vec![];
                let mut at = 0;
                loop {
                    rng ^= rng >> 12;
                    rng ^= rng << 25;
                    rng ^= rng >> 27;
                    at += 1 + (rng.wrapping_mul(0x2545_F491_4F6C_DD1D) % max as u64) as usize;
                    if at >= len {
                        break v;
                    }
                    v.push(at);
                }
            }
            Chunking::SplitAt(k) => vec![k],
        };
        cuts.extend(forced.iter().copied().filter(|&f| f > 0 && f < len));
        cuts.push(len);
        cuts.sort_unstable();
        cuts.dedup();
        cuts
    }
}

/// Checkpoints at 25%, 50%, 75% and 100% of the fixture.
pub fn checkpoints(len: usize) -> Vec<usize> {
    vec![len / 4, len / 2, len * 3 / 4, len]
}

/// Feeds `bytes` to `core` with the given chunking and returns the state hash at each
/// checkpoint.
pub fn run_hashes(core: &mut dyn Core, bytes: &[u8], chunking: &Chunking) -> Vec<String> {
    let cps = checkpoints(bytes.len());
    let mut hashes = vec![];
    let mut start = 0;
    for end in chunking.boundaries(bytes.len(), &cps) {
        core.write(&bytes[start..end]);
        start = end;
        if cps.contains(&end) {
            hashes.push(core.snapshot().hash());
        }
    }
    hashes
}

/// Trims scrollback to the snapshot window.
pub fn trim_history(mut history: Vec<Row>) -> (usize, Vec<Row>) {
    let len = history.len();
    if len > SNAPSHOT_HISTORY {
        history.drain(..len - SNAPSHOT_HISTORY);
    }
    (len, history)
}
