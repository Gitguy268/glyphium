//! Settings from a small `key = value` file. Unknown keys and bad values print a warning and
//! keep the default, so a broken file never stops the terminal from starting.
//!
//! ```text
//! # ~/.config/glyphium/config.toml
//! font_size = 14        # points
//! theme = "nujabes"     # default, nujabes or graduation
//! scrollback = 20000    # lines per pane
//! ```

use std::path::{Path, PathBuf};

use glyphium_terminal_core::Theme;

#[derive(Debug, Clone, PartialEq)]
pub struct Config {
    pub font_pt: f32,
    /// `None` is Ghostty's default colours.
    pub theme: Option<String>,
    pub scrollback: usize,
}

impl Default for Config {
    fn default() -> Self {
        Self { font_pt: 13.0, theme: None, scrollback: 10_000 }
    }
}

/// The file to read: `--config`, then `GLYPHIUM_CONFIG`, then `~/.config/glyphium/config.toml`.
pub fn path(flag: Option<PathBuf>) -> Option<PathBuf> {
    flag.or_else(|| std::env::var_os("GLYPHIUM_CONFIG").map(PathBuf::from))
        .or_else(|| std::env::var_os("HOME").map(|h| Path::new(&h).join(".config/glyphium/config.toml")))
}

/// Reads the file; a missing one gives the defaults silently.
pub fn load(path: Option<&Path>) -> Config {
    let Some(path) = path else { return Config::default() };
    match std::fs::read_to_string(path) {
        Ok(text) => {
            let (config, warnings) = parse(&text);
            for w in warnings {
                eprintln!("glyphium: {}: {w}", path.display());
            }
            config
        }
        Err(_) => Config::default(),
    }
}

/// Parses the text, returning the settings and a warning per line that was ignored.
pub fn parse(text: &str) -> (Config, Vec<String>) {
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
        let ok = match key {
            "font_size" => value.parse::<f32>().ok().filter(|v| (4.0..=96.0).contains(v)).map(|v| config.font_pt = v).is_some(),
            "scrollback" => value.parse::<usize>().ok().filter(|v| *v <= 1_000_000).map(|v| config.scrollback = v).is_some(),
            "theme" => match value {
                "default" | "" => {
                    config.theme = None;
                    true
                }
                name if Theme::by_name(name).is_some() => {
                    config.theme = Some(name.to_string());
                    true
                }
                _ => false,
            },
            _ => {
                warnings.push(format!("line {}: unknown setting {key:?}", n + 1));
                continue;
            }
        };
        if !ok {
            warnings.push(format!("line {}: bad value {value:?} for {key}", n + 1));
        }
    }
    (config, warnings)
}

/// Drops a `#` comment, leaving `#` inside quotes alone.
fn strip_comment(line: &str) -> &str {
    let mut quote = None;
    for (i, c) in line.char_indices() {
        match (c, quote) {
            ('"' | '\'', None) => quote = Some(c),
            (c, Some(q)) if c == q => quote = None,
            ('#', None) => return &line[..i],
            _ => {}
        }
    }
    line
}

/// The theme after `current` in the cycle default → each named theme → default.
pub fn next_theme(current: Option<&str>) -> Option<String> {
    let names = Theme::NAMES;
    match current.and_then(|c| names.iter().position(|n| *n == c)) {
        None => Some(names[0].to_string()),
        Some(i) if i + 1 < names.len() => Some(names[i + 1].to_string()),
        Some(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_is_default() {
        assert_eq!(parse(""), (Config::default(), vec![]));
    }

    #[test]
    fn reads_all_keys() {
        let (c, w) = parse("# mine\n[glyphium]\nfont_size = 15.5\ntheme = \"graduation\"  # loud\nscrollback=500\n");
        assert!(w.is_empty(), "{w:?}");
        assert_eq!(c, Config { font_pt: 15.5, theme: Some("graduation".into()), scrollback: 500 });
    }

    #[test]
    fn bad_lines_warn_and_keep_defaults() {
        let (c, w) = parse("font_size = huge\ntheme = neon\nwhat = 1\njunk\nscrollback = -3\n");
        assert_eq!(c, Config::default());
        assert_eq!(w.len(), 5, "{w:?}");
    }

    #[test]
    fn default_theme_name() {
        assert_eq!(parse("theme = nujabes\ntheme = default").0.theme, None);
    }

    #[test]
    fn missing_file_is_default() {
        assert_eq!(load(Some(Path::new("/nonexistent/glyphium.toml"))), Config::default());
    }

    #[test]
    fn theme_cycle_returns_to_default() {
        let mut t = None;
        let mut seen = vec![];
        for _ in 0..=Theme::NAMES.len() {
            t = next_theme(t.as_deref());
            seen.push(t.clone());
        }
        assert_eq!(seen, [Some("nujabes".into()), Some("graduation".into()), None]);
    }
}
