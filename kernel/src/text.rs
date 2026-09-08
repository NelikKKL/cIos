//! Рисование текста в framebuffer запечённым атласом шрифта.

use crate::font::Font;
use crate::framebuffer::Framebuffer;
use crate::theme::Color;

/// Рисует строку `text` моноширинным шрифтом, (x0, y0) — левый
/// верхний угол первой ячейки. Символы вне атласа (кириллица, иконки
/// Nerd Font и т.п.) сейчас молча пропускаются как пробел — полный
/// набор глифов приедет отдельным шагом (нужно решить, какие
/// диапазоны кодовых точек запекать, атлас на всё не резиновый).
pub fn draw_text(fb: &mut Framebuffer, font: &Font, x0: usize, y0: usize, text: &str, fg: Color) {
    let mut cursor_x = x0;
    for ch in text.chars() {
        if let Some(glyph) = font.glyph(ch) {
            for gy in 0..font.cell_h {
                for gx in 0..font.cell_w {
                    let coverage = glyph[gy * font.cell_w + gx];
                    if coverage == 0 {
                        continue;
                    }
                    let px_color = Color::rgba(fg.r, fg.g, fg.b, coverage);
                    fb.blend_pixel(cursor_x + gx, y0 + gy, px_color);
                }
            }
        }
        cursor_x += font.cell_w;
    }
}
