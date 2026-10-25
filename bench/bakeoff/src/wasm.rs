//! wasm-bindgen surface for running the cores in a Web Worker. The worker feeds chunks and
//! does the timing; hashes come from the same `State::hash` as the native runner, so native
//! and wasm results can be compared directly.

use wasm_bindgen::prelude::*;

use crate::Core;

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = console)]
    fn error(s: &str);
}

/// panic=abort turns a panic into a bare `unreachable` trap; print the message first.
#[wasm_bindgen(start)]
pub fn start() {
    std::panic::set_hook(Box::new(|info| error(&info.to_string())));
}

#[wasm_bindgen]
pub fn cores() -> Vec<String> {
    let mut v = vec![];
    #[cfg(feature = "ghostty")]
    v.push("libghostty-vt".to_string());
    #[cfg(feature = "alacritty")]
    v.push("alacritty_terminal".to_string());
    #[cfg(feature = "wezterm")]
    v.push("wezterm-term".to_string());
    v
}

#[wasm_bindgen]
pub struct Bench {
    core: Box<dyn Core>,
}

#[wasm_bindgen]
impl Bench {
    #[wasm_bindgen(constructor)]
    pub fn new(name: &str) -> Result<Bench, JsError> {
        let core: Box<dyn Core> = match name {
            #[cfg(feature = "ghostty")]
            "libghostty-vt" => Box::new(crate::ghostty::Ghostty::new()),
            #[cfg(feature = "alacritty")]
            "alacritty_terminal" => Box::new(crate::alacritty::Alacritty::new()),
            #[cfg(feature = "wezterm")]
            "wezterm-term" => Box::new(crate::wezterm::Wezterm::new()),
            _ => return Err(JsError::new(&format!("core not in this build: {name}"))),
        };
        Ok(Bench { core })
    }

    pub fn write(&mut self, bytes: &[u8]) {
        self.core.write(bytes);
    }

    pub fn hash(&mut self) -> String {
        self.core.snapshot().hash()
    }

    pub fn screen_text(&mut self) -> String {
        self.core.snapshot().screen_text().join("\n")
    }
}
