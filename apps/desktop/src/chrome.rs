//! The window chrome: a translucent title bar in the window's own colours, with the traffic
//! lights' space on macOS, the title, a `+` and a Queue button; a row of tab pills when there
//! is more than one tab; the queue popover; and toasts. Pure layout: it returns shapes for the
//! renderer and click targets, so it is tested without a window.

use glyphium_terminal_core::Rgb;
use glyphium_terminal_render::{Shape, color, ui_line_height};

/// Heights and gaps in points; multiplied by the window's scale factor.
const TITLE_ROW: f32 = 30.0;
const TAB_ROW: f32 = 30.0;
/// Room for the close, minimise and zoom buttons at the top left on macOS.
const TRAFFIC_LIGHTS: f32 = 78.0;
const UI_PT: f32 = 12.5;
const SMALL_PT: f32 = 11.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Hit {
    Tab(usize),
    CloseTab(usize),
    NewTab,
    Queue,
    /// Inside the queue popover: clicks there do nothing.
    Popover,
    /// Empty title bar: drags the window.
    Empty,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Area {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Area {
    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x && y >= self.y && x < self.x + self.w && y < self.y + self.h
    }

    pub fn center(&self) -> (f32, f32) {
        (self.x + self.w / 2.0, self.y + self.h / 2.0)
    }
}

pub struct TabInfo {
    pub title: String,
    pub queued: usize,
}

pub struct PromptInfo {
    pub text: String,
    pub waiting: Vec<String>,
    /// The queue is off: the pane runs a program that is not a shell.
    pub unsupported: bool,
}

pub struct Input {
    /// Window width in pixels.
    pub width: f32,
    pub scale: f32,
    /// Leave the traffic lights' space (macOS, not full screen).
    pub traffic_lights: bool,
    pub title: String,
    pub tabs: Vec<TabInfo>,
    pub active: usize,
    /// Commands waiting in the focused pane.
    pub queued: usize,
    pub hover: Option<Hit>,
    pub prompt: Option<PromptInfo>,
    pub toast: Option<String>,
    /// The paste question: a toast that stands out until answered.
    pub warning: Option<String>,
    pub background: Rgb,
    pub foreground: Rgb,
    pub accent: Rgb,
    /// Window background opacity: the bar and the gaps between panes are painted with it,
    /// since nothing else covers them.
    pub opacity: f32,
    /// Gaps between split panes, in pixels, to draw as hairlines.
    pub dividers: Vec<Area>,
}

pub struct Chrome {
    pub shapes: Vec<Shape>,
    pub hits: Vec<(Area, Hit)>,
}

impl Chrome {
    pub fn hit(&self, x: f32, y: f32) -> Option<Hit> {
        // Later targets (the popover) sit on top of earlier ones.
        self.hits.iter().rev().find(|(a, _)| a.contains(x, y)).map(|(_, h)| *h)
    }

    pub fn area(&self, hit: Hit) -> Option<Area> {
        self.hits.iter().find(|(_, h)| *h == hit).map(|(a, _)| *a)
    }
}

/// Height of the bar in pixels: one row, or two when tabs show.
pub fn height(tabs: usize, scale: f32) -> u32 {
    let pt = if tabs > 1 { TITLE_ROW + TAB_ROW } else { TITLE_ROW };
    (pt * scale).round() as u32
}

fn mix(a: Rgb, b: Rgb, t: f32) -> Rgb {
    let m = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Rgb { r: m(a.r, b.r), g: m(a.g, b.g), b: m(a.b, b.b) }
}

/// Text shortened with an ellipsis to fit `max` pixels.
fn fit(text: &str, max: f32, size: f32, bold: bool, measure: &mut dyn FnMut(&str, f32, bool) -> f32) -> String {
    if measure(text, size, bold) <= max {
        return text.to_string();
    }
    let chars: Vec<char> = text.chars().collect();
    let (mut lo, mut hi) = (0, chars.len());
    while lo < hi {
        let mid = (lo + hi).div_ceil(2);
        let s: String = chars[..mid].iter().collect::<String>() + "…";
        if measure(&s, size, bold) <= max { lo = mid } else { hi = mid - 1 }
    }
    if lo == 0 { String::new() } else { chars[..lo].iter().collect::<String>() + "…" }
}

/// Lays out the chrome. `measure(text, size_px, bold)` gives the width of UI text in pixels.
pub fn layout(input: &Input, measure: &mut dyn FnMut(&str, f32, bool) -> f32) -> Chrome {
    let s = input.scale;
    let (w, bg, fg) = (input.width, input.background, input.foreground);
    let ui = UI_PT * s;
    let small = SMALL_PT * s;
    let line = |size: f32| ui_line_height(size);
    let ink = |a: f32| color(fg, a);
    let mut shapes = vec![];
    let mut hits = vec![];
    let bar_h = height(input.tabs.len(), s) as f32;
    let row = TITLE_ROW * s;

    // Bar: the window background with a faint tint of the text colour, and a hairline.
    shapes.push(Shape::Rect { x: 0.0, y: 0.0, w, h: bar_h, radius: 0.0, color: color(bg, input.opacity) });
    shapes.push(Shape::Rect { x: 0.0, y: 0.0, w, h: bar_h, radius: 0.0, color: ink(0.035) });
    shapes.push(Shape::Rect { x: 0.0, y: bar_h - s.max(1.0), w, h: s.max(1.0), radius: 0.0, color: ink(0.10) });
    hits.push((Area { x: 0.0, y: 0.0, w, h: bar_h }, Hit::Empty));

    // Right side of the title row: + and Queue.
    let pill_h = 22.0 * s;
    let pill_y = (row - pill_h) / 2.0;
    let pad = 10.0 * s;
    let mut right = w - 10.0 * s;
    let label = "⏎ Queue";
    let badge = (input.queued > 0).then(|| input.queued.to_string());
    let label_w = measure(label, ui, true);
    let badge_w = badge.as_ref().map_or(0.0, |b| measure(b, small, true) + 12.0 * s);
    let queue_w = label_w + badge_w + 2.0 * pad;
    let queue = Area { x: right - queue_w, y: pill_y, w: queue_w, h: pill_h };
    let open = input.prompt.is_some();
    let hovered = |h: Hit| input.hover == Some(h);
    let queue_fill = if open { color(input.accent, 0.85) } else if hovered(Hit::Queue) { ink(0.16) } else { ink(0.09) };
    shapes.push(Shape::Rect { x: queue.x, y: queue.y, w: queue.w, h: queue.h, radius: pill_h / 2.0, color: queue_fill });
    shapes.push(Shape::Ring { x: queue.x, y: queue.y, w: queue.w, h: queue.h, radius: pill_h / 2.0, width: s.max(1.0), color: ink(if open { 0.0 } else { 0.10 }) });
    let text_color = if open { color(on_accent(input.accent), 1.0) } else { ink(0.88) };
    shapes.push(Shape::Text { x: queue.x + pad, y: queue.y + (pill_h - line(ui)) / 2.0, text: label.into(), size_px: ui, bold: true, ui: true, color: text_color });
    if let Some(b) = badge {
        let bw = badge_w - 4.0 * s;
        let bh = 16.0 * s;
        let bx = queue.x + pad + label_w + 6.0 * s;
        let by = queue.y + (pill_h - bh) / 2.0;
        let badge_fill = if open { color(on_accent(input.accent), 0.9) } else { color(input.accent, 1.0) };
        let badge_text = if open { color(input.accent, 1.0) } else { color(on_accent(input.accent), 1.0) };
        shapes.push(Shape::Rect { x: bx, y: by, w: bw, h: bh, radius: bh / 2.0, color: badge_fill });
        shapes.push(Shape::Text { x: bx + 6.0 * s, y: by + (bh - line(small)) / 2.0, text: b, size_px: small, bold: true, ui: true, color: badge_text });
    }
    hits.push((queue, Hit::Queue));
    right = queue.x - 6.0 * s;
    let plus = Area { x: right - pill_h, y: pill_y, w: pill_h, h: pill_h };
    if hovered(Hit::NewTab) {
        shapes.push(Shape::Rect { x: plus.x, y: plus.y, w: plus.w, h: plus.h, radius: pill_h / 2.0, color: ink(0.12) });
    }
    let plus_size = 16.0 * s;
    let pw = measure("+", plus_size, false);
    shapes.push(Shape::Text { x: plus.x + (plus.w - pw) / 2.0, y: plus.y + (pill_h - line(plus_size)) / 2.0, text: "+".into(), size_px: plus_size, bold: false, ui: true, color: ink(0.75) });
    hits.push((plus, Hit::NewTab));
    right = plus.x - 8.0 * s;

    // Title, centred in the window if it fits there, else in the free space.
    let left = if input.traffic_lights { TRAFFIC_LIGHTS * s } else { 10.0 * s };
    let title = fit(&input.title, (right - left).max(0.0), ui, true, measure);
    let tw = measure(&title, ui, true);
    let tx = ((w - tw) / 2.0).clamp(left, (right - tw).max(left));
    shapes.push(Shape::Text { x: tx, y: (row - line(ui)) / 2.0, text: title, size_px: ui, bold: true, ui: true, color: ink(0.78) });

    // Tab pills, sharing the second row equally, as Terminal does.
    if input.tabs.len() > 1 {
        let margin = 8.0 * s;
        let gap = 4.0 * s;
        let n = input.tabs.len() as f32;
        let tab_h = 24.0 * s;
        let y = row + (TAB_ROW * s - tab_h) / 2.0 - 1.0 * s;
        let tab_w = ((w - 2.0 * margin - gap * (n - 1.0)) / n).max(10.0);
        for (i, tab) in input.tabs.iter().enumerate() {
            let x = margin + i as f32 * (tab_w + gap);
            let area = Area { x, y, w: tab_w, h: tab_h };
            let active = i == input.active;
            let hover = hovered(Hit::Tab(i)) || hovered(Hit::CloseTab(i));
            if active {
                shapes.push(Shape::Rect { x, y, w: tab_w, h: tab_h, radius: 7.0 * s, color: ink(0.13) });
                shapes.push(Shape::Ring { x, y, w: tab_w, h: tab_h, radius: 7.0 * s, width: s.max(1.0), color: ink(0.14) });
            } else if hover {
                shapes.push(Shape::Rect { x, y, w: tab_w, h: tab_h, radius: 7.0 * s, color: ink(0.06) });
            }
            hits.push((area, Hit::Tab(i)));
            let close = Area { x: x + 4.0 * s, y: y + (tab_h - 18.0 * s) / 2.0, w: 18.0 * s, h: 18.0 * s };
            if hover {
                if hovered(Hit::CloseTab(i)) {
                    shapes.push(Shape::Rect { x: close.x, y: close.y, w: close.w, h: close.h, radius: 5.0 * s, color: ink(0.12) });
                }
                let xw = measure("✕", small, false);
                shapes.push(Shape::Text { x: close.x + (close.w - xw) / 2.0, y: close.y + (close.h - line(small)) / 2.0, text: "✕".into(), size_px: small, bold: false, ui: true, color: ink(0.7) });
                hits.push((close, Hit::CloseTab(i)));
            }
            let mark = if tab.queued > 0 { format!("  ⏳{}", tab.queued) } else { String::new() };
            let name = if tab.title.is_empty() { format!("Tab {}", i + 1) } else { tab.title.clone() };
            let room = tab_w - 2.0 * 26.0 * s;
            let label = fit(&format!("{name}{mark}"), room.max(0.0), ui, active, measure);
            let lw = measure(&label, ui, active);
            shapes.push(Shape::Text { x: x + (tab_w - lw) / 2.0, y: y + (tab_h - line(ui)) / 2.0, text: label, size_px: ui, bold: active, ui: true, color: ink(if active { 0.92 } else { 0.55 }) });
            if i < 9 {
                let key = format!("⌘{}", i + 1);
                let kw = measure(&key, small, false);
                shapes.push(Shape::Text { x: x + tab_w - kw - 8.0 * s, y: y + (tab_h - line(small)) / 2.0, text: key, size_px: small, bold: false, ui: true, color: ink(if hover || active { 0.35 } else { 0.0 }) });
            }
        }
    }

    for d in &input.dividers {
        shapes.push(Shape::Rect { x: d.x, y: d.y, w: d.w, h: d.h, radius: 0.0, color: color(bg, input.opacity) });
        shapes.push(Shape::Rect { x: d.x, y: d.y, w: d.w, h: d.h, radius: 0.0, color: ink(0.16) });
    }

    // Toasts sit under the bar, centred, over the terminal text.
    let toast = input.warning.as_ref().map(|t| (t, true)).or(input.toast.as_ref().map(|t| (t, false)));
    if let Some((text, warn)) = toast {
        let max = (w - 40.0 * s).max(60.0 * s);
        let text = fit(text, max - 32.0 * s, ui, false, measure);
        let tw = measure(&text, ui, false);
        let (tw_box, th) = (tw + 32.0 * s, 30.0 * s);
        let x = (w - tw_box) / 2.0;
        let y = bar_h + 10.0 * s;
        let panel = mix(bg, fg, 0.10);
        shapes.push(Shape::Rect { x, y: y + 2.0 * s, w: tw_box, h: th, radius: th / 2.0, color: [0.0, 0.0, 0.0, 0.18] });
        shapes.push(Shape::Rect { x, y, w: tw_box, h: th, radius: th / 2.0, color: if warn { [0.96, 0.72, 0.18, 1.0] } else { color(panel, 1.0) } });
        shapes.push(Shape::Ring { x, y, w: tw_box, h: th, radius: th / 2.0, width: s.max(1.0), color: ink(if warn { 0.0 } else { 0.14 }) });
        let tc = if warn { [0.1, 0.08, 0.02, 1.0] } else { ink(0.92) };
        shapes.push(Shape::Text { x: x + 16.0 * s, y: y + (th - line(ui)) / 2.0, text, size_px: ui, bold: warn, ui: true, color: tc });
    }

    if let Some(p) = &input.prompt {
        popover(input, p, queue, &mut shapes, &mut hits, measure);
    }
    Chrome { shapes, hits }
}

/// Black or white, whichever reads better on the accent colour.
fn on_accent(accent: Rgb) -> Rgb {
    if glyphium_terminal_core::theme::luminance(accent) > 0.45 { Rgb { r: 0, g: 0, b: 0 } } else { Rgb { r: 255, g: 255, b: 255 } }
}

/// The queue popover under the Queue button: input line, waiting commands, key hints.
fn popover(input: &Input, p: &PromptInfo, anchor: Area, shapes: &mut Vec<Shape>, hits: &mut Vec<(Area, Hit)>, measure: &mut dyn FnMut(&str, f32, bool) -> f32) {
    let s = input.scale;
    let (bg, fg) = (input.background, input.foreground);
    let ink = |a: f32| color(fg, a);
    let small = SMALL_PT * s;
    let mono = 13.0 * s;
    // Terminal fonts advance about 0.6 em per character; the measure callback is for UI text.
    let mono_w = |t: &str, size: f32| t.chars().count() as f32 * size * 0.6;
    let line = ui_line_height;
    let pad = 14.0 * s;
    let w = (440.0 * s).min(input.width - 16.0 * s);
    let x = (anchor.x + anchor.w - w).max(8.0 * s);
    let y = anchor.y + anchor.h + 8.0 * s;
    let shown = p.waiting.len().min(6);
    let more = p.waiting.len() - shown;
    let row_h = 22.0 * s;
    let field_h = 32.0 * s;
    let mut h = pad + line(small) + 8.0 * s + field_h + 10.0 * s;
    if shown > 0 {
        h += line(small) + 4.0 * s + shown as f32 * row_h + if more > 0 { row_h } else { 0.0 } + 6.0 * s;
    }
    h += line(small) + pad;
    let panel = mix(bg, fg, 0.07);
    shapes.push(Shape::Rect { x: x - 1.0 * s, y: y + 4.0 * s, w: w + 2.0 * s, h, radius: 14.0 * s, color: [0.0, 0.0, 0.0, 0.22] });
    shapes.push(Shape::Rect { x, y, w, h, radius: 14.0 * s, color: color(panel, 1.0) });
    shapes.push(Shape::Ring { x, y, w, h, radius: 14.0 * s, width: s.max(1.0), color: ink(0.14) });
    hits.push((Area { x, y, w, h }, Hit::Popover));

    let mut cy = y + pad;
    let heading = if p.unsupported { "Queue needs a shell in this pane" } else { "Queue next command" };
    shapes.push(Shape::Text { x: x + pad, y: cy, text: heading.into(), size_px: small, bold: true, ui: true, color: ink(0.6) });
    let note = "runs when the shell is back at its prompt";
    let nw = measure(note, small, false);
    if nw + measure(heading, small, true) + 3.0 * pad < w {
        shapes.push(Shape::Text { x: x + w - pad - nw, y: cy, text: note.into(), size_px: small, bold: false, ui: true, color: ink(0.38) });
    }
    cy += line(small) + 8.0 * s;

    // Input field with an accent focus ring; the end of a long command stays in view.
    let fx = x + pad;
    let fw = w - 2.0 * pad;
    shapes.push(Shape::Rect { x: fx, y: cy, w: fw, h: field_h, radius: 8.0 * s, color: color(bg, 0.9) });
    shapes.push(Shape::Ring { x: fx - 2.0 * s, y: cy - 2.0 * s, w: fw + 4.0 * s, h: field_h + 4.0 * s, radius: 10.0 * s, width: 2.5 * s, color: color(input.accent, 0.75) });
    let prompt_mark = "❯ ";
    let mw = mono_w(prompt_mark, mono);
    let room = fw - 20.0 * s - mw - 4.0 * s;
    let mut text = p.text.clone();
    while !text.is_empty() && mono_w(&text, mono) > room {
        text.remove(0);
    }
    let ty = cy + (field_h - line(mono)) / 2.0;
    shapes.push(Shape::Text { x: fx + 10.0 * s, y: ty, text: prompt_mark.into(), size_px: mono, bold: true, ui: false, color: color(input.accent, 1.0) });
    let text_w = mono_w(&text, mono);
    if text.is_empty() {
        shapes.push(Shape::Text { x: fx + 10.0 * s + mw, y: ty, text: "make test".into(), size_px: mono, bold: false, ui: false, color: ink(0.25) });
    } else {
        shapes.push(Shape::Text { x: fx + 10.0 * s + mw, y: ty, text, size_px: mono, bold: false, ui: false, color: ink(0.95) });
    }
    shapes.push(Shape::Rect { x: fx + 10.0 * s + mw + text_w + 1.0 * s, y: cy + 7.0 * s, w: 2.0 * s, h: field_h - 14.0 * s, radius: 1.0 * s, color: color(input.accent, 1.0) });
    cy += field_h + 10.0 * s;

    if shown > 0 {
        shapes.push(Shape::Text { x: x + pad, y: cy, text: format!("Waiting ({})", p.waiting.len()), size_px: small, bold: true, ui: true, color: ink(0.6) });
        cy += line(small) + 4.0 * s;
        for (i, cmd) in p.waiting.iter().take(shown).enumerate() {
            if i % 2 == 0 {
                shapes.push(Shape::Rect { x: x + pad - 6.0 * s, y: cy, w: w - 2.0 * pad + 12.0 * s, h: row_h, radius: 6.0 * s, color: ink(0.04) });
            }
            let n = format!("{}", i + 1);
            shapes.push(Shape::Text { x: x + pad, y: cy + (row_h - line(small)) / 2.0, text: n, size_px: small, bold: true, ui: true, color: ink(0.4) });
            let room = w - 2.0 * pad - 22.0 * s;
            let cmd = fit(cmd, room, small * 1.1, false, &mut |t, sz, _| mono_w(t, sz));
            shapes.push(Shape::Text { x: x + pad + 22.0 * s, y: cy + (row_h - line(small * 1.1)) / 2.0, text: cmd, size_px: small * 1.1, bold: false, ui: false, color: ink(0.85) });
            cy += row_h;
        }
        if more > 0 {
            shapes.push(Shape::Text { x: x + pad + 22.0 * s, y: cy + (row_h - line(small)) / 2.0, text: format!("and {more} more"), size_px: small, bold: false, ui: true, color: ink(0.45) });
            cy += row_h;
        }
        cy += 6.0 * s;
    }
    let hints = "↩ Queue    ⌫ Undo last    ⇧⌘⌫ Clear all    esc Close";
    let hints = fit(hints, w - 2.0 * pad, small, false, measure);
    shapes.push(Shape::Text { x: x + pad, y: cy, text: hints, size_px: small, bold: false, ui: true, color: ink(0.42) });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn measure(t: &str, size: f32, _: bool) -> f32 {
        t.chars().count() as f32 * size * 0.55
    }

    fn input(tabs: usize) -> Input {
        Input {
            width: 1600.0,
            scale: 2.0,
            traffic_lights: true,
            title: "bash".into(),
            tabs: (0..tabs).map(|i| TabInfo { title: format!("t{i}"), queued: 0 }).collect(),
            active: 0,
            queued: 0,
            hover: None,
            prompt: None,
            toast: None,
            warning: None,
            background: Rgb { r: 20, g: 20, b: 20 },
            foreground: Rgb { r: 230, g: 230, b: 230 },
            accent: Rgb { r: 80, g: 140, b: 255 },
            opacity: 0.8,
            dividers: vec![],
        }
    }

    #[test]
    fn one_row_for_one_tab_two_for_more() {
        assert_eq!(height(1, 2.0), 60);
        assert_eq!(height(3, 2.0), 120);
        let c = layout(&input(1), &mut measure);
        assert!(c.hits.iter().all(|(_, h)| !matches!(h, Hit::Tab(_))));
    }

    #[test]
    fn hits_find_buttons_tabs_and_empty_space() {
        let c = layout(&input(3), &mut measure);
        let q = c.area(Hit::Queue).unwrap();
        assert_eq!(c.hit(q.center().0, q.center().1), Some(Hit::Queue));
        assert!(q.x + q.w <= 1600.0 && q.y >= 0.0 && q.y + q.h <= 60.0, "queue button in the title row");
        let plus = c.area(Hit::NewTab).unwrap();
        assert!(plus.x + plus.w <= q.x);
        for i in 0..3 {
            let t = c.area(Hit::Tab(i)).unwrap();
            assert_eq!(c.hit(t.center().0, t.center().1), Some(Hit::Tab(i)));
            assert!(t.y >= 60.0, "tabs in the second row");
        }
        assert_eq!(c.hit(300.0, 20.0), Some(Hit::Empty), "title row away from buttons drags");
        assert_eq!(c.hit(300.0, 500.0), None, "below the bar is the terminal");
    }

    #[test]
    fn close_button_only_on_hover() {
        let mut i = input(2);
        assert!(layout(&i, &mut measure).area(Hit::CloseTab(1)).is_none());
        i.hover = Some(Hit::Tab(1));
        let c = layout(&i, &mut measure);
        let close = c.area(Hit::CloseTab(1)).unwrap();
        assert_eq!(c.hit(close.center().0, close.center().1), Some(Hit::CloseTab(1)));
    }

    #[test]
    fn popover_sits_under_the_queue_button_and_on_top() {
        let mut i = input(1);
        i.queued = 3;
        i.prompt = Some(PromptInfo { text: "echo hi".into(), waiting: (0..9).map(|n| format!("cmd {n}")).collect(), unsupported: false });
        let c = layout(&i, &mut measure);
        let q = c.area(Hit::Queue).unwrap();
        let pop = c.area(Hit::Popover).unwrap();
        assert!(pop.y > q.y + q.h && pop.x + pop.w <= 1600.0 && pop.x >= 0.0);
        assert_eq!(c.hit(pop.center().0, pop.center().1), Some(Hit::Popover));
        let texts: Vec<&str> = c.shapes.iter().filter_map(|s| if let Shape::Text { text, .. } = s { Some(text.as_str()) } else { None }).collect();
        assert!(texts.contains(&"3"), "badge count");
        assert!(texts.contains(&"echo hi"));
        assert!(texts.contains(&"cmd 5") && !texts.contains(&"cmd 6"), "six rows shown");
        assert!(texts.contains(&"and 3 more"));
    }

    #[test]
    fn narrow_windows_keep_everything_inside() {
        let mut i = input(4);
        i.width = 300.0;
        i.title = "a very long title that cannot possibly fit in this narrow window".into();
        i.prompt = Some(PromptInfo { text: "x".repeat(500), waiting: vec![], unsupported: false });
        i.toast = Some("y".repeat(300));
        let c = layout(&i, &mut measure);
        for s in &c.shapes {
            if let Shape::Text { x, text, size_px, .. } = s {
                assert!(*x >= 0.0 && x + measure(text, *size_px, false) <= 300.0 + 1.0, "{text:?} at {x} overflows");
            }
        }
    }

    #[test]
    fn fit_adds_an_ellipsis() {
        assert_eq!(fit("short", 100.0, 10.0, false, &mut measure), "short");
        let f = fit("a long piece of text", 30.0, 10.0, false, &mut measure);
        assert!(f.ends_with('…') && measure(&f, 10.0, false) <= 30.0, "{f}");
    }
}
