//! Named colour themes for [`crate::Terminal::set_theme`]. Both are unofficial tributes; NOTICE
//! says where their colours come from.

use crate::Rgb;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Theme {
    pub foreground: Rgb,
    pub background: Rgb,
    pub cursor: Rgb,
    /// Background of selected text.
    pub selection: Rgb,
    /// ANSI colours 0 to 15. The rest of the 256-colour palette stays Ghostty's.
    pub ansi: [Rgb; 16],
}

const fn rgb(hex: u32) -> Rgb {
    Rgb { r: (hex >> 16) as u8, g: (hex >> 8) as u8, b: hex as u8 }
}

const NUJABES: Theme = Theme {
    foreground: rgb(0xe3dcda),
    background: rgb(0x0d0a11),
    cursor: rgb(0xb26ac6),
    selection: rgb(0x3d2456),
    ansi: [
        rgb(0x231a2b),
        rgb(0xc56e6f),
        rgb(0x9aa06b),
        rgb(0xe0c25a),
        rgb(0x8f74e8),
        rgb(0xdc7fcc),
        rgb(0x7fa8b0),
        rgb(0xc9c0bd),
        rgb(0x7d747a),
        rgb(0xdd8189),
        rgb(0xb6bb85),
        rgb(0xf2e06a),
        rgb(0xa894f0),
        rgb(0xe79ad8),
        rgb(0x9cc3ca),
        rgb(0xfdf7f7),
    ],
};

const GRADUATION: Theme = Theme {
    foreground: rgb(0xfff4e0),
    background: rgb(0x1a0b2e),
    cursor: rgb(0xffc93c),
    selection: rgb(0x4a2a78),
    ansi: [
        rgb(0x2a1a44),
        rgb(0xff5a5f),
        rgb(0x7ee081),
        rgb(0xffc93c),
        rgb(0x7ea6ff),
        rgb(0xff5fa8),
        rgb(0x4dd8e8),
        rgb(0xe8dcc8),
        rgb(0x6f5a93),
        rgb(0xff8487),
        rgb(0xa3efa5),
        rgb(0xffdd7a),
        rgb(0xa5c2ff),
        rgb(0xff8fc4),
        rgb(0x86e8f2),
        rgb(0xfffaf0),
    ],
};

impl Theme {
    pub const NAMES: [&str; 2] = ["nujabes", "graduation"];

    pub fn by_name(name: &str) -> Option<Theme> {
        match name {
            "nujabes" => Some(NUJABES),
            "graduation" => Some(GRADUATION),
            _ => None,
        }
    }
}
