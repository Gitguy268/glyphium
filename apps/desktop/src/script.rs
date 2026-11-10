//! Scripted runs for end-to-end checks of the real window, GPU and shell. One step per line;
//! see `tests/e2e/`. Steps drive the same code paths as user input past the OS event, and
//! `expect` lines fail the process, so a script is a test.

use std::time::Instant;

use glyphium_terminal_core::Frame;

#[derive(Debug, PartialEq)]
pub enum Step {
    Wait(u64),
    Type(String),
    Key(String),
    Paste(String),
    NewTab,
    SplitRight,
    SplitDown,
    NextTab,
    ClosePane,
    Zoom(f32),
    Resize(u32, u32),
    LoseDevice,
    Screenshot(String),
    /// Mouse drag between two cells of the focused pane.
    Drag((u16, u16), (u16, u16)),
    DoubleClick((u16, u16)),
    Wheel(f32),
    ExpectSelection(String),
    ExpectBlink,
    ExpectScreen(String),
    ExpectNotScreen(String),
    ExpectPanes(usize),
    ExpectTabs(usize),
    ReportIdleFrames(u64),
    /// Draws allowed across the idle interval; `None` allows one per cursor blink.
    ExpectIdle(Option<u64>),
    /// Queues a command in the focused pane, as Enter in the queue prompt does.
    Queue(String),
    ExpectQueued(usize),
    /// Opens the queue prompt, as its shortcut does.
    OpenQueue,
    /// Input to the open queue prompt.
    Prompt(crate::PromptInput),
    /// `auto`, `default` or a theme name.
    Theme(String),
    /// Re-reads the config file and themes if they changed, as focusing the window does.
    ReloadConfig,
    /// Pretends the system appearance changed: true for dark.
    Appearance(bool),
    /// The theme in force, with `auto` resolved.
    ExpectTheme(String),
    /// Sets a key in the config file the app was started with, as an editor would.
    WriteConfig(String, String),
    /// Writes a theme file into the `themes/` folder next to the config file.
    WriteTheme(String, String),
    /// The config file has `key = value`.
    ExpectConfig(String, String),
    /// A menu item id, as choosing it in the menu bar sends.
    Menu(String),
    /// A click on the window chrome: `tab N`, `close N`, `new-tab`, `queue`, or `terminal`
    /// (a click into the focused pane).
    ClickChrome(String),
    ExpectPrompt(bool),
    OpenSettings,
    /// Up, Down, Left, Right, Enter or Escape in the Settings panel.
    SettingsKey(crate::SettingsInput),
    Quit,
}

/// `\n`, `\r`, `\t` and `\e` in text arguments.
fn unescape(s: &str) -> String {
    s.replace("\\n", "\n").replace("\\r", "\r").replace("\\t", "\t").replace("\\e", "\x1b")
}

pub fn parse(source: &str) -> Vec<Step> {
    source
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(|line| {
            let (cmd, arg) = line.split_once(' ').unwrap_or((line, ""));
            match cmd {
                "wait" => Step::Wait(arg.parse().expect("wait MS")),
                "type" => Step::Type(unescape(arg)),
                "key" => Step::Key(arg.into()),
                "paste" => Step::Paste(unescape(arg)),
                "new-tab" => Step::NewTab,
                "split-right" => Step::SplitRight,
                "split-down" => Step::SplitDown,
                "next-tab" => Step::NextTab,
                "close-pane" => Step::ClosePane,
                "zoom" => Step::Zoom(arg.parse().expect("zoom PX")),
                "resize" => {
                    let (w, h) = arg.split_once(' ').expect("resize W H");
                    Step::Resize(w.parse().unwrap(), h.parse().unwrap())
                }
                "lose-device" => Step::LoseDevice,
                "screenshot" => Step::Screenshot(arg.into()),
                "drag" => {
                    let n: Vec<u16> = arg.split(' ').map(|v| v.parse().expect("drag C R C R")).collect();
                    Step::Drag((n[0], n[1]), (n[2], n[3]))
                }
                "double-click" => {
                    let (c, r) = arg.split_once(' ').expect("double-click C R");
                    Step::DoubleClick((c.parse().unwrap(), r.parse().unwrap()))
                }
                "wheel" => Step::Wheel(arg.parse().expect("wheel LINES")),
                "expect-selection" => Step::ExpectSelection(unescape(arg)),
                "expect-blink" => Step::ExpectBlink,
                "expect-screen" => Step::ExpectScreen(unescape(arg)),
                "expect-not-screen" => Step::ExpectNotScreen(unescape(arg)),
                "expect-panes" => Step::ExpectPanes(arg.parse().unwrap()),
                "expect-tabs" => Step::ExpectTabs(arg.parse().unwrap()),
                "report-idle" => Step::ReportIdleFrames(arg.parse().unwrap()),
                "expect-idle" => Step::ExpectIdle(if arg.is_empty() { None } else { Some(arg.parse().expect("expect-idle [DRAWS]")) }),
                "queue" => Step::Queue(unescape(arg)),
                "open-queue" => Step::OpenQueue,
                "prompt-type" => Step::Prompt(crate::PromptInput::Text(unescape(arg))),
                "prompt-paste" => Step::Prompt(crate::PromptInput::Paste(unescape(arg))),
                "prompt-key" => Step::Prompt(match arg {
                    "Enter" => crate::PromptInput::Enter,
                    "Escape" => crate::PromptInput::Escape,
                    "Backspace" => crate::PromptInput::Backspace,
                    "ClearQueue" => crate::PromptInput::ClearQueue,
                    _ => panic!("prompt-key Enter|Escape|Backspace|ClearQueue"),
                }),
                "expect-queued" => Step::ExpectQueued(arg.parse().expect("expect-queued N")),
                "theme" => Step::Theme(arg.into()),
                "reload-config" => Step::ReloadConfig,
                "menu" => Step::Menu(arg.into()),
                "click-chrome" => Step::ClickChrome(arg.into()),
                "open-settings" => Step::OpenSettings,
                "settings-key" => Step::SettingsKey(match arg {
                    "Up" => crate::SettingsInput::Up,
                    "Down" => crate::SettingsInput::Down,
                    "Left" => crate::SettingsInput::Left,
                    "Right" => crate::SettingsInput::Right,
                    "Enter" => crate::SettingsInput::Done,
                    "Escape" => crate::SettingsInput::Cancel,
                    _ => panic!("settings-key Up|Down|Left|Right|Enter|Escape"),
                }),
                "expect-prompt" => Step::ExpectPrompt(match arg {
                    "open" => true,
                    "closed" => false,
                    _ => panic!("expect-prompt open|closed"),
                }),
                "appearance" => Step::Appearance(match arg {
                    "dark" => true,
                    "light" => false,
                    _ => panic!("appearance light|dark"),
                }),
                "expect-theme" => Step::ExpectTheme(arg.into()),
                "write-config" | "write-theme" | "expect-config" => {
                    let (k, v) = arg.split_once(' ').unwrap_or_else(|| panic!("{cmd} NAME VALUE"));
                    let (k, v) = (k.to_string(), unescape(v));
                    match cmd {
                        "write-config" => Step::WriteConfig(k, v),
                        "write-theme" => Step::WriteTheme(k, v),
                        _ => Step::ExpectConfig(k, v),
                    }
                }
                "quit" => Step::Quit,
                _ => panic!("unknown script step: {line}"),
            }
        })
        .collect()
}

pub fn screen_text(frame: &Frame) -> String {
    let lines: Vec<String> = frame.rows.iter().map(|r| r.cells.iter().map(|c| if c.text.is_empty() && c.width != 0 { " " } else { c.text.as_str() }).collect::<String>().trim_end().to_string()).collect();
    lines.join("\n")
}

#[derive(Default)]
pub struct Report {
    lines: Vec<String>,
    pub idle_baseline: Option<(u64, Instant, u64)>,
    /// Window focus changes when the idle interval began.
    pub focus_baseline: u64,
}

impl Report {
    pub fn check(&mut self, what: &str, ok: bool, detail: &str) {
        let line = if ok { format!("ok    {what}") } else { format!("FAIL  {what}\n{detail}") };
        println!("{line}");
        self.lines.push(line);
    }

    pub fn print(&self) {
        let failed = self.lines.iter().filter(|l| l.starts_with("FAIL")).count();
        println!("{} checks, {failed} failed", self.lines.len());
    }
}
