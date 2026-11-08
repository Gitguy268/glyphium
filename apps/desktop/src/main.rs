//! Glyphium desktop: one window with tabs and splits, each pane a shell on a PTY.
//!
//! Parsing and presenting are separate: output is fed to the terminal as it arrives, in bounded
//! batches, and the screen is drawn at most once per display refresh and only if something
//! changed. Panes in hidden tabs keep parsing but draw nothing.

mod config;
mod layout;
mod queue;
mod script;

use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, TryRecvError};
use std::sync::Arc;
use std::time::{Duration, Instant};

use glyphium_session_host::{Dimensions, Profile, Session, SessionEvent};
use glyphium_terminal_core::input::{KeyAction, KeyInput, MouseAction, MouseButton, MouseInput, key_from_code, mods};
use glyphium_terminal_core::{CursorStyle, Frame, Rgb, Terminal, Theme, attrs};
use glyphium_terminal_render::{FontConfig, PaneRect, PaneView, Renderer};
use layout::{Direction, Node, PaneId, Rect};
use queue::CommandQueue;
use script::Step;
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalPosition, PhysicalSize};
use winit::event::{ElementState, Ime, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::keyboard::{Key, ModifiersState, NamedKey, PhysicalKey};
use winit::platform::modifier_supplement::KeyEventExtModifierSupplement;
use winit::window::{Window, WindowId};

/// Output parsed per pane before giving other events a turn.
const DRAIN_BUDGET: usize = 2 * 1024 * 1024;
const BLINK: Duration = Duration::from_millis(530);
const DOUBLE_CLICK: Duration = Duration::from_millis(400);
const SEPARATOR: Rgb = Rgb { r: 70, g: 70, b: 70 };
/// How long a short notice, such as the theme name, stays in the top bar.
const NOTICE: Duration = Duration::from_millis(1500);

struct Pane {
    term: Terminal,
    frame: Frame,
    view: PaneView,
    session: Session,
    events: Receiver<SessionEvent>,
    rect: Rect,
    cols: u16,
    rows: u16,
    exited: bool,
    /// Wheel movement not yet worth a whole row.
    scroll_accum: f32,
    /// Commands to type once the shell is back at its prompt.
    queue: CommandQueue,
}

/// What a key press does in the queue prompt.
#[derive(Debug, Clone, PartialEq)]
pub enum PromptInput {
    Text(String),
    Paste(String),
    Backspace,
    Enter,
    Escape,
    ClearQueue,
}

/// The "Queue next:" line in the top bar, for the pane it was opened in.
struct Prompt {
    pane: PaneId,
    text: String,
}

struct Tab {
    root: Node,
    focused: PaneId,
}

/// A one-row terminal used as the top bar: tab list or a question for the user.
struct Bar {
    term: Terminal,
    frame: Frame,
    view: PaneView,
    text: String,
    cols: u16,
}

struct Gpu {
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    device: wgpu::Device,
    renderer: Renderer,
    lost: Arc<AtomicBool>,
}

struct Drag {
    pane: PaneId,
    start: (u16, u16),
    moved: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Action {
    NewTab,
    ClosePane,
    NextTab,
    PrevTab,
    SelectTab(usize),
    SplitRight,
    SplitDown,
    Focus(Direction),
    Copy,
    Paste,
    ZoomIn,
    ZoomOut,
    ZoomReset,
    PageUp,
    PageDown,
    QueueCommand,
    ClearQueue,
    NextTheme,
    Quit,
}

struct App {
    proxy: EventLoopProxy<()>,
    wake_pending: Arc<AtomicBool>,
    window: Option<Arc<Window>>,
    gpu: Option<Gpu>,
    instance: Option<wgpu::Instance>,
    panes: HashMap<PaneId, Pane>,
    tabs: Vec<Tab>,
    active: usize,
    next_id: u64,
    bar: Option<Bar>,
    banner: Option<String>,
    pending_paste: Option<String>,
    prompt: Option<Prompt>,
    notice: Option<(String, Instant)>,
    /// Whether the panes were laid out below a top bar.
    bar_shown: bool,
    /// The session program is a shell whose prompt the queue can detect.
    queue_supported: bool,
    next_queue_poll: Instant,
    config: config::Config,
    theme: Option<String>,
    mods: ModifiersState,
    focused_window: bool,
    /// Window focus changes so far; each one redraws, which idle checks allow for.
    focus_changes: u64,
    blink_on: bool,
    next_blink: Instant,
    layout_dirty: bool,
    force_redraw: bool,
    font_px: f32,
    mouse_px: (f32, f32),
    pressed: u32,
    drag: Option<Drag>,
    last_click: Option<(Instant, PaneId, (u16, u16))>,
    preedit: String,
    clipboard: Option<arboard::Clipboard>,
    profile: Profile,
    title: String,
    script: Vec<Step>,
    script_at: Instant,
    report: Option<script::Report>,
    exit_code: i32,
}

fn pixel_rect(r: Rect) -> PaneRect {
    PaneRect { x: r.x, y: r.y, width: r.w, height: r.h }
}

impl App {
    fn new(proxy: EventLoopProxy<()>, profile: Profile, config: config::Config, script: Vec<Step>) -> Self {
        let queue_supported = profile.command.as_deref().is_none_or(queue::is_shell);
        Self {
            proxy,
            wake_pending: Arc::new(AtomicBool::new(false)),
            window: None,
            gpu: None,
            instance: None,
            panes: HashMap::new(),
            tabs: vec![],
            active: 0,
            next_id: 1,
            bar: None,
            banner: None,
            pending_paste: None,
            prompt: None,
            notice: None,
            bar_shown: false,
            queue_supported,
            next_queue_poll: Instant::now(),
            theme: config.theme.clone(),
            config,
            mods: ModifiersState::empty(),
            focused_window: true,
            focus_changes: 0,
            blink_on: true,
            next_blink: Instant::now() + BLINK,
            layout_dirty: true,
            force_redraw: true,
            font_px: 0.0,
            mouse_px: (0.0, 0.0),
            pressed: 0,
            drag: None,
            last_click: None,
            preedit: String::new(),
            clipboard: arboard::Clipboard::new().ok(),
            profile,
            title: String::new(),
            script,
            script_at: Instant::now(),
            report: None,
            exit_code: 0,
        }
    }

    fn window(&self) -> &Arc<Window> {
        self.window.as_ref().expect("window exists after resumed")
    }

    fn request_redraw(&self) {
        if let Some(w) = &self.window {
            w.request_redraw();
        }
    }

    fn create_gpu(&mut self) {
        let window = self.window().clone();
        let instance = self.instance.get_or_insert_with(|| wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()));
        let surface = instance.create_surface(window.clone()).expect("create surface");
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            compatible_surface: Some(&surface),
            ..Default::default()
        }))
        .expect("no GPU adapter that can draw to this window");
        let (device, queue) = pollster::block_on(adapter.request_device(&Default::default())).expect("GPU device");
        let lost = Arc::new(AtomicBool::new(false));
        {
            let lost = lost.clone();
            let proxy = self.proxy.clone();
            device.set_device_lost_callback(move |_, _| {
                lost.store(true, Ordering::SeqCst);
                let _ = proxy.send_event(());
            });
        }
        let size = window.inner_size();
        let mut config = surface.get_default_config(&adapter, size.width.max(1), size.height.max(1)).expect("surface config");
        // Non-sRGB, so colour values reach the screen as the terminal gave them, as on the web.
        let caps = surface.get_capabilities(&adapter);
        config.format = caps.formats.iter().copied().find(|f| !f.is_srgb()).unwrap_or(config.format.remove_srgb_suffix());
        surface.configure(&device, &config);
        if self.font_px == 0.0 {
            self.font_px = (self.config.font_pt * window.scale_factor() as f32).round();
        }
        let fonts = FontConfig { size_px: self.font_px, system_fonts: true, ..Default::default() };
        let renderer = Renderer::new(device.clone(), queue, config.format, &fonts);
        self.gpu = Some(Gpu { surface, config, device, renderer, lost });
    }

    /// New device after a loss: views rebuild from frames, the shells never notice.
    fn recover_gpu(&mut self) {
        eprintln!("glyphium: GPU device lost, recreating");
        self.gpu = None;
        self.create_gpu();
        let renderer = &self.gpu.as_ref().unwrap().renderer;
        for pane in self.panes.values_mut() {
            pane.view = renderer.new_view();
        }
        if let Some(bar) = &mut self.bar {
            bar.view = renderer.new_view();
        }
        self.force_redraw = true;
        self.layout_dirty = true;
        self.request_redraw();
    }

    fn open_pane(&mut self) -> PaneId {
        let id = self.next_id;
        self.next_id += 1;
        let gpu = self.gpu.as_ref().unwrap();
        let (proxy, pending) = (self.proxy.clone(), self.wake_pending.clone());
        let wake = move || {
            if !pending.swap(true, Ordering::SeqCst) {
                let _ = proxy.send_event(());
            }
        };
        // The real size comes from the next layout pass, before any shell output is drawn.
        let dims = Dimensions { cols: 80, rows: 24, pixel_width: 0, pixel_height: 0 };
        let (session, events) = Session::open(&self.profile, dims, id, wake).expect("start shell");
        let mut term = Terminal::new(dims.cols, dims.rows, self.config.scrollback);
        term.set_theme(self.theme.as_deref().and_then(Theme::by_name).as_ref());
        let pane = Pane {
            term,
            frame: Frame::default(),
            view: gpu.renderer.new_view(),
            session,
            events,
            rect: Rect { x: 0, y: 0, w: 0, h: 0 },
            cols: dims.cols,
            rows: dims.rows,
            exited: false,
            scroll_accum: 0.0,
            queue: CommandQueue::default(),
        };
        self.panes.insert(id, pane);
        self.layout_dirty = true;
        id
    }

    fn new_tab(&mut self) {
        let id = self.open_pane();
        self.tabs.push(Tab { root: Node::Leaf(id), focused: id });
        self.active = self.tabs.len() - 1;
        self.tab_changed();
    }

    fn tab_changed(&mut self) {
        for id in self.tabs[self.active].root.panes() {
            if let Some(p) = self.panes.get_mut(&id) {
                p.view.invalidate();
            }
        }
        self.layout_dirty = true;
        self.force_redraw = true;
        self.request_redraw();
    }

    fn split(&mut self, side_by_side: bool) {
        let new = self.open_pane();
        let tab = &mut self.tabs[self.active];
        tab.root.split(tab.focused, new, side_by_side);
        tab.focused = new;
        self.force_redraw = true;
        self.request_redraw();
    }

    fn close_pane(&mut self, id: PaneId) {
        self.panes.remove(&id);
        if self.prompt.as_ref().is_some_and(|p| p.pane == id) {
            self.prompt = None;
        }
        let Some(index) = self.tabs.iter().position(|t| t.root.panes().contains(&id)) else { return };
        let tab = &mut self.tabs[index];
        match tab.root.clone().remove(id) {
            Some(root) => {
                tab.root = root;
                if tab.focused == id {
                    tab.focused = tab.root.panes()[0];
                }
            }
            None => {
                self.tabs.remove(index);
                if self.active >= index && self.active > 0 {
                    self.active -= 1;
                }
            }
        }
        if !self.tabs.is_empty() {
            self.tab_changed();
        }
    }

    fn focused_pane(&mut self) -> Option<&mut Pane> {
        let id = self.tabs.get(self.active)?.focused;
        self.panes.get_mut(&id)
    }

    /// The window area left for panes, below the top bar when one shows.
    fn content_area(&self) -> Rect {
        let size = self.window().inner_size();
        let bar = if self.bar_content().is_some() { self.gpu.as_ref().unwrap().renderer.metrics().height } else { 0 };
        Rect { x: 0, y: bar, w: size.width, h: size.height.saturating_sub(bar) }
    }

    /// Applies the current window and split sizes to terminals and PTYs. Runs once per redraw,
    /// so a storm of resize events becomes one resize with the final size.
    fn apply_layout(&mut self) {
        self.layout_dirty = false;
        self.bar_shown = self.bar_content().is_some();
        let area = self.content_area();
        let m = self.gpu.as_ref().unwrap().renderer.metrics();
        for tab in &self.tabs {
            for (id, rect) in tab.root.rects(area) {
                let Some(pane) = self.panes.get_mut(&id) else { continue };
                pane.rect = rect;
                let cols = (rect.w / m.width).clamp(1, u16::MAX as u32) as u16;
                let rows = (rect.h / m.height).clamp(1, u16::MAX as u32) as u16;
                if (cols, rows) != (pane.cols, pane.rows) {
                    pane.cols = cols;
                    pane.rows = rows;
                    pane.term.resize(cols, rows, m.width, m.height);
                    let dims = Dimensions { cols, rows, pixel_width: (cols as u32 * m.width).min(65535) as u16, pixel_height: (rows as u32 * m.height).min(65535) as u16 };
                    let _ = pane.session.resize(dims);
                }
            }
        }
        self.force_redraw = true;
    }

    /// Feeds waiting output to the terminals; true if a visible pane changed.
    fn drain_output(&mut self) -> bool {
        let visible = self.tabs.get(self.active).map(|t| t.root.panes()).unwrap_or_default();
        let mut changed = false;
        let mut more = false;
        let mut ended = vec![];
        for (&id, pane) in self.panes.iter_mut() {
            let mut budget = DRAIN_BUDGET;
            loop {
                match pane.events.try_recv() {
                    Ok(SessionEvent::Output { generation, bytes }) if generation == id => {
                        budget = budget.saturating_sub(bytes.len());
                        pane.queue.saw_output(Instant::now());
                        pane.term.write(&bytes);
                        pane.session.write(pane.term.take_replies());
                        changed |= visible.contains(&id);
                        if budget == 0 {
                            more = true;
                            break;
                        }
                    }
                    Ok(SessionEvent::Exit { .. }) | Err(TryRecvError::Disconnected) => {
                        pane.exited = true;
                        ended.push(id);
                        break;
                    }
                    Ok(_) => {} // Stale generation: it cannot belong to this pane's session.
                    Err(TryRecvError::Empty) => break,
                }
            }
        }
        for id in ended {
            self.close_pane(id);
        }
        changed |= self.poll_queues();
        if more {
            self.wake_pending.store(true, Ordering::SeqCst);
            let _ = self.proxy.send_event(());
        }
        changed
    }

    /// What the top bar should show, in escape-coded text, or `None` for no bar. The paste
    /// question wins, then the queue prompt, then a notice, then tabs and the queue.
    fn bar_content(&self) -> Option<String> {
        if let Some(b) = &self.banner {
            return Some(format!("\x1b[43;30m\x1b[K {b}\x1b[0m"));
        }
        if let Some(p) = &self.prompt {
            let waiting = self.panes.get(&p.pane).map_or(0, |pane| pane.queue.len());
            let label = if waiting > 0 { format!("Queue next ({waiting} waiting):") } else { "Queue next:".to_string() };
            // Keep the end of a long command in view, where the typing happens.
            let room = (self.bar.as_ref().map_or(80, |b| b.cols as usize)).saturating_sub(label.chars().count() + 4).max(8);
            let n = p.text.chars().count();
            let shown: String = p.text.chars().skip(n.saturating_sub(room)).collect();
            return Some(format!("\x1b[48;5;24;97m\x1b[K {label} {shown}\x1b[7m \x1b[0m"));
        }
        if let Some((text, _)) = &self.notice {
            return Some(format!("\x1b[48;5;238;97m\x1b[K {text}\x1b[0m"));
        }
        let mut s = String::new();
        if self.tabs.len() > 1 {
            s.push_str("\x1b[48;5;236m\x1b[K");
            for (i, tab) in self.tabs.iter().enumerate() {
                let title = self.panes.get(&tab.focused).map(|p| p.term.title()).unwrap_or_default();
                let short: String = title.chars().take(24).collect();
                let queued: usize = tab.root.panes().iter().filter_map(|id| self.panes.get(id)).map(|p| p.queue.len()).sum();
                let mark = if queued > 0 { format!(" ⏳{queued}") } else { String::new() };
                let label = if short.is_empty() { format!(" {}{mark} ", i + 1) } else { format!(" {} {short}{mark} ", i + 1) };
                if i == self.active {
                    s.push_str(&format!("\x1b[48;5;250;30m{label}\x1b[48;5;236;39m"));
                } else {
                    s.push_str(&label);
                }
            }
        }
        let focused = self.tabs.get(self.active).and_then(|t| self.panes.get(&t.focused));
        if let Some(pane) = focused.filter(|p| p.queue.len() > 0) {
            let list: Vec<&str> = pane.queue.items().collect();
            if s.is_empty() {
                s.push_str("\x1b[48;5;24;97m\x1b[K");
            } else {
                s.push_str("  \x1b[48;5;24;97m");
            }
            s.push_str(&format!(" Queued ({}): {} ", list.len(), list.join(" → ")));
        }
        if s.is_empty() {
            return None;
        }
        s.push_str("\x1b[0m");
        Some(s)
    }

    fn update_bar(&mut self) -> bool {
        let m = self.gpu.as_ref().unwrap().renderer.metrics();
        let cols = (self.window().inner_size().width / m.width).max(1) as u16;
        let Some(text) = self.bar_content() else {
            self.bar = None;
            return false;
        };
        let renderer = &self.gpu.as_ref().unwrap().renderer;
        let bar = self.bar.get_or_insert_with(|| Bar { term: Terminal::new(cols, 1, 0), frame: Frame::default(), view: renderer.new_view(), text: String::new(), cols });
        if bar.cols != cols {
            bar.cols = cols;
            bar.term.resize(cols, 1, m.width, m.height);
            bar.text.clear();
        }
        if bar.text != text {
            // No autowrap: text longer than the bar is cut off instead of scrolling it away.
            bar.term.write(b"\x1b[?25l\x1b[?7l\x1b[2J\x1b[H");
            bar.term.write(text.as_bytes());
            bar.text = text;
        }
        true
    }

    /// Types the next queued command in every pane whose shell is ready for it, hidden tabs
    /// included. True if what the bar shows may have changed.
    fn poll_queues(&mut self) -> bool {
        let now = Instant::now();
        let mut changed = false;
        for pane in self.panes.values_mut() {
            if !pane.queue.active() {
                continue;
            }
            if let Some(bytes) = pane.queue.poll(pane.session.shell_idle(), now) {
                pane.term.scroll_to_bottom();
                pane.session.write(bytes);
                changed = true;
            }
        }
        if changed {
            self.request_redraw();
        }
        changed
    }

    /// Adds a command to a pane's queue; it runs at once if the shell is at its prompt.
    fn queue_command(&mut self, id: PaneId, text: &str) {
        let command: String = text.chars().filter(|c| !c.is_control()).collect();
        let command = command.trim();
        if command.is_empty() {
            return;
        }
        if !self.queue_supported {
            self.show_notice("The queue needs a shell: this pane runs another program.".into());
            return;
        }
        let Some(pane) = self.panes.get_mut(&id) else { return };
        pane.queue.push(command.to_string());
        self.poll_queues();
        self.next_queue_poll = Instant::now() + queue::POLL;
        self.request_redraw();
    }

    fn show_notice(&mut self, text: String) {
        self.notice = Some((text, Instant::now() + NOTICE));
        self.request_redraw();
    }

    fn set_theme(&mut self, name: Option<String>) {
        let theme = name.as_deref().and_then(Theme::by_name);
        for pane in self.panes.values_mut() {
            pane.term.set_theme(theme.as_ref());
        }
        self.show_notice(format!("Theme: {}", name.as_deref().unwrap_or("default")));
        self.theme = name;
        self.force_redraw = true;
    }

    /// A key while the queue prompt is open. Everything goes to the prompt, never the shell.
    fn prompt_key(&mut self, event: &winit::event::KeyEvent, code: &str) {
        let input = match self.shortcut(code, self.mods) {
            Some(Action::Paste) => self.clipboard.as_mut().and_then(|c| c.get_text().ok()).map(PromptInput::Paste),
            Some(Action::ClearQueue) => Some(PromptInput::ClearQueue),
            Some(_) => None,
            None => match &event.logical_key {
                Key::Named(NamedKey::Escape) => Some(PromptInput::Escape),
                Key::Named(NamedKey::Enter) => Some(PromptInput::Enter),
                Key::Named(NamedKey::Backspace) => Some(PromptInput::Backspace),
                _ if self.mods.control_key() || self.mods.super_key() => None,
                _ => event.text_with_all_modifiers().map(|t| PromptInput::Text(t.to_string())),
            },
        };
        if let Some(input) = input {
            self.prompt_input(input);
        }
    }

    fn prompt_input(&mut self, input: PromptInput) {
        let Some(p) = &mut self.prompt else { return };
        match input {
            PromptInput::Text(text) => p.text.extend(text.chars().filter(|c| !c.is_control())),
            // One line: a pasted newline must not become a second command or an early Enter.
            PromptInput::Paste(text) => p.text.extend(text.chars().map(|c| if matches!(c, '\n' | '\r' | '\t') { ' ' } else { c }).filter(|c| !c.is_control())),
            PromptInput::Escape => self.prompt = None,
            PromptInput::Enter => {
                let p = self.prompt.take().unwrap();
                self.queue_command(p.pane, &p.text);
            }
            PromptInput::Backspace => {
                if p.text.pop().is_none() {
                    // Backspace on an empty line takes back the last queued command.
                    if let Some(pane) = self.panes.get_mut(&p.pane) {
                        pane.queue.pop_last();
                    }
                }
            }
            PromptInput::ClearQueue => {
                if let Some(pane) = self.panes.get_mut(&p.pane) {
                    pane.queue.clear();
                }
            }
        }
        self.request_redraw();
    }

    fn sync_title(&mut self) {
        let title = self.focused_pane().map(|p| p.term.title()).unwrap_or_default();
        let title = if title.is_empty() { "Glyphium".to_string() } else { title };
        if title != self.title {
            self.window().set_title(&title);
            self.title = title;
        }
    }

    /// Updates cursor look and preedit text on top of the terminal's own frame.
    fn decorate(frame: &mut Frame, focused: bool, blink_on: bool, preedit: &str) -> Option<usize> {
        if !focused {
            frame.cursor.style = CursorStyle::BlockHollow;
        } else if frame.cursor.blinking && !blink_on {
            frame.cursor.visible = false;
        }
        if !focused || preedit.is_empty() {
            return None;
        }
        // Composition text sits over the cells at the cursor until it is committed.
        let (x, y) = frame.cursor.pos?;
        let fg = frame.foreground;
        let row = frame.rows.get_mut(y as usize)?;
        let mut col = x as usize;
        for ch in preedit.chars() {
            let wide = ch as u32 >= 0x1100;
            if col >= row.cells.len() {
                break;
            }
            row.cells[col].text = ch.to_string();
            row.cells[col].width = if wide { 2 } else { 1 };
            row.cells[col].fg = fg;
            row.cells[col].attrs = attrs::UNDERLINE;
            if wide && col + 1 < row.cells.len() {
                row.cells[col + 1].text.clear();
                row.cells[col + 1].width = 0;
            }
            col += if wide { 2 } else { 1 };
        }
        row.dirty = true;
        Some(y as usize)
    }

    fn sync_panes(&mut self) -> Vec<PaneId> {
        let tab = &self.tabs[self.active];
        let (visible, focused) = (tab.root.panes(), tab.focused);
        let gpu = self.gpu.as_mut().unwrap();
        for id in &visible {
            let Some(pane) = self.panes.get_mut(id) else { continue };
            pane.term.update_frame(&mut pane.frame);
            let is_focused = *id == focused && self.focused_window;
            let preedit_row = Self::decorate(&mut pane.frame, is_focused, self.blink_on, &self.preedit);
            let changed = gpu.renderer.update(&mut pane.view, &pane.frame);
            self.force_redraw |= changed;
            if let Some(y) = preedit_row {
                // Those cells are not the terminal's: rebuild the row from real state next time.
                pane.term.mark_row_dirty(y);
            }
        }
        visible
    }

    fn redraw(&mut self) {
        if self.tabs.is_empty() || self.gpu.is_none() {
            return;
        }
        // The device-lost callback only runs when the device is polled, so ask before drawing.
        let _ = self.gpu.as_ref().unwrap().device.poll(wgpu::PollType::Poll);
        if self.gpu.as_ref().unwrap().lost.load(Ordering::SeqCst) {
            self.recover_gpu();
        }
        if self.bar_content().is_some() != self.bar_shown {
            self.layout_dirty = true;
        }
        if self.layout_dirty {
            self.apply_layout();
        }
        self.sync_title();
        let has_bar = self.update_bar();
        if has_bar {
            let bar = self.bar.as_mut().unwrap();
            bar.term.update_frame(&mut bar.frame);
            let gpu = self.gpu.as_mut().unwrap();
            self.force_redraw |= gpu.renderer.update(&mut bar.view, &bar.frame);
        }
        let visible = self.sync_panes();
        if !std::mem::take(&mut self.force_redraw) {
            return; // Nothing changed: no draw, no present.
        }
        let size = self.window().inner_size();
        let bar_height = self.gpu.as_ref().unwrap().renderer.metrics().height;
        let panes = draw_list(&self.panes, self.bar.as_ref().filter(|_| has_bar), bar_height, &visible, size.width);
        let gpu = self.gpu.as_mut().unwrap();
        let texture = match gpu.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(t) | wgpu::CurrentSurfaceTexture::Suboptimal(t) => t,
            wgpu::CurrentSurfaceTexture::Occluded | wgpu::CurrentSurfaceTexture::Timeout => {
                self.force_redraw = true;
                return;
            }
            _ => {
                gpu.surface.configure(&gpu.device, &gpu.config);
                self.force_redraw = true;
                self.window.as_ref().unwrap().request_redraw();
                return;
            }
        };
        let target = texture.texture.create_view(&Default::default());
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        gpu.renderer.draw(&mut encoder, &target, (size.width, size.height), SEPARATOR, &panes);
        gpu.renderer.queue().submit([encoder.finish()]);
        self.window.as_ref().unwrap().pre_present_notify();
        gpu.renderer.queue().present(texture);
    }

    fn screenshot(&mut self, path: &Path) {
        self.force_redraw = true;
        self.redraw();
        let size = self.window().inner_size();
        let visible = self.tabs[self.active].root.panes();
        let bar_height = self.gpu.as_ref().unwrap().renderer.metrics().height;
        let panes = draw_list(&self.panes, self.bar.as_ref(), bar_height, &visible, size.width);
        let rgba = pollster::block_on(self.gpu.as_mut().unwrap().renderer.render_panes_to_rgba(SEPARATOR, (size.width, size.height), &panes));
        let file = std::io::BufWriter::new(std::fs::File::create(path).expect("create screenshot"));
        let mut enc = png::Encoder::new(file, size.width, size.height);
        enc.set_color(png::ColorType::Rgba);
        enc.write_header().unwrap().write_image_data(&rgba).unwrap();
    }

    fn shortcut(&self, code: &str, m: ModifiersState) -> Option<Action> {
        // Cmd on macOS; Ctrl+Shift elsewhere, since bare Ctrl belongs to the shell.
        let primary = if cfg!(target_os = "macos") { m.super_key() } else { m.control_key() && m.shift_key() };
        if m.shift_key() && !m.control_key() && !m.super_key() {
            return match code {
                "PageUp" => Some(Action::PageUp),
                "PageDown" => Some(Action::PageDown),
                _ => None,
            };
        }
        if !primary {
            return None;
        }
        if m.alt_key() {
            if code == "KeyT" {
                return Some(Action::NextTheme);
            }
            return Some(Action::Focus(match code {
                "ArrowLeft" => Direction::Left,
                "ArrowRight" => Direction::Right,
                "ArrowUp" => Direction::Up,
                "ArrowDown" => Direction::Down,
                _ => return None,
            }));
        }
        let mac_shift = cfg!(target_os = "macos") && m.shift_key();
        Some(match code {
            "KeyT" if mac_shift => Action::NextTheme,
            "Enter" | "NumpadEnter" if m.shift_key() => Action::QueueCommand,
            "Backspace" if m.shift_key() => Action::ClearQueue,
            "KeyT" => Action::NewTab,
            "KeyW" => Action::ClosePane,
            "KeyD" if mac_shift => Action::SplitDown,
            "KeyD" => Action::SplitRight,
            "KeyE" => Action::SplitDown,
            "BracketRight" => Action::NextTab,
            "BracketLeft" => Action::PrevTab,
            "KeyC" => Action::Copy,
            "KeyV" => Action::Paste,
            "Equal" => Action::ZoomIn,
            "Minus" => Action::ZoomOut,
            "Digit0" => Action::ZoomReset,
            "KeyQ" => Action::Quit,
            d if d.starts_with("Digit") => Action::SelectTab(d[5..].parse::<usize>().ok()?.checked_sub(1)?),
            _ => return None,
        })
    }

    fn run_action(&mut self, action: Action, event_loop: &ActiveEventLoop) {
        match action {
            Action::NewTab => self.new_tab(),
            Action::ClosePane => {
                if let Some(id) = self.tabs.get(self.active).map(|t| t.focused) {
                    self.close_pane(id);
                }
            }
            Action::NextTab | Action::PrevTab => {
                let n = self.tabs.len();
                self.active = if action == Action::NextTab { (self.active + 1) % n } else { (self.active + n - 1) % n };
                self.tab_changed();
            }
            Action::SelectTab(i) if i < self.tabs.len() => {
                self.active = i;
                self.tab_changed();
            }
            Action::SelectTab(_) => {}
            Action::SplitRight => self.split(true),
            Action::SplitDown => self.split(false),
            Action::Focus(dir) => {
                let area = self.content_area();
                let tab = &mut self.tabs[self.active];
                if let Some(next) = layout::neighbour(&tab.root.rects(area), tab.focused, dir) {
                    tab.focused = next;
                    self.force_redraw = true;
                    self.request_redraw();
                }
            }
            Action::Copy => {
                let text = self.focused_pane().and_then(|p| p.term.selection_text());
                if let (Some(text), Some(cb)) = (text, &mut self.clipboard) {
                    let _ = cb.set_text(text);
                }
            }
            Action::Paste => {
                let text = self.clipboard.as_mut().and_then(|c| c.get_text().ok());
                if let Some(text) = text {
                    self.paste(text);
                }
            }
            Action::ZoomIn => self.set_font(self.font_px + 2.0),
            Action::ZoomOut => self.set_font((self.font_px - 2.0).max(6.0)),
            Action::ZoomReset => self.set_font((self.config.font_pt * self.window().scale_factor() as f32).round()),
            Action::PageUp | Action::PageDown => {
                if let Some(p) = self.focused_pane() {
                    let page = p.rows as isize - 1;
                    p.term.scroll_viewport(if action == Action::PageUp { -page } else { page });
                }
                self.request_redraw();
            }
            Action::QueueCommand => {
                if let Some(id) = self.tabs.get(self.active).map(|t| t.focused) {
                    self.clear_banner();
                    self.preedit.clear();
                    self.prompt = Some(Prompt { pane: id, text: String::new() });
                    self.request_redraw();
                }
            }
            Action::ClearQueue => {
                if let Some(p) = self.focused_pane() {
                    p.queue.clear();
                }
                self.request_redraw();
            }
            Action::NextTheme => {
                let next = config::next_theme(self.theme.as_deref());
                self.set_theme(next);
            }
            Action::Quit => {
                self.panes.clear();
                self.tabs.clear();
                event_loop.exit();
            }
        }
    }

    fn set_font(&mut self, px: f32) {
        self.font_px = px;
        self.gpu.as_mut().unwrap().renderer.set_font_size(px);
        self.layout_dirty = true;
        self.force_redraw = true;
        self.request_redraw();
    }

    /// Multi-line text into a program without bracketed paste can run commands, so that case
    /// asks first: a second paste confirms, any other key cancels.
    fn paste(&mut self, text: String) {
        let Some(pane) = self.focused_pane() else { return };
        if pane.term.paste_needs_confirmation(&text) {
            let confirmed = self.banner.is_some() && self.pending_paste.as_deref() == Some(text.as_str());
            if !confirmed {
                let lines = text.lines().count();
                self.pending_paste = Some(text);
                self.banner = Some(format!("Paste {lines} line(s) that may run as commands? Paste again to confirm, any other key cancels."));
                self.layout_dirty = true;
                self.request_redraw();
                return;
            }
            self.clear_banner();
        }
        let Some(pane) = self.focused_pane() else { return };
        pane.term.scroll_to_bottom();
        let bytes = pane.term.encode_paste(&text);
        pane.session.write(bytes);
        self.request_redraw();
    }

    fn clear_banner(&mut self) {
        if self.banner.take().is_some() {
            self.pending_paste = None;
            self.layout_dirty = true;
            self.request_redraw();
        }
    }

    fn key_mods(m: ModifiersState) -> u16 {
        let mut out = 0;
        for (on, bit) in [(m.shift_key(), mods::SHIFT), (m.control_key(), mods::CTRL), (m.alt_key(), mods::ALT), (m.super_key(), mods::SUPER)] {
            if on {
                out |= bit;
            }
        }
        out
    }

    /// A key on its way to the program in the focused pane.
    fn send_key(&mut self, input: &KeyInput) {
        let Some(pane) = self.focused_pane() else { return };
        let bytes = pane.term.encode_key(input);
        if !bytes.is_empty() {
            pane.term.clear_selection();
            pane.term.scroll_to_bottom();
            pane.session.write(bytes);
            self.blink_on = true;
            self.next_blink = Instant::now() + BLINK;
            self.request_redraw();
        }
    }

    fn pane_at(&self, x: f32, y: f32) -> Option<(PaneId, (u16, u16))> {
        let m = self.gpu.as_ref()?.renderer.metrics();
        for id in self.tabs.get(self.active)?.root.panes() {
            let p = self.panes.get(&id)?;
            let r = p.rect;
            if x >= r.x as f32 && y >= r.y as f32 && x < (r.x + r.w) as f32 && y < (r.y + r.h) as f32 {
                let col = ((x - r.x as f32) as u32 / m.width).min(p.cols as u32 - 1) as u16;
                let row = ((y - r.y as f32) as u32 / m.height).min(p.rows as u32 - 1) as u16;
                return Some((id, (col, row)));
            }
        }
        None
    }

    /// Sends a mouse event to the program if it wants one. Shift always keeps the mouse for
    /// selection, the usual escape hatch from full-screen programs.
    fn report_mouse(&mut self, id: PaneId, action: MouseAction, button: Option<MouseButton>) -> bool {
        let m = self.gpu.as_ref().unwrap().renderer.metrics();
        let any = self.pressed != 0;
        let (px, modifiers) = (self.mouse_px, Self::key_mods(self.mods));
        let shift = self.mods.shift_key();
        let Some(pane) = self.panes.get_mut(&id) else { return false };
        if shift || !pane.term.mouse_tracking() {
            return false;
        }
        let input = MouseInput { action, button, mods: modifiers, x: px.0 - pane.rect.x as f32, y: px.1 - pane.rect.y as f32, any_button_pressed: any };
        let bytes = pane.term.encode_mouse(&input, (pane.rect.w, pane.rect.h), (m.width, m.height));
        pane.session.write(bytes);
        true
    }

    fn mouse_button(&mut self, state: ElementState, button: MouseButton) {
        let (x, y) = self.mouse_px;
        let hit = self.pane_at(x, y);
        match state {
            ElementState::Pressed => {
                self.pressed |= 1 << button;
                let Some((id, cell)) = hit else { return };
                self.tabs[self.active].focused = id;
                self.force_redraw = true;
                self.request_redraw();
                if self.report_mouse(id, MouseAction::Press, Some(button)) || button != 1 {
                    return;
                }
                if (self.mods.super_key() || (self.mods.control_key() && !cfg!(target_os = "macos")))
                    && let Some(uri) = self.panes[&id].term.hyperlink_at(cell)
                {
                    open_link(&uri);
                    return;
                }
                let now = Instant::now();
                let double = self.last_click.is_some_and(|(t, p, c)| now - t < DOUBLE_CLICK && p == id && c == cell);
                self.last_click = Some((now, id, cell));
                let pane = self.panes.get_mut(&id).unwrap();
                pane.term.clear_selection();
                if double {
                    pane.term.select_word(cell);
                } else {
                    self.drag = Some(Drag { pane: id, start: cell, moved: false });
                }
            }
            ElementState::Released => {
                self.pressed &= !(1 << button);
                self.drag = None;
                if let Some((id, _)) = hit {
                    self.report_mouse(id, MouseAction::Release, Some(button));
                }
            }
        }
    }

    fn mouse_moved(&mut self, pos: PhysicalPosition<f64>) {
        self.mouse_px = (pos.x as f32, pos.y as f32);
        if let Some(drag) = &mut self.drag {
            let pane_id = drag.pane;
            let start = drag.start;
            if let Some((id, cell)) = self.pane_at(pos.x as f32, pos.y as f32).filter(|(id, _)| *id == pane_id)
                && (cell != start || self.drag.as_ref().unwrap().moved)
            {
                self.drag.as_mut().unwrap().moved = true;
                self.panes.get_mut(&id).unwrap().term.select(start, cell);
                self.request_redraw();
            }
            return;
        }
        if let Some((id, _)) = self.pane_at(pos.x as f32, pos.y as f32) {
            let button = (1..=3).find(|b| self.pressed & (1 << b) != 0);
            self.report_mouse(id, MouseAction::Motion, button);
        }
    }

    fn wheel(&mut self, delta: MouseScrollDelta) {
        let (x, y) = self.mouse_px;
        let Some((id, _)) = self.pane_at(x, y) else { return };
        let m = self.gpu.as_ref().unwrap().renderer.metrics();
        let rows = match delta {
            MouseScrollDelta::LineDelta(_, l) => l,
            MouseScrollDelta::PixelDelta(p) => p.y as f32 / m.height as f32,
        };
        let pane = self.panes.get_mut(&id).unwrap();
        pane.scroll_accum += rows;
        let whole = pane.scroll_accum.trunc();
        pane.scroll_accum -= whole;
        if whole == 0.0 {
            return;
        }
        if pane.term.mouse_tracking() && !self.mods.shift_key() {
            let button = if whole > 0.0 { 4 } else { 5 };
            for _ in 0..whole.abs() as u32 {
                self.report_mouse(id, MouseAction::Press, Some(button));
            }
        } else {
            pane.term.scroll_viewport(-(whole as isize));
            self.request_redraw();
        }
    }

    fn set_ime_area(&self) {
        let (Some(window), Some(gpu)) = (&self.window, &self.gpu) else { return };
        let Some(pane) = self.tabs.get(self.active).and_then(|t| self.panes.get(&t.focused)) else { return };
        let Some((x, y)) = pane.frame.cursor.pos else { return };
        let m = gpu.renderer.metrics();
        window.set_ime_cursor_area(PhysicalPosition::new(pane.rect.x + x as u32 * m.width, pane.rect.y + y as u32 * m.height), PhysicalSize::new(m.width, m.height));
    }

    /// Middle of a cell in the focused pane, for scripted mouse input.
    fn cell_px(&self, (col, row): (u16, u16)) -> PhysicalPosition<f64> {
        let m = self.gpu.as_ref().unwrap().renderer.metrics();
        let r = self.tabs.get(self.active).and_then(|t| self.panes.get(&t.focused)).map_or(Rect { x: 0, y: 0, w: 0, h: 0 }, |p| p.rect);
        PhysicalPosition::new((r.x + col as u32 * m.width + m.width / 2) as f64, (r.y + row as u32 * m.height + m.height / 2) as f64)
    }

    fn check_screen(&mut self, text: &str, wanted: bool) {
        let screen = self.focused_pane().map(|p| {
            p.term.update_frame(&mut p.frame);
            script::screen_text(&p.frame)
        });
        let screen = screen.unwrap_or_default();
        let ok = screen.contains(text) == wanted;
        let what = if wanted { "contains" } else { "lacks" };
        self.report.get_or_insert_with(Default::default).check(&format!("screen {what} {text:?}"), ok, &screen);
        if !ok {
            self.exit_code = 1;
        }
    }

    fn run_script(&mut self, event_loop: &ActiveEventLoop) {
        while !self.script.is_empty() && Instant::now() >= self.script_at {
            let step = self.script.remove(0);
            match step {
                Step::Wait(ms) => self.script_at = Instant::now() + Duration::from_millis(ms),
                Step::Type(text) => {
                    for ch in text.chars() {
                        let s = ch.to_string();
                        self.send_key(&KeyInput { action: KeyAction::Press, key: 0, mods: 0, consumed_mods: 0, text: Some(&s), unshifted: 0 });
                    }
                }
                Step::Key(code) => {
                    let key = key_from_code(&code).unwrap_or_else(|| panic!("unknown key {code}"));
                    self.send_key(&KeyInput { action: KeyAction::Press, key, mods: 0, consumed_mods: 0, text: None, unshifted: 0 });
                }
                Step::Paste(text) => self.paste(text),
                Step::NewTab => self.run_action(Action::NewTab, event_loop),
                Step::SplitRight => self.run_action(Action::SplitRight, event_loop),
                Step::SplitDown => self.run_action(Action::SplitDown, event_loop),
                Step::NextTab => self.run_action(Action::NextTab, event_loop),
                Step::ClosePane => self.run_action(Action::ClosePane, event_loop),
                Step::Zoom(px) => self.set_font(self.font_px + px),
                Step::Resize(w, h) => {
                    let _ = self.window().request_inner_size(PhysicalSize::new(w, h));
                }
                Step::LoseDevice => {
                    self.gpu.as_ref().unwrap().device.destroy();
                    self.force_redraw = true;
                    self.request_redraw();
                }
                Step::Screenshot(path) => self.screenshot(Path::new(&path)),
                Step::Drag(from, to) => {
                    self.mouse_moved(self.cell_px(from));
                    self.mouse_button(ElementState::Pressed, 1);
                    self.mouse_moved(self.cell_px(to));
                    self.mouse_button(ElementState::Released, 1);
                }
                Step::DoubleClick(cell) => {
                    self.mouse_moved(self.cell_px(cell));
                    for _ in 0..2 {
                        self.mouse_button(ElementState::Pressed, 1);
                        self.mouse_button(ElementState::Released, 1);
                    }
                }
                Step::Wheel(lines) => {
                    self.mouse_moved(self.cell_px((1, 1)));
                    self.wheel(MouseScrollDelta::LineDelta(0.0, lines));
                }
                Step::ExpectSelection(text) => {
                    let got = self.focused_pane().and_then(|p| p.term.selection_text()).unwrap_or_default();
                    self.report.get_or_insert_with(Default::default).check(&format!("selection is {text:?}"), got == text, &format!("got {got:?}"));
                    if got != text {
                        self.exit_code = 1;
                    }
                }
                Step::ExpectBlink => {
                    let (before, _, ms) = self.report.as_ref().and_then(|r| r.idle_baseline).expect("report-idle first");
                    let draws = self.gpu.as_ref().unwrap().renderer.stats.draws - before;
                    let blinks = ms / BLINK.as_millis() as u64;
                    // Only a focused window blinks, and the user may have moved focus away.
                    let ok = !self.focused_window || (draws + 1 >= blinks && draws <= blinks + 2);
                    self.report.get_or_insert_with(Default::default).check(&format!("blink: {draws} draws in {ms} ms, about {blinks} wanted (window focused: {})", self.focused_window), ok, "");
                    if !ok {
                        self.exit_code = 1;
                    }
                }
                Step::ExpectScreen(text) => self.check_screen(&text, true),
                Step::ExpectNotScreen(text) => self.check_screen(&text, false),
                Step::ExpectPanes(n) => {
                    let got = self.tabs.get(self.active).map_or(0, |t| t.root.panes().len());
                    self.report.get_or_insert_with(Default::default).check(&format!("{n} panes in the active tab"), got == n, &format!("got {got}"));
                    if got != n {
                        self.exit_code = 1;
                    }
                }
                Step::ExpectTabs(n) => {
                    let got = self.tabs.len();
                    self.report.get_or_insert_with(Default::default).check(&format!("{n} tabs"), got == n, &format!("got {got}"));
                    if got != n {
                        self.exit_code = 1;
                    }
                }
                Step::ReportIdleFrames(ms) => {
                    // Draw count across a quiet interval, with the cursor not blinking.
                    let before = self.gpu.as_ref().unwrap().renderer.stats.draws;
                    let report = self.report.get_or_insert_with(Default::default);
                    report.idle_baseline = Some((before, Instant::now(), ms));
                    report.focus_baseline = self.focus_changes;
                    self.script_at = Instant::now() + Duration::from_millis(ms);
                }
                Step::ExpectIdle(limit) => {
                    let (before, _, ms) = self.report.as_ref().and_then(|r| r.idle_baseline).expect("report-idle first");
                    let draws = self.gpu.as_ref().unwrap().renderer.stats.draws - before;
                    // Without a limit, one draw per blink at most; a steady cursor should be zero.
                    // Another window taking focus redraws for real (the cursor turns hollow), so
                    // each focus change adds one allowed draw.
                    let focus = self.focus_changes - self.report.as_ref().map_or(0, |r| r.focus_baseline);
                    let allowed = limit.unwrap_or(ms / BLINK.as_millis() as u64 + 2) + focus;
                    let ok = draws <= allowed;
                    self.report.get_or_insert_with(Default::default).check(&format!("idle: {draws} draws in {ms} ms, {focus} focus changes, at most {allowed} allowed"), ok, "");
                    if !ok {
                        self.exit_code = 1;
                    }
                }
                Step::OpenQueue => self.run_action(Action::QueueCommand, event_loop),
                Step::Prompt(input) => self.prompt_input(input),
                Step::Queue(text) => {
                    if let Some(id) = self.tabs.get(self.active).map(|t| t.focused) {
                        self.queue_command(id, &text);
                    }
                }
                Step::ExpectQueued(n) => {
                    let got = self.focused_pane().map_or(0, |p| p.queue.len());
                    self.report.get_or_insert_with(Default::default).check(&format!("{n} commands queued"), got == n, &format!("got {got}"));
                    if got != n {
                        self.exit_code = 1;
                    }
                }
                Step::Theme(name) => {
                    let name = (name != "default").then_some(name);
                    assert!(name.as_deref().is_none_or(|n| Theme::by_name(n).is_some()), "unknown theme {name:?}");
                    self.set_theme(name);
                }
                Step::Quit => {
                    if let Some(r) = &self.report {
                        r.print();
                    }
                    self.panes.clear();
                    self.tabs.clear();
                    event_loop.exit();
                }
            }
        }
    }
}

/// The top bar, if any, and the visible panes. Takes fields rather than `App`, so the renderer
/// can stay mutably borrowed while drawing from this list.
fn draw_list<'a>(panes: &'a HashMap<PaneId, Pane>, bar: Option<&'a Bar>, bar_height: u32, visible: &[PaneId], width: u32) -> Vec<(&'a PaneView, PaneRect)> {
    let mut list = vec![];
    if let Some(bar) = bar {
        list.push((&bar.view, PaneRect { x: 0, y: 0, width, height: bar_height }));
    }
    for id in visible {
        if let Some(p) = panes.get(id) {
            list.push((&p.view, pixel_rect(p.rect)));
        }
    }
    list
}

fn open_link(uri: &str) {
    // Terminal output chose this string, so only ever hand on ordinary web and mail links.
    let scheme = uri.split(':').next().unwrap_or("").to_ascii_lowercase();
    if !matches!(scheme.as_str(), "http" | "https" | "mailto") {
        eprintln!("glyphium: not opening link with scheme {scheme:?}");
        return;
    }
    let program = if cfg!(target_os = "macos") { "open" } else if cfg!(windows) { "explorer" } else { "xdg-open" };
    let _ = std::process::Command::new(program).arg(uri).spawn();
}

impl ApplicationHandler<()> for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let attrs = Window::default_attributes().with_title("Glyphium").with_inner_size(LogicalSize::new(960.0, 620.0));
        let window = Arc::new(event_loop.create_window(attrs).expect("create window"));
        window.set_ime_allowed(true);
        self.window = Some(window);
        self.create_gpu();
        self.new_tab();
        self.script_at = Instant::now();
        self.request_redraw();
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, _: ()) {
        self.wake_pending.store(false, Ordering::SeqCst);
        if self.gpu.as_ref().is_some_and(|g| g.lost.load(Ordering::SeqCst)) {
            self.request_redraw();
        }
        if self.drain_output() {
            self.request_redraw();
        }
        if self.tabs.is_empty() {
            event_loop.exit();
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => {
                self.panes.clear();
                self.tabs.clear();
                event_loop.exit();
            }
            WindowEvent::RedrawRequested => {
                self.redraw();
                self.set_ime_area();
            }
            WindowEvent::Resized(size) => {
                if let Some(gpu) = &mut self.gpu {
                    gpu.config.width = size.width.max(1);
                    gpu.config.height = size.height.max(1);
                    gpu.surface.configure(&gpu.device, &gpu.config);
                }
                self.layout_dirty = true;
                self.force_redraw = true;
                self.request_redraw();
            }
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                self.set_font((self.config.font_pt * scale_factor as f32).round());
            }
            WindowEvent::Focused(focused) => {
                self.focused_window = focused;
                self.focus_changes += 1;
                if let Some(pane) = self.focused_pane() {
                    let bytes = pane.term.encode_focus(focused);
                    pane.session.write(bytes);
                }
                self.force_redraw = true;
                self.request_redraw();
            }
            WindowEvent::ModifiersChanged(m) => self.mods = m.state(),
            WindowEvent::KeyboardInput { event, .. } => {
                let code = match event.physical_key {
                    PhysicalKey::Code(c) => format!("{c:?}"),
                    PhysicalKey::Unidentified(_) => String::new(),
                };
                if self.prompt.is_some() {
                    if event.state == ElementState::Pressed {
                        if self.shortcut(&code, self.mods) == Some(Action::Quit) {
                            self.run_action(Action::Quit, event_loop);
                            return;
                        }
                        self.prompt_key(&event, &code);
                    }
                    return;
                }
                if event.state == ElementState::Pressed {
                    // Holding a modifier on the way to the paste shortcut must not cancel it.
                    let modifier = matches!(event.logical_key, Key::Named(NamedKey::Shift | NamedKey::Control | NamedKey::Alt | NamedKey::Super | NamedKey::Meta));
                    if self.banner.is_some() && !modifier && self.shortcut(&code, self.mods) != Some(Action::Paste) {
                        self.clear_banner();
                    }
                    if let Some(action) = self.shortcut(&code, self.mods) {
                        self.run_action(action, event_loop);
                        return;
                    }
                }
                let text = event.text_with_all_modifiers().filter(|t| !t.chars().all(char::is_control));
                let m = Self::key_mods(self.mods);
                // Modifiers that went into producing the text, so the encoder won't report them too.
                let consumed = if text.is_some() { m & (mods::SHIFT | if cfg!(target_os = "macos") { mods::ALT } else { 0 }) } else { 0 };
                let unshifted = match event.key_without_modifiers() {
                    Key::Character(s) => s.chars().next().map_or(0, |c| c as u32),
                    _ => 0,
                };
                let input = KeyInput {
                    action: match (event.state, event.repeat) {
                        (ElementState::Released, _) => KeyAction::Release,
                        (ElementState::Pressed, true) => KeyAction::Repeat,
                        (ElementState::Pressed, false) => KeyAction::Press,
                    },
                    key: key_from_code(&code).unwrap_or(0),
                    mods: m,
                    consumed_mods: consumed,
                    text,
                    unshifted,
                };
                self.send_key(&input);
            }
            WindowEvent::Ime(ime) => match ime {
                Ime::Commit(text) => {
                    self.preedit.clear();
                    if let Some(p) = &mut self.prompt {
                        p.text.extend(text.chars().filter(|c| !c.is_control()));
                    } else if let Some(pane) = self.focused_pane() {
                        pane.term.scroll_to_bottom();
                        pane.session.write(text.into_bytes());
                    }
                    self.request_redraw();
                }
                Ime::Preedit(text, _) => {
                    // Composition draws at the pane's cursor; the queue prompt takes commits only.
                    self.preedit = if self.prompt.is_some() { String::new() } else { text };
                    self.request_redraw();
                }
                Ime::Enabled | Ime::Disabled => {}
            },
            WindowEvent::CursorMoved { position, .. } => self.mouse_moved(position),
            WindowEvent::MouseInput { state, button, .. } => {
                let b = match button {
                    winit::event::MouseButton::Left => 1,
                    winit::event::MouseButton::Right => 2,
                    winit::event::MouseButton::Middle => 3,
                    _ => return,
                };
                self.mouse_button(state, b);
            }
            WindowEvent::MouseWheel { delta, .. } => self.wheel(delta),
            _ => {}
        }
    }

    /// Sleeps until the next blink or script step. Blinking is explicit damage: a focused pane
    /// whose cursor does not blink wakes for nothing.
    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if !self.script.is_empty() {
            self.run_script(event_loop);
        }
        if self.tabs.is_empty() {
            event_loop.exit();
            return;
        }
        let blinking = self.focused_window && self.tabs.get(self.active).and_then(|t| self.panes.get(&t.focused)).is_some_and(|p| p.frame.cursor.blinking && p.frame.cursor.visible | !self.blink_on);
        let now = Instant::now();
        if blinking && now >= self.next_blink {
            self.blink_on = !self.blink_on;
            self.next_blink = now + BLINK;
            self.request_redraw();
        }
        if self.notice.as_ref().is_some_and(|(_, until)| now >= *until) {
            self.notice = None;
            self.request_redraw();
        }
        // Only a pane with something queued or running from the queue is ever polled, so a
        // terminal with an empty queue still sleeps until real input or output.
        let queue_wake = self.panes.values().any(|p| p.queue.active()).then(|| {
            let enter_due = self.panes.values().filter_map(|p| p.queue.enter_due()).any(|t| now >= t);
            if now >= self.next_queue_poll || enter_due {
                self.poll_queues();
                self.next_queue_poll = now + queue::POLL;
            }
            // A typed command's Enter may be due sooner than the next poll.
            self.panes.values().filter_map(|p| p.queue.enter_due()).fold(self.next_queue_poll, Instant::min)
        });
        let wakes = [(!self.script.is_empty()).then_some(self.script_at), blinking.then_some(self.next_blink), queue_wake, self.notice.as_ref().map(|(_, t)| *t)];
        event_loop.set_control_flow(match wakes.into_iter().flatten().min() {
            Some(t) => ControlFlow::WaitUntil(t),
            None => ControlFlow::Wait,
        });
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut profile = Profile::default();
    let mut script_steps = vec![];
    let mut config_flag = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-e" => {
                profile.command = args.get(i + 1).cloned();
                profile.args = args.get(i + 2..).map(<[String]>::to_vec).unwrap_or_default();
                break;
            }
            "--script" => {
                i += 1;
                let path = args.get(i).expect("--script FILE");
                script_steps = script::parse(&std::fs::read_to_string(path).expect("read script"));
            }
            "--config" => {
                i += 1;
                config_flag = Some(args.get(i).expect("--config FILE").into());
            }
            "--version" => {
                println!("glyphium {}", env!("CARGO_PKG_VERSION"));
                return;
            }
            other => {
                eprintln!("usage: glyphium [--config FILE] [--script FILE] [-e COMMAND [ARGS...]]\nunknown argument: {other}");
                std::process::exit(2);
            }
        }
        i += 1;
    }
    if profile.cwd.is_none() {
        profile.cwd = std::env::var_os("HOME").map(Into::into);
    }
    let event_loop = EventLoop::<()>::with_user_event().build().expect("event loop");
    let proxy = event_loop.create_proxy();
    let config = config::load(config::path(config_flag).as_deref());
    let mut app = App::new(proxy, profile, config, script_steps);
    event_loop.run_app(&mut app).expect("event loop");
    std::process::exit(app.exit_code);
}
