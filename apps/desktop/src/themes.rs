//! Every theme the app can show: the built-ins plus theme files in the settings folder's
//! `themes/` directory (Ghostty format; the file name is the theme name, and a file wins over a
//! built-in of the same name). `theme = auto` picks a light or dark theme by system appearance.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use glyphium_terminal_core::Theme;

use crate::config::Config;

#[derive(Debug, Clone)]
pub struct UserTheme {
    pub name: String,
    pub theme: Theme,
}

#[derive(Debug, Clone, Default)]
pub struct Themes {
    user: Vec<UserTheme>,
}

/// File types read from `themes/`. Ghostty's theme files have no extension.
fn is_theme_file(path: &Path) -> bool {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    if name.starts_with('.') || !path.is_file() {
        return false;
    }
    matches!(path.extension().and_then(|e| e.to_str()), None | Some("toml" | "conf" | "theme" | "ghostty"))
}

/// The newest modification time of a file or of a folder and the files in it; used to notice
/// edits without watching the disk.
pub fn newest_mtime(paths: &[&Path]) -> Option<SystemTime> {
    let mut newest = None;
    let mut see = |p: &Path| {
        if let Ok(t) = std::fs::metadata(p).and_then(|m| m.modified()) {
            newest = newest.max(Some(t));
        }
    };
    for p in paths {
        see(p);
        if let Ok(entries) = std::fs::read_dir(p) {
            for e in entries.flatten() {
                see(&e.path());
            }
        }
    }
    newest
}

impl Themes {
    /// Reads the theme files in `dir`; a missing folder gives just the built-ins.
    pub fn load(dir: Option<&Path>) -> (Self, Vec<String>) {
        let mut user = vec![];
        let mut warnings = vec![];
        let mut paths: Vec<PathBuf> = dir.and_then(|d| std::fs::read_dir(d).ok()).map(|r| r.flatten().map(|e| e.path()).filter(|p| is_theme_file(p)).collect()).unwrap_or_default();
        paths.sort();
        for path in paths {
            let Some(name) = path.file_stem().and_then(|n| n.to_str()).map(str::to_string) else { continue };
            if name == "auto" || name == "default" {
                warnings.push(format!("themes/{name}: that name is reserved; rename the file"));
                continue;
            }
            match std::fs::read_to_string(&path) {
                Ok(text) => {
                    let (theme, w) = Theme::parse(&text);
                    warnings.extend(w.into_iter().map(|w| format!("themes/{name}: {w}")));
                    user.push(UserTheme { name, theme });
                }
                Err(e) => warnings.push(format!("themes/{name}: {e}")),
            }
        }
        (Self { user }, warnings)
    }

    /// A user theme, a built-in, or a theme file named by path.
    pub fn get(&self, name: &str) -> Option<Theme> {
        if let Some(u) = self.user.iter().find(|u| u.name == name) {
            return Some(u.theme);
        }
        if let Some(t) = Theme::by_name(name) {
            return Some(t);
        }
        if name.contains('/') {
            let path = expand_home(name);
            return std::fs::read_to_string(path).ok().map(|t| Theme::parse(&t).0);
        }
        None
    }

    pub fn exists(&self, name: &str) -> bool {
        self.get(name).is_some()
    }

    /// Theme names for menus and the theme cycle, with the name to show: built-ins first, then
    /// user themes that are not overriding a built-in.
    pub fn list(&self) -> Vec<(String, String)> {
        let mut out: Vec<(String, String)> = Theme::built_in().iter().map(|b| (b.name.to_string(), b.display.to_string())).collect();
        for u in &self.user {
            if Theme::by_name(&u.name).is_none() {
                out.push((u.name.clone(), display_name(&u.name)));
            }
        }
        out
    }

    pub fn names(&self) -> Vec<String> {
        self.list().into_iter().map(|(n, _)| n).collect()
    }

    /// The theme name in force: `auto` resolved by appearance. `dark` is the window's
    /// appearance (`None` when unknown, taken as dark).
    pub fn effective_name(config: &Config, chosen: &str, dark: Option<bool>) -> String {
        if chosen == "auto" {
            if dark.unwrap_or(true) { config.theme_dark.clone() } else { config.theme_light.clone() }
        } else {
            chosen.to_string()
        }
    }

    /// The colours to set: `None` keeps Ghostty's defaults (theme `default` with no colour
    /// overrides). Overrides on `default` apply over Glass Dark. Returns the theme and the
    /// window opacity to use.
    pub fn resolve(&self, config: &Config, chosen: &str, dark: Option<bool>) -> (Option<Theme>, f32) {
        let name = Self::effective_name(config, chosen, dark);
        let mut theme = if name == "default" { None } else { self.get(&name) };
        if !config.colors.is_empty() {
            let mut t = theme.unwrap_or_else(|| Theme::by_name("glass-dark").unwrap());
            for (k, v) in &config.colors {
                let _ = t.set(k, v);
            }
            theme = Some(t);
        }
        let opacity = config.opacity.or(theme.and_then(|t| t.opacity)).unwrap_or(1.0);
        (theme, opacity)
    }
}

/// `my-theme_2` → `My Theme 2`.
pub fn display_name(name: &str) -> String {
    let base = Path::new(name).file_stem().and_then(|s| s.to_str()).unwrap_or(name);
    base.split(['-', '_', ' '])
        .filter(|w| !w.is_empty())
        .map(|w| {
            let mut c = w.chars();
            c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// The display name of a theme the user might pick: built-in name, user name, or a path.
pub fn display(themes: &Themes, name: &str) -> String {
    match name {
        "auto" => "Auto (follow system)".into(),
        "default" => "Ghostty Default".into(),
        _ => themes.list().into_iter().find(|(n, _)| n == name).map_or_else(|| display_name(name), |(_, d)| d),
    }
}

fn expand_home(path: &str) -> PathBuf {
    match (path.strip_prefix("~/"), std::env::var_os("HOME")) {
        (Some(rest), Some(home)) => Path::new(&home).join(rest),
        _ => PathBuf::from(path),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("glyphium-themes-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn user_files_load_and_override_built_ins() {
        let d = temp_dir("load");
        std::fs::write(d.join("Night Owl"), "background = #011627\nforeground = #d6deeb\n").unwrap();
        std::fs::write(d.join("dracula.toml"), "background = #000000\n").unwrap();
        std::fs::write(d.join("auto"), "background = #000000\n").unwrap();
        std::fs::write(d.join("notes.md"), "not a theme").unwrap();
        let (themes, warnings) = Themes::load(Some(&d));
        assert_eq!(warnings.len(), 1, "{warnings:?}");
        assert_eq!(themes.get("Night Owl").unwrap().background, glyphium_terminal_core::Rgb { r: 0x01, g: 0x16, b: 0x27 });
        assert_eq!(themes.get("dracula").unwrap().background, glyphium_terminal_core::Rgb { r: 0, g: 0, b: 0 });
        let names = themes.names();
        assert_eq!(names.iter().filter(|n| *n == "dracula").count(), 1);
        assert_eq!(names.last().map(String::as_str), Some("Night Owl"));
        assert!(!themes.exists("notes"));
        let path = d.join("Night Owl");
        assert!(themes.exists(path.to_str().unwrap()), "a path works too");
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn missing_folder_is_built_ins() {
        let (themes, w) = Themes::load(Some(Path::new("/nonexistent/themes")));
        assert!(w.is_empty());
        assert_eq!(themes.names().len(), Theme::NAMES.len());
    }

    #[test]
    fn auto_follows_appearance() {
        let themes = Themes::default();
        let config = Config::default();
        assert_eq!(Themes::effective_name(&config, "auto", Some(false)), "glass-light");
        assert_eq!(Themes::effective_name(&config, "auto", Some(true)), "glass-dark");
        assert_eq!(Themes::effective_name(&config, "auto", None), "glass-dark");
        assert_eq!(Themes::effective_name(&config, "nujabes", Some(false)), "nujabes");
        let (light, opacity) = themes.resolve(&config, "auto", Some(false));
        assert!(light.unwrap().is_light());
        assert_eq!(opacity, 0.84);
    }

    #[test]
    fn default_is_ghostty_unless_overridden() {
        let themes = Themes::default();
        let mut config = Config::default();
        assert_eq!(themes.resolve(&config, "default", None), (None, 1.0));
        config.colors.push(("background".into(), "#102030".into()));
        config.opacity = Some(0.5);
        let (t, o) = themes.resolve(&config, "default", None);
        assert_eq!(t.unwrap().background, glyphium_terminal_core::Rgb { r: 0x10, g: 0x20, b: 0x30 });
        assert_eq!(o, 0.5);
    }

    #[test]
    fn display_names() {
        assert_eq!(display_name("night-owl_2"), "Night Owl 2");
        assert_eq!(display(&Themes::default(), "catppuccin-mocha"), "Catppuccin Mocha");
        assert_eq!(display(&Themes::default(), "auto"), "Auto (follow system)");
    }
}
