//! Простой CSS-подобный парсер тем.
//!
//! ```text
//! terminal {
//!   background: rgba(10,10,16,0.7);
//!   color: #ebebf0;
//!   padding: 24;
//!   shape: square;        /* square | fill */
//! }
//! wallpaper {
//!   type: bitmap;         /* solid | gradient | bitmap */
//!   bitmap: autumn;       /* default | autumn | snow — встроенные картинки */
//!   color: #121422;       /* для type: solid */
//!   color2: #281636;      /* для type: gradient — второй цвет */
//! }
//! bar {
//!   position: bottom;     /* top | bottom | hidden */
//!   background: rgba(0,0,0,0.63);
//!   color: #ffffff;
//!   height: 28;
//! }
//! cursor {
//!   color: #ffffff;
//! }
//! ```
//!
//! Парсер терпимый: неизвестные блоки/свойства/значения молча
//! пропускаются, а не роняют разбор — нераспознанное просто остаётся
//! равным базовой теме, с которой начали (см. `parse`).
//!
//! Настоящей загрузки произвольной картинки пользователя пока нет —
//! нет драйвера диска, чтобы затащить файл в ОС (это будет в Phase 6+).
//! Поэтому `wallpaper { type: bitmap; bitmap: ...; }` может ссылаться
//! только на один из трёх встроенных наборов пикселей (default/autumn/snow).

use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::bitmap;
use crate::theme::{BarPosition, Color, PanelShape, Theme, Wallpaper};

/// Разбирает CSS-подобный текст, накладывая найденные свойства поверх
/// `base`. Никогда не паникует.
pub fn parse(input: &str, base: Theme) -> Theme {
    let mut theme = base;
    for block in find_blocks(input) {
        match block.name.as_str() {
            "terminal" => apply_terminal(&mut theme, &block.body),
            "wallpaper" => apply_wallpaper(&mut theme, &block.body),
            "bar" => apply_bar(&mut theme, &block.body),
            "cursor" => apply_cursor(&mut theme, &block.body),
            _ => {}
        }
    }
    theme
}

struct Block {
    name: String,
    body: String,
}

/// Находит блоки вида `имя { тело }` — без вложенности, плоского
/// набора свойств вполне достаточно.
fn find_blocks(input: &str) -> Vec<Block> {
    let mut blocks = Vec::new();
    let mut i = 0;
    while i < input.len() {
        let Some(open_rel) = input[i..].find('{') else { break };
        let name = input[i..i + open_rel].trim().to_string();
        let after_open = i + open_rel + 1;
        let Some(close_rel) = input[after_open..].find('}') else { break };
        let body = input[after_open..after_open + close_rel].to_string();
        if !name.is_empty() {
            blocks.push(Block { name, body });
        }
        i = after_open + close_rel + 1;
    }
    blocks
}

/// Разбирает тело блока на пары (свойство, значение) по ';' и ':'.
fn declarations(body: &str) -> Vec<(String, String)> {
    body.split(';')
        .filter_map(|decl| {
            let (key, value) = decl.split_once(':')?;
            let key = key.trim();
            let value = value.trim();
            if key.is_empty() || value.is_empty() {
                None
            } else {
                Some((key.to_string(), value.to_string()))
            }
        })
        .collect()
}

fn apply_terminal(theme: &mut Theme, body: &str) {
    for (key, value) in declarations(body) {
        match key.as_str() {
            "background" => {
                if let Some(c) = parse_color(&value) {
                    theme.terminal_bg = c;
                }
            }
            "color" => {
                if let Some(c) = parse_color(&value) {
                    theme.terminal_fg = c;
                }
            }
            "padding" => {
                if let Some(n) = parse_u32(&value) {
                    theme.terminal_padding_px = n;
                }
            }
            "shape" => {
                theme.panel_shape = match value.as_str() {
                    "square" => PanelShape::Square,
                    "fill" => PanelShape::Fill,
                    _ => theme.panel_shape,
                };
            }
            _ => {}
        }
    }
}

fn apply_wallpaper(theme: &mut Theme, body: &str) {
    let decls = declarations(body);
    let kind = decls.iter().find(|(k, _)| k == "type").map(|(_, v)| v.as_str());
    match kind {
        Some("solid") => {
            if let Some(c) = decls.iter().find(|(k, _)| k == "color").and_then(|(_, v)| parse_color(v)) {
                theme.wallpaper = Wallpaper::Solid(c);
            }
        }
        Some("gradient") => {
            let c1 = decls.iter().find(|(k, _)| k == "color").and_then(|(_, v)| parse_color(v));
            let c2 = decls.iter().find(|(k, _)| k == "color2").and_then(|(_, v)| parse_color(v));
            if let (Some(c1), Some(c2)) = (c1, c2) {
                theme.wallpaper = Wallpaper::VerticalGradient(c1, c2);
            }
        }
        Some("bitmap") => {
            if let Some((_, v)) = decls.iter().find(|(k, _)| k == "bitmap") {
                theme.wallpaper = match v.as_str() {
                    "default" => Wallpaper::Bitmap(&bitmap::DEFAULT_WALLPAPER),
                    "autumn" => Wallpaper::Bitmap(&bitmap::AUTUMN_WALLPAPER),
                    "snow" => Wallpaper::Bitmap(&bitmap::SNOW_WALLPAPER),
                    _ => theme.wallpaper,
                };
            }
        }
        _ => {}
    }
}

fn apply_bar(theme: &mut Theme, body: &str) {
    for (key, value) in declarations(body) {
        match key.as_str() {
            "position" => {
                theme.bar.position = match value.as_str() {
                    "top" => BarPosition::Top,
                    "bottom" => BarPosition::Bottom,
                    "hidden" => BarPosition::Hidden,
                    _ => theme.bar.position,
                };
            }
            "background" => {
                if let Some(c) = parse_color(&value) {
                    theme.bar.background = c;
                }
            }
            "color" => {
                if let Some(c) = parse_color(&value) {
                    theme.bar.foreground = c;
                }
            }
            "height" => {
                if let Some(n) = parse_u32(&value) {
                    theme.bar.height_px = n;
                }
            }
            _ => {}
        }
    }
}

fn apply_cursor(theme: &mut Theme, body: &str) {
    for (key, value) in declarations(body) {
        if key == "color" {
            if let Some(c) = parse_color(&value) {
                theme.cursor = c;
            }
        }
    }
}

fn parse_u32(s: &str) -> Option<u32> {
    s.trim().parse().ok()
}

/// Понимает "#rrggbb", "rgb(r,g,b)" и "rgba(r,g,b,a)" (a — float 0.0..=1.0).
fn parse_color(s: &str) -> Option<Color> {
    let s = s.trim();
    if let Some(hex) = s.strip_prefix('#') {
        if hex.len() != 6 {
            return None;
        }
        let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
        let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
        let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
        return Some(Color::rgb(r, g, b));
    }
    if let Some(inner) = s.strip_prefix("rgba(").and_then(|s| s.strip_suffix(')')) {
        let parts: Vec<&str> = inner.split(',').map(|p| p.trim()).collect();
        if parts.len() != 4 {
            return None;
        }
        let r: u8 = parts[0].parse().ok()?;
        let g: u8 = parts[1].parse().ok()?;
        let b: u8 = parts[2].parse().ok()?;
        let a_f: f32 = parts[3].parse().ok()?;
        let a = (a_f.clamp(0.0, 1.0) * 255.0) as u8;
        return Some(Color::rgba(r, g, b, a));
    }
    if let Some(inner) = s.strip_prefix("rgb(").and_then(|s| s.strip_suffix(')')) {
        let parts: Vec<&str> = inner.split(',').map(|p| p.trim()).collect();
        if parts.len() != 3 {
            return None;
        }
        let r: u8 = parts[0].parse().ok()?;
        let g: u8 = parts[1].parse().ok()?;
        let b: u8 = parts[2].parse().ok()?;
        return Some(Color::rgb(r, g, b));
    }
    None
}
