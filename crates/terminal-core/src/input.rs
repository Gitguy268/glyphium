//! User input encoded for the application: keys, mouse, focus and paste, using Ghostty's
//! encoders so that the terminal's modes (kitty keyboard, DECCKM, SGR mouse…) are honoured.
//! Also viewport scrolling and selection, which act on the terminal itself.

use std::ffi::{c_char, c_void};
use std::mem::size_of;
use std::ptr;

use glyphium_core_ffi::*;

use crate::{MODE_BRACKETED_PASTE, Terminal, check, zeroed};

pub mod mods {
    pub const SHIFT: u16 = 1 << 0;
    pub const CTRL: u16 = 1 << 1;
    pub const ALT: u16 = 1 << 2;
    pub const SUPER: u16 = 1 << 3;
    pub const CAPS_LOCK: u16 = 1 << 4;
    pub const NUM_LOCK: u16 = 1 << 5;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyAction {
    Press,
    Repeat,
    Release,
}

/// A physical key by its W3C `KeyboardEvent.code` name ("KeyA", "ArrowUp", "ShiftLeft").
/// winit's `KeyCode` names and browser events both use these, so native and web share one
/// mapping.
pub fn key_from_code(code: &str) -> Option<u32> {
    macro_rules! keys {
        ($($name:literal => $k:ident),* $(,)?) => { match code { $($name => Some($k as u32),)* _ => None } };
    }
    keys! {
        "Backquote" => GHOSTTY_KEY_BACKQUOTE, "Backslash" => GHOSTTY_KEY_BACKSLASH,
        "BracketLeft" => GHOSTTY_KEY_BRACKET_LEFT, "BracketRight" => GHOSTTY_KEY_BRACKET_RIGHT,
        "Comma" => GHOSTTY_KEY_COMMA, "Equal" => GHOSTTY_KEY_EQUAL, "Minus" => GHOSTTY_KEY_MINUS,
        "Period" => GHOSTTY_KEY_PERIOD, "Quote" => GHOSTTY_KEY_QUOTE, "Semicolon" => GHOSTTY_KEY_SEMICOLON,
        "Slash" => GHOSTTY_KEY_SLASH, "IntlBackslash" => GHOSTTY_KEY_INTL_BACKSLASH,
        "Digit0" => GHOSTTY_KEY_DIGIT_0, "Digit1" => GHOSTTY_KEY_DIGIT_1, "Digit2" => GHOSTTY_KEY_DIGIT_2,
        "Digit3" => GHOSTTY_KEY_DIGIT_3, "Digit4" => GHOSTTY_KEY_DIGIT_4, "Digit5" => GHOSTTY_KEY_DIGIT_5,
        "Digit6" => GHOSTTY_KEY_DIGIT_6, "Digit7" => GHOSTTY_KEY_DIGIT_7, "Digit8" => GHOSTTY_KEY_DIGIT_8,
        "Digit9" => GHOSTTY_KEY_DIGIT_9,
        "KeyA" => GHOSTTY_KEY_A, "KeyB" => GHOSTTY_KEY_B, "KeyC" => GHOSTTY_KEY_C, "KeyD" => GHOSTTY_KEY_D,
        "KeyE" => GHOSTTY_KEY_E, "KeyF" => GHOSTTY_KEY_F, "KeyG" => GHOSTTY_KEY_G, "KeyH" => GHOSTTY_KEY_H,
        "KeyI" => GHOSTTY_KEY_I, "KeyJ" => GHOSTTY_KEY_J, "KeyK" => GHOSTTY_KEY_K, "KeyL" => GHOSTTY_KEY_L,
        "KeyM" => GHOSTTY_KEY_M, "KeyN" => GHOSTTY_KEY_N, "KeyO" => GHOSTTY_KEY_O, "KeyP" => GHOSTTY_KEY_P,
        "KeyQ" => GHOSTTY_KEY_Q, "KeyR" => GHOSTTY_KEY_R, "KeyS" => GHOSTTY_KEY_S, "KeyT" => GHOSTTY_KEY_T,
        "KeyU" => GHOSTTY_KEY_U, "KeyV" => GHOSTTY_KEY_V, "KeyW" => GHOSTTY_KEY_W, "KeyX" => GHOSTTY_KEY_X,
        "KeyY" => GHOSTTY_KEY_Y, "KeyZ" => GHOSTTY_KEY_Z,
        "AltLeft" => GHOSTTY_KEY_ALT_LEFT, "AltRight" => GHOSTTY_KEY_ALT_RIGHT,
        "ControlLeft" => GHOSTTY_KEY_CONTROL_LEFT, "ControlRight" => GHOSTTY_KEY_CONTROL_RIGHT,
        "ShiftLeft" => GHOSTTY_KEY_SHIFT_LEFT, "ShiftRight" => GHOSTTY_KEY_SHIFT_RIGHT,
        "MetaLeft" => GHOSTTY_KEY_META_LEFT, "MetaRight" => GHOSTTY_KEY_META_RIGHT,
        "SuperLeft" => GHOSTTY_KEY_META_LEFT, "SuperRight" => GHOSTTY_KEY_META_RIGHT,
        "Backspace" => GHOSTTY_KEY_BACKSPACE, "CapsLock" => GHOSTTY_KEY_CAPS_LOCK,
        "ContextMenu" => GHOSTTY_KEY_CONTEXT_MENU, "Enter" => GHOSTTY_KEY_ENTER, "Space" => GHOSTTY_KEY_SPACE,
        "Tab" => GHOSTTY_KEY_TAB, "Delete" => GHOSTTY_KEY_DELETE, "End" => GHOSTTY_KEY_END,
        "Home" => GHOSTTY_KEY_HOME, "Insert" => GHOSTTY_KEY_INSERT, "PageDown" => GHOSTTY_KEY_PAGE_DOWN,
        "PageUp" => GHOSTTY_KEY_PAGE_UP, "ArrowDown" => GHOSTTY_KEY_ARROW_DOWN, "ArrowLeft" => GHOSTTY_KEY_ARROW_LEFT,
        "ArrowRight" => GHOSTTY_KEY_ARROW_RIGHT, "ArrowUp" => GHOSTTY_KEY_ARROW_UP, "NumLock" => GHOSTTY_KEY_NUM_LOCK,
        "Numpad0" => GHOSTTY_KEY_NUMPAD_0, "Numpad1" => GHOSTTY_KEY_NUMPAD_1, "Numpad2" => GHOSTTY_KEY_NUMPAD_2,
        "Numpad3" => GHOSTTY_KEY_NUMPAD_3, "Numpad4" => GHOSTTY_KEY_NUMPAD_4, "Numpad5" => GHOSTTY_KEY_NUMPAD_5,
        "Numpad6" => GHOSTTY_KEY_NUMPAD_6, "Numpad7" => GHOSTTY_KEY_NUMPAD_7, "Numpad8" => GHOSTTY_KEY_NUMPAD_8,
        "Numpad9" => GHOSTTY_KEY_NUMPAD_9, "NumpadAdd" => GHOSTTY_KEY_NUMPAD_ADD,
        "NumpadDecimal" => GHOSTTY_KEY_NUMPAD_DECIMAL, "NumpadDivide" => GHOSTTY_KEY_NUMPAD_DIVIDE,
        "NumpadEnter" => GHOSTTY_KEY_NUMPAD_ENTER, "NumpadEqual" => GHOSTTY_KEY_NUMPAD_EQUAL,
        "NumpadMultiply" => GHOSTTY_KEY_NUMPAD_MULTIPLY, "NumpadSubtract" => GHOSTTY_KEY_NUMPAD_SUBTRACT,
        "Escape" => GHOSTTY_KEY_ESCAPE,
        "F1" => GHOSTTY_KEY_F1, "F2" => GHOSTTY_KEY_F2, "F3" => GHOSTTY_KEY_F3, "F4" => GHOSTTY_KEY_F4,
        "F5" => GHOSTTY_KEY_F5, "F6" => GHOSTTY_KEY_F6, "F7" => GHOSTTY_KEY_F7, "F8" => GHOSTTY_KEY_F8,
        "F9" => GHOSTTY_KEY_F9, "F10" => GHOSTTY_KEY_F10, "F11" => GHOSTTY_KEY_F11, "F12" => GHOSTTY_KEY_F12,
    }
}

#[derive(Clone, Copy, Debug)]
pub struct KeyInput<'a> {
    pub action: KeyAction,
    /// From [`key_from_code`]; 0 when the physical key is unknown.
    pub key: u32,
    pub mods: u16,
    /// Modifiers already used up producing `text` (shift for "A", option for "é").
    pub consumed_mods: u16,
    /// Text the key produces under the current layout, if any.
    pub text: Option<&'a str>,
    /// The key's character with no modifiers, under the current layout; 0 if unknown.
    pub unshifted: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MouseAction {
    Press,
    Release,
    Motion,
}

/// Left 1, right 2, middle 3, wheel up 4, wheel down 5, wheel left 6, wheel right 7.
pub type MouseButton = u32;

#[derive(Clone, Copy, Debug)]
pub struct MouseInput {
    pub action: MouseAction,
    pub button: Option<MouseButton>,
    pub mods: u16,
    pub x: f32,
    pub y: f32,
    pub any_button_pressed: bool,
}

/// Encoder handles, created once per terminal and reused for every event.
pub(crate) struct Encoders {
    key: GhosttyKeyEncoder,
    key_event: GhosttyKeyEvent,
    mouse: GhosttyMouseEncoder,
    mouse_event: GhosttyMouseEvent,
}

impl Encoders {
    pub(crate) fn new() -> Self {
        let mut e = Encoders { key: ptr::null_mut(), key_event: ptr::null_mut(), mouse: ptr::null_mut(), mouse_event: ptr::null_mut() };
        unsafe {
            check(ghostty_key_encoder_new(ptr::null(), &mut e.key), "key encoder");
            check(ghostty_key_event_new(ptr::null(), &mut e.key_event), "key event");
            check(ghostty_mouse_encoder_new(ptr::null(), &mut e.mouse), "mouse encoder");
            check(ghostty_mouse_event_new(ptr::null(), &mut e.mouse_event), "mouse event");
        }
        e
    }
}

impl Drop for Encoders {
    fn drop(&mut self) {
        unsafe {
            ghostty_key_event_free(self.key_event);
            ghostty_key_encoder_free(self.key);
            ghostty_mouse_event_free(self.mouse_event);
            ghostty_mouse_encoder_free(self.mouse);
        }
    }
}

/// Calls `f` with a buffer, growing it once if the encoder asks for more room.
fn encode_with(mut f: impl FnMut(*mut c_char, usize, *mut usize) -> GhosttyResult) -> Vec<u8> {
    let mut buf = vec![0u8; 128];
    let mut n = 0usize;
    let mut r = f(buf.as_mut_ptr() as *mut c_char, buf.len(), &mut n);
    if r == GHOSTTY_OUT_OF_SPACE {
        buf.resize(n, 0);
        r = f(buf.as_mut_ptr() as *mut c_char, buf.len(), &mut n);
    }
    check(r, "encode");
    buf.truncate(n);
    buf
}

impl Terminal {
    /// Bytes to send to the application for this key; empty when the key produces nothing.
    pub fn encode_key(&mut self, input: &KeyInput) -> Vec<u8> {
        let (enc, ev) = (self.encoders.key, self.encoders.key_event);
        unsafe {
            ghostty_key_encoder_setopt_from_terminal(enc, self.term);
            ghostty_key_event_set_action(ev, match input.action {
                KeyAction::Press => GHOSTTY_KEY_ACTION_PRESS,
                KeyAction::Repeat => GHOSTTY_KEY_ACTION_REPEAT,
                KeyAction::Release => GHOSTTY_KEY_ACTION_RELEASE,
            });
            ghostty_key_event_set_key(ev, input.key as GhosttyKey);
            ghostty_key_event_set_mods(ev, input.mods);
            ghostty_key_event_set_consumed_mods(ev, input.consumed_mods);
            ghostty_key_event_set_composing(ev, false);
            let text = input.text.unwrap_or("");
            ghostty_key_event_set_utf8(ev, text.as_ptr() as *const c_char, text.len());
            ghostty_key_event_set_unshifted_codepoint(ev, input.unshifted);
        }
        encode_with(|buf, len, out| unsafe { ghostty_key_encoder_encode(enc, ev, buf, len, out) })
    }

    /// True when the application has asked for mouse events.
    pub fn mouse_tracking(&self) -> bool {
        self.get(GHOSTTY_TERMINAL_DATA_MOUSE_TRACKING)
    }

    /// Bytes for a mouse event, positioned in pixels; empty when the application's mouse mode
    /// does not report it. `size` is the screen in pixels, `cell` a cell in pixels.
    pub fn encode_mouse(&mut self, input: &MouseInput, size: (u32, u32), cell: (u32, u32)) -> Vec<u8> {
        let (enc, ev) = (self.encoders.mouse, self.encoders.mouse_event);
        let mut geometry: GhosttyMouseEncoderSize = unsafe { zeroed() };
        geometry.size = size_of::<GhosttyMouseEncoderSize>();
        geometry.screen_width = size.0;
        geometry.screen_height = size.1;
        geometry.cell_width = cell.0;
        geometry.cell_height = cell.1;
        let any = input.any_button_pressed;
        let track = true;
        unsafe {
            ghostty_mouse_encoder_setopt_from_terminal(enc, self.term);
            ghostty_mouse_encoder_setopt(enc, GHOSTTY_MOUSE_ENCODER_OPT_SIZE, &geometry as *const _ as *const c_void);
            ghostty_mouse_encoder_setopt(enc, GHOSTTY_MOUSE_ENCODER_OPT_ANY_BUTTON_PRESSED, &any as *const _ as *const c_void);
            ghostty_mouse_encoder_setopt(enc, GHOSTTY_MOUSE_ENCODER_OPT_TRACK_LAST_CELL, &track as *const _ as *const c_void);
            ghostty_mouse_event_set_action(ev, match input.action {
                MouseAction::Press => GHOSTTY_MOUSE_ACTION_PRESS,
                MouseAction::Release => GHOSTTY_MOUSE_ACTION_RELEASE,
                MouseAction::Motion => GHOSTTY_MOUSE_ACTION_MOTION,
            });
            match input.button {
                Some(b) => ghostty_mouse_event_set_button(ev, b as GhosttyMouseButton),
                None => ghostty_mouse_event_clear_button(ev),
            }
            ghostty_mouse_event_set_mods(ev, input.mods);
            ghostty_mouse_event_set_position(ev, GhosttyMousePosition { x: input.x, y: input.y });
        }
        encode_with(|buf, len, out| unsafe { ghostty_mouse_encoder_encode(enc, ev, buf, len, out) })
    }

    /// Focus report, only if the application enabled mode 1004.
    pub fn encode_focus(&self, focused: bool) -> Vec<u8> {
        if !self.mode(MODE_FOCUS_EVENT) {
            return vec![];
        }
        let event = if focused { GHOSTTY_FOCUS_GAINED } else { GHOSTTY_FOCUS_LOST };
        encode_with(|buf, len, out| unsafe { ghostty_focus_encode(event, buf, len, out) })
    }

    /// Clipboard text framed for the application: bracketed if it asked for that, with control
    /// characters and any embedded end-of-paste marker neutralised.
    pub fn encode_paste(&self, text: &str) -> Vec<u8> {
        let bracketed = self.mode(MODE_BRACKETED_PASTE);
        let mut data = text.as_bytes().to_vec();
        let mut buf = vec![0u8; data.len() + 16];
        let mut n = 0usize;
        let r = unsafe { ghostty_paste_encode(data.as_mut_ptr() as *mut c_char, data.len(), bracketed, buf.as_mut_ptr() as *mut c_char, buf.len(), &mut n) };
        check(r, "paste encode");
        buf.truncate(n);
        buf
    }

    /// True if pasting `text` unbracketed could run commands: the app should confirm first.
    pub fn paste_needs_confirmation(&self, text: &str) -> bool {
        !self.mode(MODE_BRACKETED_PASTE) && !unsafe { ghostty_paste_is_safe(text.as_ptr() as *const c_char, text.len()) }
    }

    /// Scrolls the viewport by `delta` rows; negative is up, into history.
    pub fn scroll_viewport(&mut self, delta: isize) {
        let mut b: GhosttyTerminalScrollViewport = unsafe { zeroed() };
        b.tag = GHOSTTY_SCROLL_VIEWPORT_DELTA;
        b.value.delta = delta;
        unsafe { ghostty_terminal_scroll_viewport(self.term, b) };
    }

    pub fn scroll_to_bottom(&mut self) {
        let mut b: GhosttyTerminalScrollViewport = unsafe { zeroed() };
        b.tag = GHOSTTY_SCROLL_VIEWPORT_BOTTOM;
        unsafe { ghostty_terminal_scroll_viewport(self.term, b) };
    }

    fn viewport_ref(&self, x: u16, y: u16) -> Option<GhosttyGridRef> {
        let mut r: GhosttyGridRef = unsafe { zeroed() };
        r.size = size_of::<GhosttyGridRef>();
        let mut pt: GhosttyPoint = unsafe { zeroed() };
        pt.tag = GHOSTTY_POINT_TAG_VIEWPORT;
        pt.value.coordinate.x = x;
        pt.value.coordinate.y = y as u32;
        (unsafe { ghostty_terminal_grid_ref(self.term, pt, &mut r) } == GHOSTTY_SUCCESS).then_some(r)
    }

    /// Selects from one viewport cell to another, inclusive. The terminal keeps the selection
    /// anchored to the text through scrolling and reflow.
    pub fn select(&mut self, start: (u16, u16), end: (u16, u16)) {
        let (Some(s), Some(e)) = (self.viewport_ref(start.0, start.1), self.viewport_ref(end.0, end.1)) else { return };
        let mut sel: GhosttySelection = unsafe { zeroed() };
        sel.size = size_of::<GhosttySelection>();
        sel.start = s;
        sel.end = e;
        check(unsafe { ghostty_terminal_set(self.term, GHOSTTY_TERMINAL_OPT_SELECTION, &sel as *const _ as *const c_void) }, "select");
    }

    /// Selects the word under a viewport cell.
    pub fn select_word(&mut self, cell: (u16, u16)) {
        let Some(r) = self.viewport_ref(cell.0, cell.1) else { return };
        let mut sel: GhosttySelection = unsafe { zeroed() };
        sel.size = size_of::<GhosttySelection>();
        let mut options: GhosttyTerminalSelectWordOptions = unsafe { zeroed() };
        options.size = size_of::<GhosttyTerminalSelectWordOptions>();
        options.ref_ = r;
        if unsafe { ghostty_terminal_select_word(self.term, &options, &mut sel) } == GHOSTTY_SUCCESS {
            check(unsafe { ghostty_terminal_set(self.term, GHOSTTY_TERMINAL_OPT_SELECTION, &sel as *const _ as *const c_void) }, "select word");
        }
    }

    pub fn clear_selection(&mut self) {
        check(unsafe { ghostty_terminal_set(self.term, GHOSTTY_TERMINAL_OPT_SELECTION, ptr::null()) }, "clear selection");
    }

    /// The selected text, with soft wraps joined.
    pub fn selection_text(&self) -> Option<String> {
        let mut options: GhosttyTerminalSelectionFormatOptions = unsafe { zeroed() };
        options.size = size_of::<GhosttyTerminalSelectionFormatOptions>();
        options.emit = GHOSTTY_FORMATTER_FORMAT_PLAIN;
        options.unwrap = true;
        options.trim = true;
        let (mut p, mut n) = (ptr::null_mut::<u8>(), 0usize);
        let r = unsafe { ghostty_terminal_selection_format_alloc(self.term, ptr::null(), options, &mut p, &mut n) };
        if r != GHOSTTY_SUCCESS {
            return None;
        }
        let text = if n == 0 { String::new() } else { String::from_utf8_lossy(unsafe { std::slice::from_raw_parts(p, n) }).into_owned() };
        unsafe { ghostty_free(ptr::null(), p, n) };
        Some(text)
    }
}

pub(crate) const MODE_FOCUS_EVENT: GhosttyMode = crate::mode(1004, false);
