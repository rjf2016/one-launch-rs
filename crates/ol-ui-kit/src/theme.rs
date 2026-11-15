use gpui::{Hsla, Rgba, rgb};

pub const FONT: &str = "Segoe UI";

pub const BG: u32 = 0x0b0d12;
pub const SURFACE: u32 = 0x12151c;
pub const SURFACE_2: u32 = 0x1a1e28;
pub const SURFACE_3: u32 = 0x222736;
pub const BORDER: u32 = 0x262b38;
pub const BORDER_STRONG: u32 = 0x343a4b;
pub const TEXT: u32 = 0xe8eaf0;
pub const TEXT_STRONG: u32 = 0xffffff;
pub const MUTED: u32 = 0x8f97ab;
pub const FAINT: u32 = 0x7f879b;
pub const ACCENT: u32 = 0x8b5cf6;
pub const WARNING: u32 = 0xf5b750;
pub const CLOSE_HOVER: u32 = 0xc42b1c;

pub const LAUNCHERS: [(&str, u32); 4] = [
    ("Steam", 0x66c0f4),
    ("Epic Games", 0xe6e6e6),
    ("Battle.net", 0x148eff),
    ("Riot Games", 0xff4655),
];

pub fn color(hex: u32) -> Rgba {
    rgb(hex)
}

pub fn alpha(hex: u32, opacity: f32) -> Hsla {
    Hsla::from(rgb(hex)).opacity(opacity)
}
