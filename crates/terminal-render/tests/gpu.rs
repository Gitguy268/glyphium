//! Renderer behaviour on a real GPU. Without an adapter each test prints SKIPPED and returns,
//! so CI without a GPU stays green without claiming anything.

use glyphium_terminal_core::{Frame, Rgb, Terminal, Theme};
use glyphium_terminal_render::{FontConfig, OFFSCREEN_FORMAT, PaneRect, Renderer};

fn renderer(size_px: f32) -> Option<Renderer> {
    renderer_with(FontConfig { size_px, ..Default::default() })
}

fn renderer_with(fonts: FontConfig) -> Option<Renderer> {
    let instance = wgpu::Instance::default();
    let Ok(adapter) = pollster::block_on(instance.request_adapter(&Default::default())) else {
        eprintln!("SKIPPED: no GPU adapter");
        return None;
    };
    let (device, queue) = pollster::block_on(adapter.request_device(&Default::default())).expect("device");
    Some(Renderer::new(device, queue, OFFSCREEN_FORMAT, &fonts))
}

struct Pane {
    term: Terminal,
    frame: Frame,
}

impl Pane {
    fn new(cols: u16, rows: u16) -> Self {
        Self { term: Terminal::new(cols, rows, 100), frame: Frame::default() }
    }
    fn sync(&mut self, bytes: &[u8]) -> &Frame {
        self.term.write(bytes);
        self.term.update_frame(&mut self.frame);
        &self.frame
    }
}

#[test]
fn idle_does_no_work() {
    let Some(mut r) = renderer(16.0) else { return };
    let mut view = r.new_view();
    let mut p = Pane::new(40, 10);
    assert!(r.update(&mut view, p.sync(b"hello\r\nworld")));
    let before = r.stats;
    for _ in 0..5 {
        assert!(!r.update(&mut view, p.sync(b"")), "nothing changed");
    }
    assert_eq!(r.stats, before, "no rows rebuilt, no uploads");
}

#[test]
fn only_dirty_rows_rebuild() {
    let Some(mut r) = renderer(16.0) else { return };
    let mut view = r.new_view();
    let mut p = Pane::new(40, 10);
    r.update(&mut view, p.sync(b"one\r\ntwo\r\nthree"));
    let rows = r.stats.rows_rebuilt;
    assert_eq!(rows, 10, "first frame builds everything");
    assert!(r.update(&mut view, p.sync(b"\x1b[2;1HX")));
    // The edit's row, plus the row the cursor left.
    assert!(r.stats.rows_rebuilt - rows <= 2, "rebuilt {} rows", r.stats.rows_rebuilt - rows);
}

#[test]
fn cursor_motion_redraws_without_rebuilding_rows() {
    let Some(mut r) = renderer(16.0) else { return };
    let mut view = r.new_view();
    let mut p = Pane::new(40, 10);
    r.update(&mut view, p.sync(b"text"));
    let mut hidden = p.frame.clone();
    hidden.cursor.visible = false;
    hidden.full_redraw = false;
    hidden.rows.iter_mut().for_each(|row| row.dirty = false);
    let rows = r.stats.rows_rebuilt;
    assert!(r.update(&mut view, &hidden), "a blink is damage");
    assert_eq!(r.stats.rows_rebuilt, rows);
}

const LOREM: std::ops::Range<u32> = 0x21..0x7f;

/// A page that holds about 40 glyphs at this size, so a few lines fill it.
fn tiny_atlas() -> FontConfig {
    FontConfig { size_px: 96.0, atlas_sizes: Some((512, 512)), ..Default::default() }
}

#[test]
fn atlas_eviction_keeps_the_picture() {
    let Some(mut r) = renderer_with(tiny_atlas()) else { return };
    let mut view = r.new_view();
    let mut p = Pane::new(8, 2);
    let chars: Vec<char> = LOREM.filter_map(char::from_u32).collect();
    for line in chars.chunks(8) {
        let line: String = line.iter().collect();
        r.update(&mut view, p.sync(format!("\x1b[H{line}").as_bytes()));
    }
    assert!(r.stats.atlas_evictions > 0, "expected the page to fill");
    assert_eq!(r.stats.atlas_overflows, 0, "every frame fits a page on its own");
    let (w, h) = r.pixel_size(8, 2);
    let after = pollster::block_on(r.render_to_rgba(&view, p.frame.background, (w, h)));

    // A fresh renderer draws the same final screen with no eviction history.
    let mut fresh = renderer_with(tiny_atlas()).unwrap();
    let mut fresh_view = fresh.new_view();
    let mut full = p.frame.clone();
    full.full_redraw = true;
    fresh.update(&mut fresh_view, &full);
    let expected = pollster::block_on(fresh.render_to_rgba(&fresh_view, p.frame.background, (w, h)));
    assert!(after == expected, "pixels differ after eviction");
}

#[test]
fn a_frame_larger_than_the_atlas_does_not_hang() {
    let Some(mut r) = renderer_with(tiny_atlas()) else { return };
    let mut view = r.new_view();
    let mut p = Pane::new(40, 8);
    let text: String = LOREM.filter_map(char::from_u32).chain(LOREM.filter_map(char::from_u32).flat_map(|c| [c, ' '])).collect();
    assert!(r.update(&mut view, p.sync(text.as_bytes())));
    assert!(r.stats.atlas_overflows > 0, "this frame cannot fit");
    // Later frames that fit are whole again.
    let small = b"\x1b[2J\x1b[Hok";
    assert!(r.update(&mut view, p.sync(small)));
}

#[test]
fn invalidate_rebuilds_the_same_pixels() {
    let Some(mut r) = renderer(16.0) else { return };
    let mut view = r.new_view();
    let mut p = Pane::new(40, 10);
    r.update(&mut view, p.sync(b"\x1b[31mred\x1b[0m plain"));
    let size = r.pixel_size(40, 10);
    let a = pollster::block_on(r.render_to_rgba(&view, p.frame.background, size));
    view.invalidate();
    assert!(r.update(&mut view, &p.frame));
    let b = pollster::block_on(r.render_to_rgba(&view, p.frame.background, size));
    assert!(a == b);
}

#[test]
fn panes_draw_at_their_origins_and_are_clipped() {
    let Some(mut r) = renderer(16.0) else { return };
    let (mut left, mut right) = (Pane::new(10, 4), Pane::new(10, 4));
    let (mut lv, mut rv) = (r.new_view(), r.new_view());
    r.update(&mut lv, left.sync("\x1b[41m\x1b[2J".as_bytes()));
    r.update(&mut rv, right.sync("\x1b[42m\x1b[2J".as_bytes()));
    let (w, h) = r.pixel_size(10, 4);
    let total = (w * 2 + 2, h);
    let panes = [(&lv, PaneRect { x: 0, y: 0, width: w, height: h }), (&rv, PaneRect { x: w + 2, y: 0, width: w, height: h })];
    let rgba = pollster::block_on(r.render_panes_to_rgba(glyphium_terminal_core::Rgb { r: 9, g: 9, b: 9 }, total, &panes));
    let px = |x: u32| &rgba[((h / 2 * total.0 + x) * 4) as usize..((h / 2 * total.0 + x) * 4 + 3) as usize];
    assert!(px(w / 2) != px(w + 2 + w / 2), "the two panes have different colours");
    assert_eq!(px(w), [9, 9, 9], "the gap keeps the clear colour");
    assert_eq!(px(w - 1), px(0));
}

#[test]
fn font_size_change_rebuilds() {
    let Some(mut r) = renderer(16.0) else { return };
    let mut view = r.new_view();
    let mut p = Pane::new(10, 2);
    r.update(&mut view, p.sync(b"zoom"));
    let before = r.metrics();
    r.set_font_size(32.0);
    assert!(r.metrics().height > before.height);
    assert!(r.update(&mut view, p.sync(b"")), "stale glyphs force a rebuild");
}

#[test]
fn theme_colours_the_background_and_selection() {
    let Some(mut r) = renderer(16.0) else { return };
    let theme = Theme::by_name("nujabes").unwrap();
    let mut view = r.new_view();
    let mut p = Pane::new(20, 2);
    p.term.set_theme(Some(&theme));
    p.term.write(b"abc");
    p.term.select((0, 0), (2, 0));
    r.update(&mut view, p.sync(b""));
    let size = r.pixel_size(20, 2);
    let rgba = pollster::block_on(r.render_to_rgba(&view, p.frame.background, size));
    let pixel = |x: u32, y: u32| {
        let i = ((y * size.0 + x) * 4) as usize;
        Rgb { r: rgba[i], g: rgba[i + 1], b: rgba[i + 2] }
    };
    let m = r.metrics();
    assert_eq!(pixel(0, 0), theme.selection, "selected cell's corner");
    assert_eq!(pixel(size.0 - 1, 0), theme.background, "unselected cell");
    assert_eq!(pixel(size.0 - 1, m.height), theme.background, "empty row");
}
