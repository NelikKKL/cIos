//! Рендер "рабочего стола": обои + полупрозрачная панель терминала + бар.
//!
//! Реального рендера текста здесь ещё нет — это Phase 4, потребует
//! запечь шрифт AdwaitaMono Nerd Font в битмап-атлас на этапе сборки.
//! Пока панель рисуется пустой, с плейсхолдер-курсором — этого достаточно,
//! чтобы увидеть эффект темы (обои + прозрачность) вживую в QEMU.

use crate::font;
use crate::framebuffer::Framebuffer;
use crate::text;
use crate::theme::{BarPosition, Color, Theme, Wallpaper};

pub fn draw_desktop(fb: &mut Framebuffer, theme: &Theme) {
    draw_wallpaper(fb, theme);
    draw_bar(fb, theme);
    draw_terminal_panel(fb, theme);
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

fn draw_terminal_panel(fb: &mut Framebuffer, theme: &Theme) {
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
    let mut text_y = y0 + inner_pad;
    // Атлас пока содержит только ASCII (0x20..=0x7E) — кириллица и
    // иконки Nerd Font приедут отдельным шагом запекания.
    text::draw_text(fb, &f, text_x, text_y, "CIOS - hello, world", theme.terminal_fg);
    text_y += f.cell_h + 6;
    text::draw_text(fb, &f, text_x, text_y, "wallpaper + transparent panel: OK", theme.terminal_fg);
    text_y += f.cell_h + 6;
    text::draw_text(fb, &f, text_x, text_y, "> ", theme.terminal_fg);
    // Блочный курсор сразу после приглашения "> ".
    fb.fill_rect(text_x + 2 * f.cell_w, text_y, f.cell_w, f.cell_h, theme.cursor);
}
