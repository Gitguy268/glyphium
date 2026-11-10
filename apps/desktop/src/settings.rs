//! The Settings panel's state: rows of ◀ value ▶ controls over a draft of the config. Every
//! change previews at once; Done writes the changed keys to the config file, Esc puts the
//! original back. Pure logic, tested without a window.

use crate::config::{Config, CursorShape};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Row {
    Theme,
    Font,
    Size,
    LineHeight,
    Opacity,
    Blur,
    Padding,
    Cursor,
    Blink,
    OptionAsMeta,
    CopyOnSelect,
}

pub const ROWS: [Row; 11] = [Row::Theme, Row::Font, Row::Size, Row::LineHeight, Row::Opacity, Row::Blur, Row::Padding, Row::Cursor, Row::Blink, Row::OptionAsMeta, Row::CopyOnSelect];

impl Row {
    pub fn label(self) -> &'static str {
        match self {
            Row::Theme => "Theme",
            Row::Font => "Font",
            Row::Size => "Font size",
            Row::LineHeight => "Line height",
            Row::Opacity => "Opacity",
            Row::Blur => "Blur behind window",
            Row::Padding => "Padding",
            Row::Cursor => "Cursor",
            Row::Blink => "Cursor blink",
            Row::OptionAsMeta => "Option as Meta",
            Row::CopyOnSelect => "Copy on select",
        }
    }

    /// The config key this row writes.
    fn key(self) -> &'static str {
        match self {
            Row::Theme => "theme",
            Row::Font => "font_family",
            Row::Size => "font_size",
            Row::LineHeight => "line_height",
            Row::Opacity => "opacity",
            Row::Blur => "blur",
            Row::Padding => "padding",
            Row::Cursor => "cursor_style",
            Row::Blink => "cursor_blink",
            Row::OptionAsMeta => "option_as_meta",
            Row::CopyOnSelect => "copy_on_select",
        }
    }
}

pub struct Settings {
    pub row: usize,
    pub draft: Config,
    pub original: Config,
    /// The chosen theme (`auto`, `default`, a name or a path), and the one to go back to.
    pub theme: String,
    pub original_theme: String,
    /// Theme cycle: auto, every theme, default.
    themes: Vec<String>,
    /// Font choices; the empty string is the bundled font.
    fonts: Vec<String>,
    /// The theme's suggested opacity: shown, and where stepping starts, when none is set.
    pub base_opacity: f32,
}

fn cycle<T: PartialEq + Clone>(list: &[T], current: &T, dir: i32) -> T {
    let n = list.len() as i32;
    let i = list.iter().position(|x| x == current).map_or(if dir > 0 { -1 } else { 0 }, |i| i as i32);
    list[(i + dir).rem_euclid(n) as usize].clone()
}

fn round2(v: f32) -> f32 {
    (v * 100.0).round() / 100.0
}

fn on_off(v: bool) -> &'static str {
    if v { "On" } else { "Off" }
}

impl Settings {
    pub fn new(config: &Config, theme: &str, theme_names: &[String], fonts: Vec<String>, base_opacity: f32) -> Self {
        let mut themes = vec!["auto".to_string()];
        themes.extend(theme_names.iter().cloned());
        themes.push("default".into());
        let mut font_list = vec![String::new()];
        font_list.extend(fonts.into_iter().filter(|f| !f.is_empty() && f != glyphium_terminal_render::DEFAULT_FAMILY));
        if let Some(f) = &config.font_family
            && !font_list.contains(f)
        {
            font_list.push(f.clone());
        }
        Self { row: 0, draft: config.clone(), original: config.clone(), theme: theme.to_string(), original_theme: theme.to_string(), themes, fonts: font_list, base_opacity }
    }

    pub fn current(&self) -> Row {
        ROWS[self.row]
    }

    pub fn move_row(&mut self, dir: i32) {
        self.row = (self.row as i32 + dir).rem_euclid(ROWS.len() as i32) as usize;
    }

    /// Steps the selected row's value left (`-1`) or right (`1`).
    pub fn step(&mut self, dir: i32) {
        self.step_row(self.current(), dir);
    }

    pub fn step_row(&mut self, row: Row, dir: i32) {
        let d = &mut self.draft;
        let f = dir as f32;
        match row {
            Row::Theme => self.theme = cycle(&self.themes, &self.theme, dir),
            Row::Font => {
                let now = d.font_family.clone().unwrap_or_default();
                let next = cycle(&self.fonts, &now, dir);
                d.font_family = (!next.is_empty()).then_some(next);
            }
            Row::Size => d.font_pt = (d.font_pt.round() + f).clamp(6.0, 48.0),
            Row::LineHeight => d.line_height = round2((d.line_height + 0.05 * f).clamp(0.9, 2.0)),
            Row::Opacity => d.opacity = Some(round2((d.opacity.unwrap_or(self.base_opacity) + 0.05 * f).clamp(0.3, 1.0))),
            Row::Blur => d.blur = !d.blur,
            Row::Padding => d.padding = (d.padding.round() + 2.0 * f).clamp(0.0, 32.0),
            Row::Cursor => {
                let list = [None, Some(CursorShape::Block), Some(CursorShape::Bar), Some(CursorShape::Underline)];
                d.cursor_style = cycle(&list, &d.cursor_style, dir);
            }
            Row::Blink => d.cursor_blink = cycle(&[None, Some(true), Some(false)], &d.cursor_blink, dir),
            Row::OptionAsMeta => d.option_as_meta = !d.option_as_meta,
            Row::CopyOnSelect => d.copy_on_select = !d.copy_on_select,
        }
    }

    /// The value shown for a row. `theme_name` gives a theme's display name.
    pub fn value(&self, row: Row, theme_name: &dyn Fn(&str) -> String) -> String {
        let d = &self.draft;
        match row {
            Row::Theme => theme_name(&self.theme),
            Row::Font => d.font_family.clone().unwrap_or_else(|| format!("{} (built in)", glyphium_terminal_render::DEFAULT_FAMILY)),
            Row::Size => format!("{} pt", d.font_pt),
            Row::LineHeight => format!("{:.2}", d.line_height),
            Row::Opacity => match d.opacity {
                Some(o) => format!("{}%", (o * 100.0).round()),
                None => format!("Theme ({}%)", (self.base_opacity * 100.0).round()),
            },
            Row::Blur => on_off(d.blur).into(),
            Row::Padding => format!("{} pt", d.padding),
            Row::Cursor => d.cursor_style.map_or("Default".into(), |c| {
                let n = c.name();
                n[..1].to_uppercase() + &n[1..]
            }),
            Row::Blink => d.cursor_blink.map_or("Default".into(), |b| on_off(b).into()),
            Row::OptionAsMeta => on_off(d.option_as_meta).into(),
            Row::CopyOnSelect => on_off(d.copy_on_select).into(),
        }
    }

    /// Config lines for every row that differs from when the panel opened.
    pub fn changes(&self) -> Vec<(&'static str, String)> {
        let (d, o) = (&self.draft, &self.original);
        let mut out = vec![];
        for row in ROWS {
            let value = match row {
                Row::Theme => (self.theme != self.original_theme).then(|| self.theme.clone()),
                Row::Font => (d.font_family != o.font_family).then(|| d.font_family.clone().unwrap_or_default()),
                Row::Size => (d.font_pt != o.font_pt).then(|| d.font_pt.to_string()),
                Row::LineHeight => (d.line_height != o.line_height).then(|| d.line_height.to_string()),
                Row::Opacity => (d.opacity != o.opacity).then(|| d.opacity.map_or(String::new(), |v| v.to_string())),
                Row::Blur => (d.blur != o.blur).then(|| d.blur.to_string()),
                Row::Padding => (d.padding != o.padding).then(|| d.padding.to_string()),
                Row::Cursor => (d.cursor_style != o.cursor_style).then(|| d.cursor_style.map_or("default".into(), |c| c.name().into())),
                Row::Blink => (d.cursor_blink != o.cursor_blink).then(|| d.cursor_blink.map_or("default".into(), |b| b.to_string())),
                Row::OptionAsMeta => (d.option_as_meta != o.option_as_meta).then(|| d.option_as_meta.to_string()),
                Row::CopyOnSelect => (d.copy_on_select != o.copy_on_select).then(|| d.copy_on_select.to_string()),
            };
            if let Some(v) = value {
                out.push((row.key(), v));
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config;

    fn settings() -> Settings {
        let names: Vec<String> = ["glass-dark", "glass-light", "dracula"].iter().map(|s| s.to_string()).collect();
        Settings::new(&Config::default(), "auto", &names, vec!["Menlo".into(), "JetBrains Mono NL".into(), "SF Mono".into()], 0.8)
    }

    #[test]
    fn rows_wrap() {
        let mut s = settings();
        s.move_row(-1);
        assert_eq!(s.current(), Row::CopyOnSelect);
        s.move_row(1);
        assert_eq!(s.current(), Row::Theme);
    }

    #[test]
    fn theme_cycles_both_ways() {
        let mut s = settings();
        s.step(1);
        assert_eq!(s.theme, "glass-dark");
        s.step(-1);
        s.step(-1);
        assert_eq!(s.theme, "default");
        s.step(1);
        assert_eq!(s.theme, "auto");
    }

    #[test]
    fn fonts_skip_the_bundled_duplicate() {
        let mut s = settings();
        let mut seen = vec![];
        for _ in 0..3 {
            s.step_row(Row::Font, 1);
            seen.push(s.draft.font_family.clone());
        }
        assert_eq!(seen, [Some("Menlo".into()), Some("SF Mono".into()), None]);
    }

    #[test]
    fn numbers_clamp() {
        let mut s = settings();
        for _ in 0..100 {
            s.step_row(Row::Size, 1);
            s.step_row(Row::Opacity, -1);
            s.step_row(Row::LineHeight, 1);
            s.step_row(Row::Padding, -1);
        }
        assert_eq!((s.draft.font_pt, s.draft.opacity, s.draft.line_height, s.draft.padding), (48.0, Some(0.3), 2.0, 0.0));
    }

    #[test]
    fn opacity_starts_from_the_theme() {
        let mut s = settings();
        assert_eq!(s.value(Row::Opacity, &|n| n.to_string()), "Theme (80%)");
        s.step_row(Row::Opacity, 1);
        assert_eq!(s.draft.opacity, Some(0.85));
        assert_eq!(s.value(Row::Opacity, &|n| n.to_string()), "85%");
    }

    #[test]
    fn changes_round_trip_through_the_config_file() {
        let mut s = settings();
        s.step_row(Row::Theme, 1);
        s.step_row(Row::Size, 1);
        s.step_row(Row::Cursor, 2);
        s.step_row(Row::Blink, -1);
        s.step_row(Row::Blur, 1);
        s.step_row(Row::Font, 2);
        let changes = s.changes();
        assert_eq!(changes.len(), 6, "{changes:?}");
        let mut text = String::new();
        for (k, v) in &changes {
            text = config::set_key(&text, k, v);
        }
        let (parsed, w) = config::parse(&text, &|n| n == "glass-dark");
        assert!(w.is_empty(), "{w:?}\n{text}");
        assert_eq!(parsed.font_pt, 14.0);
        assert_eq!(parsed.theme, "glass-dark");
        assert_eq!(parsed.cursor_style, Some(CursorShape::Bar));
        assert_eq!(parsed.cursor_blink, Some(false));
        assert!(!parsed.blur);
        assert_eq!(parsed.font_family.as_deref(), Some("SF Mono"));
    }

    #[test]
    fn no_changes_writes_nothing() {
        let mut s = settings();
        s.step_row(Row::Blur, 1);
        s.step_row(Row::Blur, 1);
        assert!(s.changes().is_empty());
    }

    #[test]
    fn going_back_to_defaults_writes_default_values() {
        let c = Config { cursor_style: Some(CursorShape::Bar), cursor_blink: Some(true), ..Default::default() };
        let mut s = Settings::new(&c, "auto", &[], vec![], 1.0);
        s.step_row(Row::Cursor, -2);
        s.step_row(Row::Blink, -1);
        let mut text = String::new();
        for (k, v) in s.changes() {
            text = config::set_key(&text, k, &v);
        }
        let (parsed, w) = config::parse(&text, &|_| false);
        assert!(w.is_empty(), "{w:?}");
        assert_eq!((parsed.cursor_style, parsed.cursor_blink), (None, None));
    }
}
