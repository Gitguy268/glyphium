//! Seeded fuzzing of the whole core: hostile byte streams, resizes and user actions. A failure
//! prints its seed, and the same seed gives the same stream. `FUZZ_SEEDS=5000` runs for longer.

use glyphium_terminal_core::{Frame, Terminal};

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }

    fn pick<'a>(&mut self, items: &[&'a str]) -> &'a str {
        items[self.below(items.len() as u64) as usize]
    }
}

fn seeds() -> u64 {
    std::env::var("FUZZ_SEEDS").ok().and_then(|s| s.parse().ok()).unwrap_or(100)
}

/// Non-zero for xorshift, and different streams for neighbouring seeds.
fn rng(seed: u64) -> Rng {
    Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
}

/// One piece of terminal output, weighted towards what real parsers get wrong.
fn piece(r: &mut Rng, out: &mut Vec<u8>) {
    const PARAMS: &[&str] = &["", "0", "1", "2", "3", "7", "24", "80", "999", "65535", "4294967295", "99999999999999999999"];
    match r.below(13) {
        0 => {
            for _ in 0..r.below(64) {
                out.push(r.next() as u8);
            }
        }
        1 => {
            for _ in 0..r.below(200) {
                out.push(b' ' + r.below(95) as u8);
            }
        }
        2 => {
            for _ in 0..r.below(12) {
                let s = r.pick(&["é", "日本語", "🙂", "👨‍👩‍👧", "e\u{301}", "\u{fe0f}", "\u{200d}", "🇳🇱", "\u{ff}", "a"]);
                out.extend_from_slice(s.as_bytes());
            }
        }
        3 => {
            out.extend_from_slice(b"\x1b[");
            out.extend_from_slice(r.pick(&["", "", "?", ">", "<", "="]).as_bytes());
            for i in 0..r.below(7) {
                if i > 0 {
                    out.push(if r.below(8) == 0 { b':' } else { b';' });
                }
                out.extend_from_slice(r.pick(PARAMS).as_bytes());
            }
            let finals = b"ABCDEFGHIJKLMPSTXZ@`abcdefghilmnpqrstuxy";
            out.push(finals[r.below(finals.len() as u64) as usize]);
        }
        4 => {
            let colour = format!("\x1b[{};2;{};{};{}m", 38 + r.below(2) * 10, r.below(300), r.below(300), r.below(300));
            out.extend_from_slice(colour.as_bytes());
            out.extend_from_slice(r.pick(&["\x1b[1m", "\x1b[4:3m", "\x1b[7m", "\x1b[9m", "\x1b[0m", "\x1b[58;5;3m", "\x1b[38;5;999m"]).as_bytes());
        }
        5 => {
            let mode = r.pick(&["1049", "1047", "47", "25", "1000", "1002", "1003", "1006", "1016", "2004", "2026", "2027", "7", "6", "69", "12", "1004", "3"]);
            out.extend_from_slice(format!("\x1b[?{mode}{}", if r.below(2) == 0 { 'h' } else { 'l' }).as_bytes());
        }
        6 => {
            out.extend_from_slice(r.pick(&["\x1b]0;title-", "\x1b]2;", "\x1b]8;;https://example.com/", "\x1b]8;;", "\x1b]52;c;aGVsbG8=", "\x1b]52;c;?", "\x1b]4;1;rgb:ff/00/00", "\x1b]10;?", "\x1b]11;#123456", "\x1b]133;A", "\x1b]7;file:///tmp", "\x1b]1337;x"]).as_bytes());
            for _ in 0..r.below(40) {
                out.push(b'a' + r.below(26) as u8);
            }
            out.extend_from_slice(r.pick(&["\x07", "\x1b\\", "\x07", "\x1b\\", ""]).as_bytes());
        }
        7 => {
            out.extend_from_slice(r.pick(&["\x1bP$qm", "\x1bP+q544e", "\x1bPq#0;2;0;0;0", "\x1b_Ga=q,i=1;AAAA", "\x1b_Ga=T,f=24,s=1,v=1;AAAA", "\x1b^pm", "\x1bX"]).as_bytes());
            out.extend_from_slice(r.pick(&["\x1b\\", "\x1b\\", "\x07", ""]).as_bytes());
        }
        8 => {
            for _ in 0..r.below(16) {
                out.push(*[b'\n', b'\r', b'\t', 8, 0x0e, 0x0f, 0x7f, 0, 0x9b, 0x85, 0x18, 0x1a].get(r.below(12) as usize).unwrap());
            }
        }
        9 => out.extend_from_slice(r.pick(&["\x1bc", "\x1b7", "\x1b8", "\x1bD", "\x1bE", "\x1bM", "\x1bH", "\x1b#8", "\x1b(0", "\x1b(B", "\x1b=", "\x1b>", "\x1b)", "\x1b"]).as_bytes()),
        10 => {
            let (a, b) = (r.below(40), r.below(40));
            out.extend_from_slice(format!("\x1b[{a};{b}r\x1b[?69h\x1b[{a};{b}s").as_bytes());
        }
        11 => out.extend_from_slice(r.pick(&["\x1b[6n", "\x1b[?6n", "\x1b[c", "\x1b[>c", "\x1b[=c", "\x1b[5n", "\x1b[?u", "\x1b[?2027$p", "\x1b[18t", "\x1b[14t", "\x1b[>q"]).as_bytes()),
        _ => out.extend_from_slice(b"\r\nplain text line\r\n"),
    }
}

/// The parts of a frame a user can see.
fn visible(frame: &Frame) -> Vec<Vec<String>> {
    frame.rows.iter().map(|row| row.cells.iter().map(|c| format!("{:?}|{}|{:?}|{:?}|{}", c.text, c.width, c.fg, c.bg, c.attrs)).collect()).collect()
}

fn check_shape(frame: &Frame, cols: u16, rows: u16, seed: u64) {
    assert_eq!(frame.rows.len(), rows as usize, "seed {seed}: row count");
    for (y, row) in frame.rows.iter().enumerate() {
        assert_eq!(row.cells.len(), cols as usize, "seed {seed}: cells in row {y}");
    }
    if let Some((x, y)) = frame.cursor.pos {
        assert!(x < cols && y < rows, "seed {seed}: cursor ({x},{y}) outside {cols}x{rows}");
    }
}

/// Parsing, resizing and every user action, in any order, keep the frame well formed.
#[test]
fn hostile_streams_keep_state_well_formed() {
    for seed in 0..seeds() {
        let mut r = rng(seed);
        let (mut cols, mut rows) = (80u16, 24u16);
        let mut term = Terminal::new(cols, rows, 200);
        let mut frame = Frame::default();
        for _ in 0..200 {
            let mut bytes = vec![];
            piece(&mut r, &mut bytes);
            term.write(&bytes);
            let replies = term.take_replies();
            assert!(replies.len() < 64 * 1024, "seed {seed}: {} reply bytes", replies.len());
            match r.below(24) {
                0 => {
                    cols = 1 + r.below(200) as u16;
                    rows = 1 + r.below(60) as u16;
                    term.resize(cols, rows, 8, 16);
                }
                1 => term.scroll_viewport(r.below(400) as isize - 200),
                2 => term.scroll_to_bottom(),
                3 => term.select((r.below(cols as u64) as u16, r.below(rows as u64) as u16), (r.below(cols as u64) as u16, r.below(rows as u64) as u16)),
                4 => term.select_word((r.below(cols as u64) as u16, r.below(rows as u64) as u16)),
                5 => {
                    let _ = term.selection_text();
                    term.clear_selection();
                }
                6 => {
                    let _ = term.hyperlink_at((r.below(cols as u64) as u16, r.below(rows as u64) as u16));
                    let _ = term.title();
                }
                _ => {}
            }
            term.update_frame(&mut frame);
            check_shape(&frame, cols, rows, seed);
        }
    }
}

/// How output is split across writes never shows on screen. Replies are compared too: a program
/// must get the same answers however its queries arrive.
#[test]
fn chunking_never_changes_the_outcome() {
    let mut with_text = 0;
    for seed in 0..seeds() {
        let mut r = rng(seed);
        let mut stream = vec![];
        for _ in 0..200 {
            piece(&mut r, &mut stream);
        }
        let (mut whole, mut split) = (Terminal::new(80, 24, 200), Terminal::new(80, 24, 200));
        whole.write(&stream);
        let mut at = 0;
        while at < stream.len() {
            let n = (1 + r.below(32) as usize).min(stream.len() - at);
            split.write(&stream[at..at + n]);
            at += n;
        }
        assert_eq!(whole.take_replies(), split.take_replies(), "seed {seed}: replies");
        assert_eq!(whole.title(), split.title(), "seed {seed}: title");
        let (mut a, mut b) = (Frame::default(), Frame::default());
        whole.update_frame(&mut a);
        split.update_frame(&mut b);
        assert_eq!(visible(&a), visible(&b), "seed {seed}: screen");
        assert_eq!(a.cursor.pos, b.cursor.pos, "seed {seed}: cursor");
        with_text += a.rows.iter().flat_map(|row| &row.cells).any(|c| !c.text.is_empty() && c.text != " ") as u64;
    }
    // Guards the test itself: a stream swallowed by an unterminated string would prove nothing.
    assert!(with_text * 2 >= seeds(), "only {with_text} of {} screens have text", seeds());
}

/// Dirty tracking never loses a change: a frame kept up to date row by row shows the same cells
/// as one read from scratch. This is what stops stale rows on screen.
#[test]
fn incremental_frames_match_a_full_read() {
    let (mut changed, mut rebuilt, mut total) = (0, 0, 0);
    for seed in 0..seeds() {
        let mut r = rng(seed);
        let (mut cols, mut rows) = (80u16, 24u16);
        let mut term = Terminal::new(cols, rows, 200);
        let mut kept = Frame::default();
        term.write(b"before");
        term.update_frame(&mut kept);
        for step in 0..200 {
            let mut bytes = vec![];
            piece(&mut r, &mut bytes);
            term.write(&bytes);
            term.take_replies();
            if r.below(40) == 0 {
                cols = 1 + r.below(200) as u16;
                rows = 1 + r.below(60) as u16;
                term.resize(cols, rows, 8, 16);
            }
            changed += term.update_frame(&mut kept) as u64;
            rebuilt += kept.rows.iter().filter(|row| row.dirty).count() as u64;
            total += kept.rows.len() as u64;
            let mut fresh = Frame::default();
            term.update_frame(&mut fresh);
            assert_eq!(visible(&kept), visible(&fresh), "seed {seed}, step {step}");
        }
    }
    // Guards the test itself: it means nothing unless frames changed and some rows were reused.
    println!("{changed} changed frames, {rebuilt} of {total} rows rebuilt");
    assert!(changed > 0 && rebuilt > 0 && rebuilt < total);
}

/// Oversized control strings neither hang the parser nor grow state without limit.
#[test]
fn oversized_control_strings_stay_bounded() {
    let mut term = Terminal::new(80, 24, 200);
    let mut frame = Frame::default();
    let huge = vec![b'x'; 16 * 1024 * 1024];
    for intro in [&b"\x1b]0;"[..], b"\x1bP$q", b"\x1b_G", b"\x1b]8;;", b"\x1b]52;c;"] {
        term.write(intro);
        term.write(&huge);
        term.write(b"\x1b\\ok");
        term.update_frame(&mut frame);
        assert!(term.take_replies().len() < 64 * 1024);
    }
    term.write(b"\x1bc\x1b]0;short\x07");
    assert_eq!(term.title(), "short", "the parser still works afterwards");
}
