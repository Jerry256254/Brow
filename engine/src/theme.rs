//! KucLab Brow design tokens - "Gerin Paper Design v2" adapted for Brownie.
//! Paper and ink only: no gradients, no purple, no glow, no emoji.

use crate::css::Color;

pub const PAPER: Color = Color::rgb(0xf2, 0xee, 0xe6);
pub const SURFACE: Color = Color::rgb(0xff, 0xfd, 0xf9);
pub const LINE: Color = Color::rgb(0xd9, 0xd1, 0xc4);
pub const INK: Color = Color::rgb(0x1c, 0x1b, 0x18);
pub const INK_STRONG: Color = Color::rgb(0x14, 0x13, 0x0f);
pub const MUTED: Color = Color::rgb(0x5b, 0x55, 0x4a);
pub const MUTED_2: Color = Color::rgb(0x6b, 0x64, 0x57);
pub const LINK: Color = Color::rgb(0x1f, 0x4e, 0x73);
pub const BTN: Color = Color::rgb(0x16, 0x20, 0x2a);
pub const BTN_TEXT: Color = Color::rgb(0xff, 0xff, 0xff);
pub const ACCENT: Color = Color::rgb(0x2b, 0x6c, 0xa3);
pub const OK_BG: Color = Color::rgb(0xee, 0xf6, 0xee);
pub const OK_LINE: Color = Color::rgb(0xcf, 0xe4, 0xcf);
pub const OK_TEXT: Color = Color::rgb(0x2a, 0x5a, 0x2a);
pub const ERR_BG: Color = Color::rgb(0xfb, 0xf1, 0xef);
pub const ERR_LINE: Color = Color::rgb(0xe8, 0xcf, 0xc9);
pub const ERR_TEXT: Color = Color::rgb(0x8a, 0x2f, 0x2f);

/// Page background used when a page sets none (paper, not white).
pub const PAGE_BG: Color = PAPER;
