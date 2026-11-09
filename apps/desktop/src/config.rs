//! Settings from a small `key = value` file. Unknown keys and bad values give a warning and
//! keep the default, so a broken file never stops the terminal from starting.
//! [`TEMPLATE`] lists every key; [`write_key`] changes one line and keeps the rest of the file.

use std::path::{Path, PathBuf};

use glyphium_terminal_core::Theme;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CursorShape {
    Block,
    Bar,
    Underline,
}

impl CursorShape {
    pub const ALL: [CursorShape; 3] = [CursorShape::Block, CursorShape::Bar, CursorShape::Underline];

    pub fn name(self) -> &'static str {
        match self {
            CursorShape::Block => "block",
            CursorShape::Bar => "bar",
            CursorShape::Underline => "underline",
        }
    }

    fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|c| c.name() == s)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Config {
    pub font_pt: f32,
    /// `None` is the bundled JetBrains Mono NL.
    pub font_family: Option<String>,
    pub line_height: f32,
    /// `default` (Ghostty's colours), `auto`, a theme name or a theme file path.
    pub theme: String,
    /// The themes `auto` picks for a light and a dark system appearance.
    pub theme_light: String,
    pub theme_dark: String,
    /// Colour lines (`background = #...`, `palette = 4=#...`) applied on top of the theme.
    pub colors: Vec<(String, String)>,
    /// Window background opacity; `None` takes the theme's suggestion, else opaque.
    pub opacity: Option<f32>,
    pub blur: bool,
    /// Space around the text in each pane, in points.
    pub padding: f32,
    pub window_columns: u16,
    pub window_rows: u16,
    pub cursor_style: Option<CursorShape>,
    pub cursor_blink: Option<bool>,
    pub option_as_meta: bool,
    pub copy_on_select: bool,
    pub confirm_paste: bool,
    pub scrollback: usize,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            font_pt: 13.0,
            font_family: None,
            line_height: glyphium_terminal_render::DEFAULT_LINE_HEIGHT,
            theme: "auto".into(),
            theme_light: "glass-light".into(),
            theme_dark: "glass-dark".into(),
            colors: vec![],
            opacity: None,
            blur: true,
            padding: 8.0,
            window_columns: 100,
            window_rows: 30,
            cursor_style: None,
            cursor_blink: None,
            option_as_meta: false,
            copy_on_select: false,
            confirm_paste: true,
            scrollback: 10_000,
        }
    }
}

impl Config {
    /// The DECSCUSR sequence for the configured cursor, if any part of it is set.
    pub fn cursor_sequence(&self) -> Option<String> {
        if self.cursor_style.is_none() && self.cursor_blink.is_none() {
            return None;
        }
        let blink = self.cursor_blink.unwrap_or(true);
        let base = match self.cursor_style.unwrap_or(CursorShape::Block) {
            CursorShape::Block => 1,
            CursorShape::Underline => 3,
            CursorShape::Bar => 5,
        };
        Some(format!("\x1b[{} q", if blink { base } else { base + 1 }))
    }
}

/// Written when Glyphium creates the file, so every key is there to discover.
pub const TEMPLATE: &str = r##"# Glyphium settings. Edit and switch back to the window: changes apply at once.
# Every line is optional; a commented line shows the default.

# Text
# font_family = "JetBrains Mono NL"   # any installed monospaced font, e.g. "SF Mono", "Menlo"
# font_size = 13                      # points
# line_height = 1.2                   # times the font size

# Colours: default, auto, a built-in theme, a file in themes/ next to this file, or a path.
# Built-in: glass-dark, glass-light, nujabes, graduation, solarized-dark, solarized-light,
#           dracula, catppuccin-mocha, catppuccin-latte, tokyo-night
# theme = auto                        # follows the system appearance
# theme_light = glass-light           # what auto uses in light mode
# theme_dark = glass-dark             # what auto uses in dark mode
# Override single colours on top of any theme (Ghostty theme keys work too):
# background = #1c1c1f
# foreground = #e8e8ed
# cursor_color = #7aa7ff
# selection_background = #3a4a6b
# palette = 4=#6aa6ff

# Window
# opacity = 0.8                       # 0.3 to 1; unset takes the theme's suggestion
# blur = true                         # blur what is behind a translucent window (macOS)
# padding = 8                         # points around the text
# window_columns = 100
# window_rows = 30

# Cursor
# cursor_style = block                # block, bar or underline
# cursor_blink = true

# Behaviour
# option_as_meta = false              # macOS: Option sends Esc+key instead of special characters
# copy_on_select = false
# confirm_paste = true                # ask before a multi-line paste that may run commands
# scrollback = 10000                  # lines per pane
"##;

/// The file to read: `--config`, then `GLYPHIUM_CONFIG`, then `config.toml` in [`dir`].
pub fn path(flag: Option<PathBuf>) -> Option<PathBuf> {
    flag.or_else(|| std::env::var_os("GLYPHIUM_CONFIG").map(PathBuf::from)).or_else(|| dir().map(|d| d.join("config.toml")))
}

/// Glyphium's settings folder: `GLYPHIUM_CONFIG_DIR`, else `~/.config/glyphium`. Theme files
/// live in its `themes/` folder.
pub fn dir() -> Option<PathBuf> {
    std::env::var_os("GLYPHIUM_CONFIG_DIR").map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| Path::new(&h).join(".config/glyphium")))
}

/// Reads the file; a missing one gives the defaults silently. `theme_exists` says whether a
/// theme name can be used.
pub fn load(path: Option<&Path>, theme_exists: &dyn Fn(&str) -> bool) -> (Config, Vec<String>) {
    let Some(path) = path else { return (Config::default(), vec![]) };
    match std::fs::read_to_string(path) {
        Ok(text) => {
            let (config, warnings) = parse(&text, theme_exists);
            let name = path.file_name().map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into_owned());
            (config, warnings.into_iter().map(|w| format!("{name}: {w}")).collect())
        }
        Err(_) => (Config::default(), vec![]),
    }
}

fn parse_bool(v: &str) -> Option<bool> {
    match v {
        "true" | "yes" | "on" => Some(true),
        "false" | "no" | "off" => Some(false),
        _ => None,
    }
}

/// Parses the text, returning the settings and a warning per line that was ignored.
pub fn parse(text: &str, theme_exists: &dyn Fn(&str) -> bool) -> (Config, Vec<String>) {
    let mut config = Config::default();
    let mut warnings = vec![];
    for (n, raw) in text.lines().enumerate() {
        let line = strip_comment(raw).trim();
        if line.is_empty() || line.starts_with('[') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            warnings.push(format!("line {}: expected key = value", n + 1));
            continue;
        };
        let (key, value) = (key.trim(), value.trim().trim_matches('"').trim_matches('\''));
        let theme_ok = |v: &str| v == "default" || v == "auto" || theme_exists(v);
        let ok = match key {
            "font_size" => value.parse::<f32>().ok().filter(|v| (4.0..=96.0).contains(v)).map(|v| config.font_pt = v).is_some(),
            "font_family" => {
                config.font_family = (!value.is_empty()).then(|| value.to_string());
                true
            }
            "line_height" => value.parse::<f32>().ok().filter(|v| (0.8..=3.0).contains(v)).map(|v| config.line_height = v).is_some(),
            "scrollback" => value.parse::<usize>().ok().filter(|v| *v <= 1_000_000).map(|v| config.scrollback = v).is_some(),
            "theme" => {
                let v = if value.is_empty() { "default" } else { value };
                theme_ok(v).then(|| config.theme = v.to_string()).is_some()
            }
            "theme_light" => (value != "auto" && theme_ok(value)).then(|| config.theme_light = value.to_string()).is_some(),
            "theme_dark" => (value != "auto" && theme_ok(value)).then(|| config.theme_dark = value.to_string()).is_some(),
            "opacity" => value.parse::<f32>().ok().filter(|v| (0.3..=1.0).contains(v)).map(|v| config.opacity = Some(v)).is_some(),
            "blur" => parse_bool(value).map(|v| config.blur = v).is_some(),
            "padding" => value.parse::<f32>().ok().filter(|v| (0.0..=64.0).contains(v)).map(|v| config.padding = v).is_some(),
            "window_columns" => value.parse::<u16>().ok().filter(|v| (20..=500).contains(v)).map(|v| config.window_columns = v).is_some(),
            "window_rows" => value.parse::<u16>().ok().filter(|v| (5..=200).contains(v)).map(|v| config.window_rows = v).is_some(),
            "cursor_style" => CursorShape::parse(value).map(|v| config.cursor_style = Some(v)).is_some(),
            "cursor_blink" => parse_bool(value).map(|v| config.cursor_blink = Some(v)).is_some(),
            "option_as_meta" => parse_bool(value).map(|v| config.option_as_meta = v).is_some(),
            "copy_on_select" => parse_bool(value).map(|v| config.copy_on_select = v).is_some(),
            "confirm_paste" => parse_bool(value).map(|v| config.confirm_paste = v).is_some(),
            _ => {
                // Colour keys are checked against a scratch theme, then kept as text.
                let mut scratch = Theme::by_name("glass-dark").unwrap();
                match scratch.set(key, value) {
                    Some(Ok(())) => {
                        config.colors.push((key.to_string(), value.to_string()));
                        continue;
                    }
                    Some(Err(e)) => {
                        warnings.push(format!("line {}: {e}", n + 1));
                        continue;
                    }
                    None => {
                        warnings.push(format!("line {}: unknown setting {key:?}", n + 1));
                        continue;
                    }
                }
            }
        };
        if !ok {
            warnings.push(format!("line {}: bad value {value:?} for {key}", n + 1));
        }
    }
    (config, warnings)
}

/// Drops a `#` comment that starts a line or follows a space, leaving `#` inside quotes and in
/// colours like `#1c1c1f` alone.
fn strip_comment(line: &str) -> &str {
    let mut quote = None;
    let mut prev = ' ';
    for (i, c) in line.char_indices() {
        match (c, quote) {
            ('"' | '\'', None) => quote = Some(c),
            (c, Some(q)) if c == q => quote = None,
            ('#', None) if prev.is_whitespace() && !line[i + 1..].starts_with(|c: char| c.is_ascii_hexdigit()) => return &line[..i],
            ('#', None) if i == 0 => return &line[..i],
            _ => {}
        }
        prev = c;
    }
    line
}

/// Sets `key = value` in the file: the first line for that key is replaced in place, keeping
/// any comment after it, or the line is added at the end. A missing file starts from
/// [`TEMPLATE`]. Commented-out lines (`# key = ...`) are left alone.
pub fn write_key(path: &Path, key: &str, value: &str) -> std::io::Result<()> {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            TEMPLATE.to_string()
        }
        Err(e) => return Err(e),
    };
    std::fs::write(path, set_key(&text, key, value))
}

/// Text form of [`write_key`].
pub fn set_key(text: &str, key: &str, value: &str) -> String {
    let quoted = if value.contains(' ') || value.contains('#') && !value.starts_with('#') { format!("\"{value}\"") } else { value.to_string() };
    let mut out = String::with_capacity(text.len() + 32);
    let mut done = false;
    for line in text.lines() {
        let body = strip_comment(line);
        let is_key = !done && body.split_once('=').is_some_and(|(k, _)| k.trim() == key);
        if is_key {
            let comment = &line[body.len()..];
            let pad = if comment.is_empty() { "" } else { "  " };
            out.push_str(&format!("{key} = {quoted}{pad}{}", comment.trim_start()));
            done = true;
        } else {
            out.push_str(line);
        }
        out.push('\n');
    }
    if !done {
        out.push_str(&format!("{key} = {quoted}\n"));
    }
    out
}

/// The theme after `current` in the cycle auto → each name → default → auto.
pub fn next_theme(current: &str, names: &[String]) -> String {
    let mut cycle: Vec<&str> = vec!["auto"];
    cycle.extend(names.iter().map(String::as_str));
    cycle.push("default");
    let i = cycle.iter().position(|n| *n == current).map_or(0, |i| (i + 1) % cycle.len());
    cycle[i].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn builtin(name: &str) -> bool {
        Theme::by_name(name).is_some()
    }

    #[test]
    fn empty_is_default() {
        assert_eq!(parse("", &builtin), (Config::default(), vec![]));
    }

    #[test]
    fn template_parses_to_defaults() {
        assert_eq!(parse(TEMPLATE, &builtin), (Config::default(), vec![]));
    }

    #[test]
    fn reads_all_keys() {
        let text = "# mine\n[glyphium]\nfont_size = 15.5\ntheme = \"graduation\"  # loud\nscrollback=500\nfont_family = \"SF Mono\"\nline_height = 1.4\ntheme_light = solarized-light\ntheme_dark = dracula\nopacity = 0.75\nblur = off\npadding = 12\nwindow_columns = 120\nwindow_rows = 40\ncursor_style = bar\ncursor_blink = false\noption_as_meta = yes\ncopy_on_select = true\nconfirm_paste = false\nbackground = #101010 # dark\npalette = 4=#3366ff\n";
        let (c, w) = parse(text, &builtin);
        assert!(w.is_empty(), "{w:?}");
        assert_eq!(
            c,
            Config {
                font_pt: 15.5,
                font_family: Some("SF Mono".into()),
                line_height: 1.4,
                theme: "graduation".into(),
                theme_light: "solarized-light".into(),
                theme_dark: "dracula".into(),
                colors: vec![("background".into(), "#101010".into()), ("palette".into(), "4=#3366ff".into())],
                opacity: Some(0.75),
                blur: false,
                padding: 12.0,
                window_columns: 120,
                window_rows: 40,
                cursor_style: Some(CursorShape::Bar),
                cursor_blink: Some(false),
                option_as_meta: true,
                copy_on_select: true,
                confirm_paste: false,
                scrollback: 500,
            }
        );
    }

    #[test]
    fn bad_lines_warn_and_keep_defaults() {
        let (c, w) = parse("font_size = huge\ntheme = neon\nwhat = 1\njunk\nscrollback = -3\nopacity = 0.1\ncursor_style = box\nbackground = #zzz\ntheme_dark = auto\n", &builtin);
        assert_eq!(c, Config::default());
        assert_eq!(w.len(), 9, "{w:?}");
    }

    #[test]
    fn user_themes_count_as_known() {
        let (c, w) = parse("theme = my-theme\n", &|n| n == "my-theme");
        assert!(w.is_empty());
        assert_eq!(c.theme, "my-theme");
    }

    #[test]
    fn default_and_auto_theme_names() {
        assert_eq!(parse("theme = nujabes\ntheme = default", &builtin).0.theme, "default");
        assert_eq!(parse("theme =", &builtin).0.theme, "default");
        assert_eq!(parse("theme = auto", &builtin).0.theme, "auto");
    }

    #[test]
    fn missing_file_is_default() {
        assert_eq!(load(Some(Path::new("/nonexistent/glyphium.toml")), &builtin), (Config::default(), vec![]));
    }

    #[test]
    fn theme_cycle_visits_everything_and_returns() {
        let names: Vec<String> = Theme::NAMES.iter().map(|s| s.to_string()).collect();
        let mut t = "auto".to_string();
        let mut seen = vec![];
        for _ in 0..names.len() + 2 {
            t = next_theme(&t, &names);
            seen.push(t.clone());
        }
        assert_eq!(seen.first().map(String::as_str), Some("glass-dark"));
        assert_eq!(seen[names.len()], "default");
        assert_eq!(seen.last().map(String::as_str), Some("auto"));
        assert_eq!(next_theme("/some/file", &names), "auto", "an unknown current starts over");
    }

    #[test]
    fn cursor_sequences() {
        let mut c = Config::default();
        assert_eq!(c.cursor_sequence(), None);
        c.cursor_style = Some(CursorShape::Bar);
        assert_eq!(c.cursor_sequence().as_deref(), Some("\x1b[5 q"));
        c.cursor_blink = Some(false);
        assert_eq!(c.cursor_sequence().as_deref(), Some("\x1b[6 q"));
        c.cursor_style = None;
        assert_eq!(c.cursor_sequence().as_deref(), Some("\x1b[2 q"));
    }

    #[test]
    fn set_key_replaces_in_place_and_keeps_comments() {
        let text = "# top\nfont_size = 13  # small\n# theme = auto\ntheme = nujabes\nopacity = 0.9\n";
        let out = set_key(text, "theme", "dracula");
        assert_eq!(out, "# top\nfont_size = 13  # small\n# theme = auto\ntheme = dracula\nopacity = 0.9\n");
        let out = set_key(&out, "font_size", "15");
        assert!(out.contains("font_size = 15  # small\n"), "{out}");
        let out = set_key(&out, "font_family", "SF Mono");
        assert!(out.ends_with("font_family = \"SF Mono\"\n"), "{out}");
        let out = set_key(&out, "background", "#101010");
        assert!(out.ends_with("background = #101010\n"), "{out}");
        let (c, w) = parse(&out, &builtin);
        assert!(w.is_empty(), "{w:?}");
        assert_eq!((c.font_pt, c.theme.as_str(), c.font_family.as_deref()), (15.0, "dracula", Some("SF Mono")));
        assert_eq!(c.colors, vec![("background".to_string(), "#101010".to_string())]);
    }

    #[test]
    fn write_key_creates_the_file_from_the_template() {
        let dir = std::env::temp_dir().join(format!("glyphium-config-test-{}", std::process::id()));
        let path = dir.join("sub/config.toml");
        let _ = std::fs::remove_dir_all(&dir);
        write_key(&path, "opacity", "0.7").unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.starts_with("# Glyphium settings"));
        assert_eq!(parse(&text, &builtin).0.opacity, Some(0.7));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
