//! Deterministic terminal output fixtures for parser, render and end-to-end benchmarks.
//!
//! Every fixture is a pure function of its name and a fixed seed, so the same build always
//! writes the same bytes. `fixtures/MANIFEST.txt` records the SHA-256 of each file; the
//! benchmark harnesses refuse to run against bytes that don't match.

use std::fmt::Write as _;
use std::fs;
use std::path::PathBuf;

/// xorshift64*: tiny, dependency-free and stable across platforms.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
    fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.below(items.len() as u64) as usize]
    }
}

const WORDS: &[&str] = &[
    "terminal", "glyph", "render", "atlas", "cursor", "scroll", "buffer", "shell", "pty",
    "parser", "unicode", "frame", "damage", "resize", "window", "process", "signal", "stdin",
    "error:", "warning:", "ok", "done", "src/main.rs:42:7", "0x7ffee4", "--release", "|", "=>",
];

fn ascii_flood(rng: &mut Rng, lines: usize) -> Vec<u8> {
    let mut out = String::new();
    for i in 0..lines {
        write!(out, "{i:>7} ").unwrap();
        let mut len = 8;
        while len < 78 {
            let w = rng.pick(WORDS);
            if len + w.len() + 1 > 79 {
                break;
            }
            out.push_str(w);
            out.push(' ');
            len += w.len() + 1;
        }
        out.push_str("\r\n");
    }
    out.into_bytes()
}

fn sgr_flood(rng: &mut Rng, lines: usize) -> Vec<u8> {
    let mut out = String::new();
    for _ in 0..lines {
        let mut len = 0;
        while len < 76 {
            match rng.below(6) {
                0 => write!(out, "\x1b[{}m", 30 + rng.below(8)).unwrap(),
                1 => write!(out, "\x1b[38;5;{}m", rng.below(256)).unwrap(),
                2 => write!(out, "\x1b[38;2;{};{};{}m", rng.below(256), rng.below(256), rng.below(256)).unwrap(),
                3 => write!(out, "\x1b[48;2;{};{};{}m", rng.below(256), rng.below(256), rng.below(256)).unwrap(),
                4 => out.push_str(rng.pick(&["\x1b[1m", "\x1b[3m", "\x1b[4m", "\x1b[7m", "\x1b[9m", "\x1b[2m"])),
                _ => out.push_str("\x1b[0m"),
            }
            let w = rng.pick(WORDS);
            out.push_str(w);
            out.push(' ');
            len += w.len() + 1;
        }
        out.push_str("\x1b[0m\r\n");
    }
    out.into_bytes()
}

/// Wide CJK, emoji (incl. ZWJ sequences and VS16), combining marks and complex scripts.
fn unicode_mix(rng: &mut Rng, lines: usize) -> Vec<u8> {
    const PIECES: &[&str] = &[
        "日本語", "中文字符", "한국어", "テスト", "漢字", "😀", "🎉", "👍🏽", "👨‍👩‍👧‍👦", "🏳️‍🌈",
        "❤️", "☺︎", "e\u{301}", "a\u{308}\u{332}", "ñ", "Z\u{335}\u{321}", "नमस्ते", "क्षत्रिय",
        "مرحبا", "שלום", "ไทย", "Ελληνικά", "Кириллица", "─│┌┐└┘├┤", "█▓▒░", "→←↑↓", "ascii",
        "½", "™", "\u{200b}", "ﬁ", "𝓜𝓪𝓽𝓱", "🇳🇱", "🇯🇵",
    ];
    let mut out = String::new();
    for _ in 0..lines {
        let n = 3 + rng.below(10);
        for _ in 0..n {
            out.push_str(rng.pick(PIECES));
            out.push(' ');
        }
        out.push_str("\r\n");
    }
    out.into_bytes()
}

/// Lines far wider than the screen, so soft wraps and reflow matter.
fn long_lines(rng: &mut Rng, lines: usize, width: usize) -> Vec<u8> {
    let mut out = String::new();
    for _ in 0..lines {
        let mut len = 0;
        while len < width {
            let w = rng.pick(WORDS);
            out.push_str(w);
            out.push(' ');
            len += w.len() + 1;
        }
        out.push_str("\r\n");
    }
    out.into_bytes()
}

/// Full-screen app traffic: alternate screen, cursor addressing, erase, scroll regions,
/// insert/delete lines and a status bar, in the style of htop/vim/tmux redraws.
fn tui_redraw(rng: &mut Rng, frames: usize, cols: u64, rows: u64) -> Vec<u8> {
    let mut out = String::from("\x1b[?1049h\x1b[?25l\x1b[H\x1b[2J");
    for f in 0..frames {
        // Header bar
        write!(out, "\x1b[1;1H\x1b[7m frame {f:>6} \x1b[K\x1b[0m").unwrap();
        // Scrolling region in the middle, like a log pane
        write!(out, "\x1b[3;{}r\x1b[{};1H", rows - 2, rows - 2).unwrap();
        for _ in 0..rng.below(4) {
            write!(out, "\r\n\x1b[38;5;{}m{}\x1b[0m\x1b[K", rng.below(256), rng.pick(WORDS)).unwrap();
        }
        out.push_str("\x1b[r");
        // Random cell updates (meters, numbers)
        for _ in 0..20 {
            let r = 3 + rng.below(rows - 5);
            let c = 1 + rng.below(cols - 10);
            write!(out, "\x1b[{r};{c}H\x1b[3{}m{:>5}\x1b[0m", 1 + rng.below(7), rng.below(100_000)).unwrap();
        }
        // Occasional insert/delete line and erase-in-display
        match rng.below(10) {
            0 => write!(out, "\x1b[{};1H\x1b[2L", 3 + rng.below(rows - 6)).unwrap(),
            1 => write!(out, "\x1b[{};1H\x1b[3M", 3 + rng.below(rows - 6)).unwrap(),
            2 => out.push_str("\x1b[5;1H\x1b[J"),
            _ => {}
        }
        // Status line
        write!(out, "\x1b[{rows};1H\x1b[44m F1 Help  F10 Quit  {}% \x1b[K\x1b[0m", rng.below(100)).unwrap();
    }
    out.push_str("\x1b[?25h\x1b[?1049l");
    out.into_bytes()
}

fn main() {
    let dir = PathBuf::from(std::env::args().nth(1).unwrap_or_else(|| "fixtures/generated".into()));
    fs::create_dir_all(&dir).expect("create fixture dir");
    let fixtures: Vec<(&str, Vec<u8>)> = vec![
        ("ascii-flood", ascii_flood(&mut Rng(1), 100_000)), // also fills a 100k-line history
        ("sgr-flood", sgr_flood(&mut Rng(2), 50_000)),
        ("unicode-mix", unicode_mix(&mut Rng(3), 30_000)),
        ("long-lines", long_lines(&mut Rng(4), 2_000, 4_000)),
        ("tui-redraw", tui_redraw(&mut Rng(5), 20_000, 120, 40)),
    ];
    for (name, bytes) in &fixtures {
        let path = dir.join(format!("{name}.bin"));
        fs::write(&path, bytes).expect("write fixture");
        println!("{name:<14} {:>10} bytes  {}", bytes.len(), path.display());
    }
}
