//! Colour themes for [`crate::Terminal::set_theme`]: the built-in set, and [`Theme::parse`] for
//! theme files in Ghostty's format, so the large Ghostty and iTerm2-Color-Schemes collections
//! import as they are. NOTICE says where each built-in palette comes from.

use crate::Rgb;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Theme {
    pub foreground: Rgb,
    pub background: Rgb,
    pub cursor: Rgb,
    /// Background of selected text.
    pub selection: Rgb,
    /// ANSI colours 0 to 15. The rest of the 256-colour palette stays Ghostty's.
    pub ansi: [Rgb; 16],
    /// Suggested window background opacity, for themes made for a translucent window. The
    /// app's own `opacity` setting wins over it.
    pub opacity: Option<f32>,
}

/// A theme that ships with Glyphium.
#[derive(Clone, Copy, Debug)]
pub struct BuiltIn {
    /// Name used in config files and on the command line.
    pub name: &'static str,
    /// Name shown in menus.
    pub display: &'static str,
    pub theme: Theme,
}

const fn rgb(hex: u32) -> Rgb {
    Rgb { r: (hex >> 16) as u8, g: (hex >> 8) as u8, b: hex as u8 }
}

const fn palette(hex: [u32; 16]) -> [Rgb; 16] {
    let mut out = [Rgb { r: 0, g: 0, b: 0 }; 16];
    let mut i = 0;
    while i < 16 {
        out[i] = rgb(hex[i]);
        i += 1;
    }
    out
}

const fn theme(fg: u32, bg: u32, cursor: u32, selection: u32, ansi: [u32; 16], opacity: Option<f32>) -> Theme {
    Theme { foreground: rgb(fg), background: rgb(bg), cursor: rgb(cursor), selection: rgb(selection), ansi: palette(ansi), opacity }
}

const GLASS_DARK: Theme = theme(
    0xe8e8ed,
    0x1c1c1f,
    0x7aa7ff,
    0x3a4a6b,
    [
        0x2c2c30, 0xff6b6b, 0x5fd38d, 0xf5c451, 0x6aa6ff, 0xd58cff, 0x5ad1d1, 0xd8d8dc, //
        0x76767c, 0xff8a85, 0x7fe3a6, 0xffd76e, 0x8dbbff, 0xe3a6ff, 0x7fe5e5, 0xf5f5f7,
    ],
    Some(0.80),
);

const GLASS_LIGHT: Theme = theme(
    0x1d1d1f,
    0xf6f6f8,
    0x3478f6,
    0xbcd4fa,
    [
        0x1d1d1f, 0xc4302b, 0x1e7f46, 0x8f5d00, 0x1f5fd1, 0x8f3bb8, 0x0e7480, 0xaeaeb2, //
        0x6e6e73, 0xd9403a, 0x249354, 0xa86f00, 0x3b7cf0, 0xa452cc, 0x158a98, 0xffffff,
    ],
    Some(0.84),
);

const NUJABES: Theme = theme(
    0xe3dcda,
    0x0d0a11,
    0xb26ac6,
    0x3d2456,
    [
        0x231a2b, 0xc56e6f, 0x9aa06b, 0xe0c25a, 0x8f74e8, 0xdc7fcc, 0x7fa8b0, 0xc9c0bd, //
        0x7d747a, 0xdd8189, 0xb6bb85, 0xf2e06a, 0xa894f0, 0xe79ad8, 0x9cc3ca, 0xfdf7f7,
    ],
    None,
);

const GRADUATION: Theme = theme(
    0xfff4e0,
    0x1a0b2e,
    0xffc93c,
    0x4a2a78,
    [
        0x2a1a44, 0xff5a5f, 0x7ee081, 0xffc93c, 0x7ea6ff, 0xff5fa8, 0x4dd8e8, 0xe8dcc8, //
        0x6f5a93, 0xff8487, 0xa3efa5, 0xffdd7a, 0xa5c2ff, 0xff8fc4, 0x86e8f2, 0xfffaf0,
    ],
    None,
);

// Solarized: bright black is base01 rather than base03, which is the background itself.
const SOLARIZED_DARK: Theme = theme(
    0x839496,
    0x002b36,
    0x93a1a1,
    0x073642,
    [
        0x073642, 0xdc322f, 0x859900, 0xb58900, 0x268bd2, 0xd33682, 0x2aa198, 0xeee8d5, //
        0x586e75, 0xcb4b16, 0x586e75, 0x657b83, 0x839496, 0x6c71c4, 0x93a1a1, 0xfdf6e3,
    ],
    None,
);

const SOLARIZED_LIGHT: Theme = theme(
    0x657b83,
    0xfdf6e3,
    0x586e75,
    0xeee8d5,
    [
        0x073642, 0xdc322f, 0x859900, 0xb58900, 0x268bd2, 0xd33682, 0x2aa198, 0xeee8d5, //
        0x002b36, 0xcb4b16, 0x586e75, 0x657b83, 0x839496, 0x6c71c4, 0x93a1a1, 0xfdf6e3,
    ],
    None,
);

const DRACULA: Theme = theme(
    0xf8f8f2,
    0x282a36,
    0xf8f8f2,
    0x44475a,
    [
        0x21222c, 0xff5555, 0x50fa7b, 0xf1fa8c, 0xbd93f9, 0xff79c6, 0x8be9fd, 0xf8f8f2, //
        0x6272a4, 0xff6e6e, 0x69ff94, 0xffffa5, 0xd6acff, 0xff92df, 0xa4ffff, 0xffffff,
    ],
    None,
);

const CATPPUCCIN_MOCHA: Theme = theme(
    0xcdd6f4,
    0x1e1e2e,
    0xf5e0dc,
    0x585b70,
    [
        0x45475a, 0xf38ba8, 0xa6e3a1, 0xf9e2af, 0x89b4fa, 0xf5c2e7, 0x94e2d5, 0xbac2de, //
        0x585b70, 0xf38ba8, 0xa6e3a1, 0xf9e2af, 0x89b4fa, 0xf5c2e7, 0x94e2d5, 0xa6adc8,
    ],
    None,
);

const CATPPUCCIN_LATTE: Theme = theme(
    0x4c4f69,
    0xeff1f5,
    0xdc8a78,
    0xacb0be,
    [
        0x5c5f77, 0xd20f39, 0x40a02b, 0xdf8e1d, 0x1e66f5, 0xea76cb, 0x179299, 0xacb0be, //
        0x6c6f85, 0xd20f39, 0x40a02b, 0xdf8e1d, 0x1e66f5, 0xea76cb, 0x179299, 0xbcc0cc,
    ],
    None,
);

// Tokyo Night: bright black is the theme's comment colour, so it stays readable.
const TOKYO_NIGHT: Theme = theme(
    0xa9b1d6,
    0x1a1b26,
    0xc0caf5,
    0x33467c,
    [
        0x363b54, 0xf7768e, 0x73daca, 0xe0af68, 0x7aa2f7, 0xbb9af7, 0x7dcfff, 0x787c99, //
        0x565f89, 0xf7768e, 0x73daca, 0xe0af68, 0x7aa2f7, 0xbb9af7, 0x7dcfff, 0xacb0d0,
    ],
    None,
);

const BUILT_IN: [BuiltIn; 10] = [
    BuiltIn { name: "glass-dark", display: "Glass Dark", theme: GLASS_DARK },
    BuiltIn { name: "glass-light", display: "Glass Light", theme: GLASS_LIGHT },
    BuiltIn { name: "nujabes", display: "Nujabes", theme: NUJABES },
    BuiltIn { name: "graduation", display: "Graduation", theme: GRADUATION },
    BuiltIn { name: "solarized-dark", display: "Solarized Dark", theme: SOLARIZED_DARK },
    BuiltIn { name: "solarized-light", display: "Solarized Light", theme: SOLARIZED_LIGHT },
    BuiltIn { name: "dracula", display: "Dracula", theme: DRACULA },
    BuiltIn { name: "catppuccin-mocha", display: "Catppuccin Mocha", theme: CATPPUCCIN_MOCHA },
    BuiltIn { name: "catppuccin-latte", display: "Catppuccin Latte", theme: CATPPUCCIN_LATTE },
    BuiltIn { name: "tokyo-night", display: "Tokyo Night", theme: TOKYO_NIGHT },
];

/// Parses `#rrggbb`, `rrggbb` or `#rgb`.
pub fn parse_color(s: &str) -> Option<Rgb> {
    let hex = s.trim().trim_start_matches('#');
    if !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let v = u32::from_str_radix(hex, 16).ok()?;
    match hex.len() {
        6 => Some(rgb(v)),
        3 => Some(Rgb { r: ((v >> 8) & 0xf) as u8 * 17, g: ((v >> 4) & 0xf) as u8 * 17, b: (v & 0xf) as u8 * 17 }),
        _ => None,
    }
}

/// Relative luminance (WCAG), 0 for black to 1 for white.
pub fn luminance(c: Rgb) -> f32 {
    let lin = |v: u8| {
        let v = v as f32 / 255.0;
        if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
    };
    0.2126 * lin(c.r) + 0.7152 * lin(c.g) + 0.0722 * lin(c.b)
}

/// WCAG contrast ratio between two colours, from 1 to 21.
pub fn contrast(a: Rgb, b: Rgb) -> f32 {
    let (la, lb) = (luminance(a), luminance(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

impl Theme {
    pub const NAMES: [&str; 10] = {
        let mut names = [""; 10];
        let mut i = 0;
        while i < 10 {
            names[i] = BUILT_IN[i].name;
            i += 1;
        }
        names
    };

    pub fn built_in() -> &'static [BuiltIn] {
        &BUILT_IN
    }

    pub fn by_name(name: &str) -> Option<Theme> {
        BUILT_IN.iter().find(|b| b.name == name).map(|b| b.theme)
    }

    /// A light background, so the window chrome and `auto` can match it.
    pub fn is_light(&self) -> bool {
        luminance(self.background) > 0.4
    }

    /// Sets one colour by its Ghostty key name (`background`, `cursor-color`, `palette` with
    /// `N=#hex`, ...); underscores work as well as dashes. `None` for a key that isn't a colour
    /// key, `Some(Err)` for a bad value.
    pub fn set(&mut self, key: &str, value: &str) -> Option<Result<(), String>> {
        let key = key.trim().replace('_', "-");
        let value = value.trim().trim_matches('"').trim_matches('\'');
        let color = |v: &str| parse_color(v).ok_or_else(|| format!("bad colour {v:?} for {key}"));
        let result = match key.as_str() {
            "background" => color(value).map(|c| self.background = c),
            "foreground" => color(value).map(|c| self.foreground = c),
            "cursor-color" | "cursor" => color(value).map(|c| self.cursor = c),
            "selection-background" | "selection" => color(value).map(|c| self.selection = c),
            "palette" => {
                let parsed = value.split_once('=').and_then(|(n, c)| Some((n.trim().parse::<usize>().ok().filter(|n| *n < 16)?, parse_color(c)?)));
                match parsed {
                    Some((n, c)) => {
                        self.ansi[n] = c;
                        Ok(())
                    }
                    // Entries past 15 are valid Ghostty lines; this theme keeps only the first 16.
                    None if value.split_once('=').and_then(|(n, _)| n.trim().parse::<usize>().ok()).is_some_and(|n| (16..256).contains(&n)) => Ok(()),
                    None => Err(format!("bad palette entry {value:?}, expected N=#rrggbb with N from 0 to 15")),
                }
            }
            "background-opacity" | "opacity" => match value.parse::<f32>() {
                Ok(v) if (0.0..=1.0).contains(&v) => {
                    self.opacity = Some(v);
                    Ok(())
                }
                _ => Err(format!("bad opacity {value:?}, expected 0 to 1")),
            },
            // Ghostty keys a theme may carry that this terminal does not use.
            "cursor-text" | "selection-foreground" => Ok(()),
            _ => return None,
        };
        Some(result)
    }

    /// Reads a theme file in Ghostty's `key = value` format. Keys it lacks come from Glass Dark
    /// (without its opacity); each line that could not be used gives a warning.
    pub fn parse(text: &str) -> (Theme, Vec<String>) {
        let mut theme = Theme { opacity: None, ..GLASS_DARK };
        let mut warnings = vec![];
        for (n, raw) in text.lines().enumerate() {
            // `#` starts a comment only at the start of a line: colours contain it.
            let line = raw.trim();
            if line.is_empty() || line.starts_with('#') || line.starts_with("//") {
                continue;
            }
            let Some((key, value)) = line.split_once('=') else {
                warnings.push(format!("line {}: expected key = value", n + 1));
                continue;
            };
            match theme.set(key, value) {
                Some(Ok(())) => {}
                Some(Err(e)) => warnings.push(format!("line {}: {e}", n + 1)),
                None => warnings.push(format!("line {}: unknown theme key {:?}", n + 1, key.trim())),
            }
        }
        (theme, warnings)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_match_the_table() {
        assert_eq!(Theme::NAMES.len(), Theme::built_in().len());
        for b in Theme::built_in() {
            assert_eq!(Theme::by_name(b.name), Some(b.theme));
        }
        assert_eq!(Theme::by_name("nope"), None);
    }

    #[test]
    fn built_ins_are_readable() {
        for b in Theme::built_in() {
            let t = b.theme;
            // Solarized Light is published at about 4.1:1; everything else reaches 4.5:1.
            let ratio = contrast(t.foreground, t.background);
            assert!(ratio >= 4.0, "{}: text contrast {ratio:.2}", b.name);
            // A filled block: Catppuccin Latte's official rosewater cursor is about 2.3:1.
            assert!(contrast(t.cursor, t.background) >= 2.2, "{}: cursor", b.name);
            // Bright black (comments, hints) must not vanish into the background.
            assert!(contrast(t.ansi[8], t.background) >= 1.8, "{}: bright black {:.2}", b.name, contrast(t.ansi[8], t.background));
        }
    }

    #[test]
    fn light_and_dark() {
        assert!(Theme::by_name("glass-light").unwrap().is_light());
        assert!(Theme::by_name("catppuccin-latte").unwrap().is_light());
        assert!(!Theme::by_name("glass-dark").unwrap().is_light());
        assert!(!Theme::by_name("dracula").unwrap().is_light());
    }

    #[test]
    fn parses_a_ghostty_theme_file() {
        let text = "# Ghostty theme\npalette = 0=#1d1f21\npalette = 1=cc6666\npalette = 15=#fff\npalette = 200=#123456\nbackground = 1d1f21\nforeground = #c5c8c6\ncursor-color = #c5c8c6\ncursor-text = #1d1f21\nselection-background = #373b41\nselection-foreground = #c5c8c6\nbackground-opacity = 0.9\n";
        let (t, w) = Theme::parse(text);
        assert!(w.is_empty(), "{w:?}");
        assert_eq!(t.background, rgb(0x1d1f21));
        assert_eq!(t.foreground, rgb(0xc5c8c6));
        assert_eq!(t.selection, rgb(0x373b41));
        assert_eq!((t.ansi[0], t.ansi[1], t.ansi[15]), (rgb(0x1d1f21), rgb(0xcc6666), rgb(0xffffff)));
        assert_eq!(t.ansi[2], GLASS_DARK.ansi[2], "missing entries come from Glass Dark");
        assert_eq!(t.opacity, Some(0.9));
    }

    #[test]
    fn bad_theme_lines_warn() {
        let (t, w) = Theme::parse("background = purple\npalette = 3\nfont = x\njunk\nbackground-opacity = 2\n");
        assert_eq!(w.len(), 5, "{w:?}");
        assert_eq!(t, Theme { opacity: None, ..GLASS_DARK });
    }

    #[test]
    fn snake_case_keys() {
        let mut t = GLASS_DARK;
        assert_eq!(t.set("cursor_color", "\"#010203\""), Some(Ok(())));
        assert_eq!(t.cursor, rgb(0x010203));
        assert_eq!(t.set("font_size", "3"), None);
    }
}
