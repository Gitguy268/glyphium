//! The macOS menu bar, laid out like Terminal's. Item ids are the strings `main.rs` maps to
//! actions (`action:new-tab`, `theme:dracula`), so scripts can send the same ids. The menu owns
//! the Cmd shortcuts on macOS: winit's view never sees a key the menu handled.

use muda::accelerator::{Accelerator, Code, Modifiers};
use muda::{AboutMetadata, CheckMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu};

pub struct AppMenu {
    _menu: Menu,
    theme_menu: Submenu,
    theme_items: Vec<(String, CheckMenuItem)>,
}

fn item(id: &str, text: &str, mods: Modifiers, key: Option<Code>) -> MenuItem {
    let accel = key.map(|k| Accelerator::new(mods, k));
    MenuItem::with_id(format!("action:{id}"), text, true, accel)
}

/// Cmd on macOS.
const CMD: Modifiers = Modifiers::META;
const CMD_SHIFT: Modifiers = Modifiers::META.union(Modifiers::SHIFT);
const NONE: Modifiers = Modifiers::empty();

impl AppMenu {
    /// Builds the menu and installs it as the app's main menu. `themes` is (name, display).
    pub fn new(themes: &[(String, String)], current: &str) -> Self {
        let menu = Menu::new();
        let sep = PredefinedMenuItem::separator;
        let about = AboutMetadata {
            name: Some("Glyphium".into()),
            version: Some(env!("CARGO_PKG_VERSION").into()),
            comments: Some("GPU terminal".into()),
            license: Some("MIT".into()),
            ..Default::default()
        };
        let app = Submenu::with_items(
            "Glyphium",
            true,
            &[
                &PredefinedMenuItem::about(Some("About Glyphium"), Some(about)),
                &sep(),
                &item("settings", "Settings…", CMD, Some(Code::Comma)),
                &item("open-config", "Open Config File", NONE, None),
                &item("reload-config", "Reload Settings", NONE, None),
                &sep(),
                &PredefinedMenuItem::hide(Some("Hide Glyphium")),
                &PredefinedMenuItem::hide_others(None),
                &PredefinedMenuItem::show_all(None),
                &sep(),
                &item("quit", "Quit Glyphium", CMD, Some(Code::KeyQ)),
            ],
        )
        .expect("app menu");
        let shell = Submenu::with_items(
            "Shell",
            true,
            &[
                &item("new-tab", "New Tab", CMD, Some(Code::KeyT)),
                &item("split-right", "Split Right", CMD, Some(Code::KeyD)),
                &item("split-down", "Split Down", CMD_SHIFT, Some(Code::KeyD)),
                &sep(),
                &item("queue", "Queue Next Command…", CMD_SHIFT, Some(Code::Enter)),
                &item("clear-queue", "Clear Queue", CMD_SHIFT, Some(Code::Backspace)),
                &sep(),
                &item("close", "Close", CMD, Some(Code::KeyW)),
            ],
        )
        .expect("shell menu");
        let edit = Submenu::with_items("Edit", true, &[&item("copy", "Copy", CMD, Some(Code::KeyC)), &item("paste", "Paste", CMD, Some(Code::KeyV))]).expect("edit menu");
        let theme_menu = Submenu::new("Theme", true);
        let view = Submenu::with_items(
            "View",
            true,
            &[
                &theme_menu,
                &item("next-theme", "Next Theme", CMD_SHIFT, Some(Code::KeyT)),
                &sep(),
                &item("zoom-in", "Bigger", CMD, Some(Code::Equal)),
                &item("zoom-out", "Smaller", CMD, Some(Code::Minus)),
                &item("zoom-reset", "Actual Size", CMD, Some(Code::Digit0)),
                &sep(),
                &PredefinedMenuItem::fullscreen(None),
            ],
        )
        .expect("view menu");
        let window = Submenu::with_items(
            "Window",
            true,
            &[
                &PredefinedMenuItem::minimize(None),
                &sep(),
                &item("prev-tab", "Show Previous Tab", CMD, Some(Code::BracketLeft)),
                &item("next-tab", "Show Next Tab", CMD, Some(Code::BracketRight)),
                &sep(),
                &PredefinedMenuItem::bring_all_to_front(None),
            ],
        )
        .expect("window menu");
        menu.append_items(&[&app, &shell, &edit, &view, &window]).expect("menu bar");
        menu.init_for_nsapp();
        window.set_as_windows_menu_for_nsapp();
        let mut this = Self { _menu: menu, theme_menu, theme_items: vec![] };
        this.set_themes(themes, current);
        this
    }

    /// Rebuilds the Theme submenu, after theme files changed.
    pub fn set_themes(&mut self, themes: &[(String, String)], current: &str) {
        while self.theme_menu.remove_at(0).is_some() {}
        self.theme_items.clear();
        let check = |name: &str, text: &str| CheckMenuItem::with_id(format!("theme:{name}"), text, true, false, None);
        let mut add = |name: &str, text: &str| {
            let c = check(name, text);
            let _ = self.theme_menu.append(&c);
            self.theme_items.push((name.to_string(), c));
        };
        add("auto", "Auto (Follow System)");
        let _ = self.theme_menu.append(&PredefinedMenuItem::separator());
        for (name, display) in themes {
            add(name, display);
        }
        let _ = self.theme_menu.append(&PredefinedMenuItem::separator());
        add("default", "Ghostty Default");
        let _ = self.theme_menu.append(&PredefinedMenuItem::separator());
        let _ = self.theme_menu.append(&item("open-themes", "Open Themes Folder", NONE, None));
        self.check_theme(current);
    }

    /// Ticks the chosen theme.
    pub fn check_theme(&self, current: &str) {
        for (name, item) in &self.theme_items {
            item.set_checked(name == current);
        }
    }
}
