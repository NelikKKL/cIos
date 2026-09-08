//! Рендер "рабочего стола": обои + полупрозрачная панель терминала + бар.
//! Панель теперь показывает настоящую прокручиваемую историю вывода
//! шелла (fs.rs/shell.rs), а не два захардкоженных демо-текста.

use alloc::string::String;

use crate::font;
use crate::framebuffer::Framebuffer;
use crate::text;
use crate::theme::{BarPosition, Color, Theme, Wallpaper};

pub fn draw_desktop(
    fb: &mut Framebuffer,
    theme: &Theme,
    history: &[String],
    prompt_text: &str,
    prompt_cursor: usize,
) {
    draw_wallpaper(fb, theme);
    draw_bar(fb, theme);
    draw_terminal_panel(fb, theme, history, prompt_text, prompt_cursor);
}

fn draw_wallpaper(fb: &mut Framebuffer, theme: &Theme) {
    match theme.wallpaper {
        Wallpaper::Solid(color) => {
            fb.fill_rect(0, 0, fb.width, fb.height, color);
        }
        Wallpaper::VerticalGradient(top, bottom) => {
            for y in 0..fb.height {
                let t = y as u32 * 255 / fb.height.max(1) as u32;
                let r = (top.r as u32 * (255 - t) + bottom.r as u32 * t) / 255;
                let g = (top.g as u32 * (255 - t) + bottom.g as u32 * t) / 255;
                let b = (top.b as u32 * (255 - t) + bottom.b as u32 * t) / 255;
                let color = Color::rgb(r as u8, g as u8, b as u8);
                for x in 0..fb.width {
                    fb.put_pixel(x, y, color);
                }
            }
        }
    }
}

fn draw_bar(fb: &mut Framebuffer, theme: &Theme) {
    let h = theme.bar.height_px as usize;
    let y0 = match theme.bar.position {
        BarPosition::Top => 0,
        BarPosition::Bottom => fb.height.saturating_sub(h),
        BarPosition::Hidden => return,
    };
    fb.fill_rect_blended(0, y0, fb.width, h, theme.bar.background);
}

fn draw_terminal_panel(
    fb: &mut Framebuffer,
    theme: &Theme,
    history: &[String],
    prompt_text: &str,
    prompt_cursor: usize,
) {
    let pad = theme.terminal_padding_px as usize;
    let bar_h = match theme.bar.position {
        BarPosition::Hidden => 0,
        _ => theme.bar.height_px as usize,
    };
    let x0 = pad;
    let y0 = pad;
    let w = fb.width.saturating_sub(pad * 2);
    let h = fb.height.saturating_sub(pad * 2 + bar_h);

    // Полупрозрачная панель — здесь и работает theme.terminal_bg.a
    fb.fill_rect_blended(x0, y0, w, h, theme.terminal_bg);

    let f = font::font();
    let inner_pad = 12;
    let text_x = x0 + inner_pad;
    let usable_h = h.saturating_sub(inner_pad * 2);
    let usable_w = w.saturating_sub(inner_pad * 2);
    let max_rows = (usable_h / f.cell_h).max(1);
    let max_cols = (usable_w / f.cell_w).max(1);

    // Последняя строка зарезервирована под приглашение ввода.
    let history_rows = max_rows.saturating_sub(1);
    let start = history.len().saturating_sub(history_rows);

    let mut text_y = y0 + inner_pad;
    for line in &history[start..] {
        text::draw_text(fb, &f, text_x, text_y, clip_to_cols(line, max_cols), theme.terminal_fg);
        text_y += f.cell_h;
    }

    let prompt = "> ";
    text::draw_text(fb, &f, text_x, text_y, prompt, theme.terminal_fg);
    let input_x = text_x + prompt.chars().count() * f.cell_w;
    let input_max_cols = max_cols.saturating_sub(prompt.chars().count());
    text::draw_text(fb, &f, input_x, text_y, clip_to_cols(prompt_text, input_max_cols), theme.terminal_fg);

    // Блочный курсор на позиции prompt_cursor внутри введённой строки —
    // им управляют стрелки влево/вправо (см. main.rs).
    let cursor_x = input_x + prompt_cursor.min(input_max_cols) * f.cell_w;
    fb.fill_rect(cursor_x, text_y, f.cell_w, f.cell_h, theme.cursor);
}

/// Обрезает строку до max_cols символов (без выделения памяти) —
/// защита от вылезания текста за пределы панели.
fn clip_to_cols(s: &str, max_cols: usize) -> &str {
    match s.char_indices().nth(max_cols) {
        Some((byte_idx, _)) => &s[..byte_idx],
        None => s,
    }
}
