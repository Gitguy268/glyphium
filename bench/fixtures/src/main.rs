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

/// A few KB of parser edge cases, small enough to split at every byte offset.
/// Covers multi-byte and invalid UTF-8, CSI parameters and subparameters, OSC with both
/// terminators, DCS, plain ESC sequences, CAN/SUB aborts, C0 inside sequences, and queries
/// whose replies must be emitted exactly once however the input is chunked.
fn edge_cases() -> Vec<u8> {
    let mut out: Vec<u8> = Vec::new();
    let blocks: &[&[u8]] = &[
        // DECALN fills the screen, then a full clear: exercises every cell once.
        b"\x1b#8\x1b[H\x1b[2J",
        // UTF-8: 2-, 3- and 4-byte, ZWJ sequences, VS15/VS16, flags, combining marks.
        "caf\u{e9} \u{65e5}\u{672c} \u{1f600} \u{1f469}\u{200d}\u{1f467} \u{1f468}\u{200d}\u{1f469}\u{200d}\u{1f467}\u{200d}\u{1f466} \u{2764}\u{fe0f} \u{263a}\u{fe0e} \u{1f1f3}\u{1f1f1} e\u{301}\u{302} \u{915}\u{94d}\u{937}\r\n".as_bytes(),
        // Invalid UTF-8: lone continuation, truncated 3-byte lead before ASCII, 0xFF, overlong.
        b"[\x80][\xe2\x82A][\xff][\xc0\xaf][\xf0\x9f\x98]\r\n",
        // SGR: plain, many params, colon subparameters, underline styles and colour.
        b"\x1b[1;31;42mA\x1b[0m \x1b[1;2;3;4;5;7;8;9mB\x1b[0m \x1b[38:2::10:20:30mC\x1b[48;2;200;100;50mD\x1b[0m \x1b[4:3;58:5:196mE\x1b[0m \x1b[38;5;208mF\x1b[m\r\n",
        // CSI with intermediates and private markers: cursor style, autowrap off/on, origin mode.
        b"\x1b[2 q\x1b[?7lno-wrap-no-wrap-no-wrap-no-wrap-no-wrap-no-wrap-no-wrap-no-wrap-no-wrap-no-wrap-no-wrap-no-wrap-no-wrap-no-wrap-no-wrap-X\x1b[?7h\r\n",
        // OSC: titles ended by BEL and by ST, a hyperlink, a palette change.
        b"\x1b]0;edge title\x07\x1b]2;second \xc3\xa9 title\x1b\\\x1b]8;id=x;https://example.com/\xe2\x9c\x93\x1b\\link\x1b]8;;\x1b\\ \x1b]4;1;rgb:ff/80/00\x07\x1b[31mR\x1b[0m\r\n",
        // DCS: DECRQSS (has a reply) and an unknown passthrough string.
        b"\x1bP$qm\x1b\\\x1bPzunknown dcs payload\x1b\\after-dcs\r\n",
        // Plain ESC: save/restore, index, reverse index, next line, charset switch, keypad modes.
        b"\x1b7\x1b[5;5Hsaved\x1b8restored\x1bD\x1bM\x1bE\x1b(0lqqk\r\nx  x\r\nmqqj\x1b(B\x1b=\x1b>\r\n",
        // CAN and SUB abort a sequence; the bytes after them print.
        b"\x1b[31\x18m-after-can \x1b[4\x1am-after-sub \x1b]0;aborted\x18title\r\n",
        // C0 inside CSI is executed in place: the LF moves the cursor, then CUP still applies.
        b"\x1b[1\n;20Hc0-in-csi\x1b[25;1H",
        // Editing: insert/delete/erase characters, tabs and tab stops, backspace.
        b"abcdefghij\x1b[5G\x1b[3@XYZ\x1b[2P\x1b[2X\r\n\tT1\tT2\x1b[3g\x1bH\r\tT3\x08\x08bs\r\n",
        // Scroll region with SU/SD and IL/DL inside it.
        b"\x1b[28;34r\x1b[30;1Hregion-a\r\nregion-b\x1b[2S\x1b[1T\x1b[31;1H\x1b[2L\x1b[1M\x1b[r",
        // Pending wrap at the last column, then a combining mark onto the wrapped character.
        b"\x1b[36;1H\x1b[119Gxy\xcc\x81z\r\n",
        // Queries: DSR status and cursor position, DA1, DA2, DECRQM, XTVERSION.
        b"\x1b[5n\x1b[6n\x1b[c\x1b[>c\x1b[?25$p\x1b[>q",
        // Modes: bracketed paste, SGR mouse, focus events; synchronized output open and closed.
        b"\x1b[?2004h\x1b[?1000h\x1b[?1006h\x1b[?1004h\x1b[?2026hsync-body\x1b[?2026l\r\n",
        // Alternate screen with content, then back to the main screen.
        b"\x1b[?1049h\x1b[Halt-screen-content\x1b[10;10H\x1b[7malt\x1b[0m\x1b[?1049l",
        // Bell, enquiry, and a C1 control encoded as UTF-8 (U+009B).
        b"\x07\x05\xc2\x9b31mc1?\x1b[0m\r\n",
    ];
    // Twice, so the second pass starts from the state the first left behind; then scroll
    // enough lines to push the top into scrollback.
    for _ in 0..2 {
        for b in blocks {
            out.extend_from_slice(b);
        }
    }
    for i in 0..60 {
        out.extend_from_slice(format!("scroll {i:>3} \x1b[3{}m\u{2588}\x1b[0m\r\n", i % 8).as_bytes());
    }
    out
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
        ("edge-cases", edge_cases()),
    ];
    for (name, bytes) in &fixtures {
        let path = dir.join(format!("{name}.bin"));
        fs::write(&path, bytes).expect("write fixture");
        println!("{name:<14} {:>10} bytes  {}", bytes.len(), path.display());
    }
}
