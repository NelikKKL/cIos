//! Рендер "рабочего стола": обои + полупрозрачная панель терминала + бар.
//!
//! Важно для производительности: обои и бар перерисовываются только
//! когда меняется тема (см. `draw_*` функции без суффикса — вызываются
//! из main.rs при первом кадре и при смене темы через `css`). На
//! обычный ввод (печать, стрелки) main.rs вызывает `draw_*_panel` —
//! они трогают только область панели, а не весь экран. Раньше на
//! каждое нажатие клавиши перерисовывался весь framebuffer (обои +
//! alpha-блендинг бара + панели), что на некоторых разрешениях/машинах
//! ощущалось как зависание клавиатуры — на деле клавиатура работала,
//! просто перерисовка не поспевала.

use alloc::string::String;
use alloc::vec::Vec;

use crate::font;
use crate::framebuffer::Framebuffer;
use crate::text;
use crate::theme::{BarPosition, Color, PanelShape, Theme, Wallpaper};

/// Полный кадр: обои + бар + панель терминала. Вызывать только на
/// первом кадре и когда меняется тема — иначе см. draw_terminal_panel.
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
        Wallpaper::Bitmap(bmp) => {
            bmp.blit_scaled(fb);
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

/// Общая геометрия панели (все режимы рисуют один и тот же
/// полупрозрачный прямоугольник, просто наполняют его по-разному). При
/// PanelShape::Square панель — центрированный квадрат, а не во всю
/// ширину экрана (тема Default: "сделай терминал квадратным").
fn panel_geometry(fb: &Framebuffer, theme: &Theme) -> (usize, usize, usize, usize) {
    let pad = theme.terminal_padding_px as usize;
    let bar_h = match theme.bar.position {
        BarPosition::Hidden => 0,
        _ => theme.bar.height_px as usize,
    };
    let avail_w = fb.width.saturating_sub(pad * 2);
    let avail_h = fb.height.saturating_sub(pad * 2 + bar_h);

    match theme.panel_shape {
        PanelShape::Fill => (pad, pad, avail_w, avail_h),
        PanelShape::Square => {
            let side = avail_w.min(avail_h);
            let x0 = pad + (avail_w.saturating_sub(side)) / 2;
            let y0 = pad + (avail_h.saturating_sub(side)) / 2;
            (x0, y0, side, side)
        }
    }
}

/// Только содержимое панели шелла (без обоев/бара) — это и есть
/// "лёгкая" перерисовка на каждое нажатие клавиши.
pub fn draw_terminal_panel(
    fb: &mut Framebuffer,
    theme: &Theme,
    history: &[String],
    prompt_text: &str,
    prompt_cursor: usize,
) {
    let (x0, y0, w, h) = panel_geometry(fb, theme);

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

/// Полный кадр файлового менеджера (обои + бар + список). Только для
/// первого кадра после входа в режим — дальше см. draw_file_manager_panel.
pub fn draw_file_manager(fb: &mut Framebuffer, theme: &Theme, path: &str, entries: &[String], selected: usize) {
    draw_wallpaper(fb, theme);
    draw_bar(fb, theme);
    draw_file_manager_panel(fb, theme, path, entries, selected);
}

/// Интерактивный файловый менеджер (команда `file-sys`): список файлов
/// текущей директории, выбранная строка подсвечена. Только область
/// панели — лёгкая перерисовка на каждое нажатие (стрелки и т.п.).
pub fn draw_file_manager_panel(fb: &mut Framebuffer, theme: &Theme, path: &str, entries: &[String], selected: usize) {
    let (x0, y0, w, h) = panel_geometry(fb, theme);
    fb.fill_rect_blended(x0, y0, w, h, theme.terminal_bg);

    let f = font::font();
    let inner_pad = 12;
    let text_x = x0 + inner_pad;
    let usable_h = h.saturating_sub(inner_pad * 2);
    let usable_w = w.saturating_sub(inner_pad * 2);
    let max_cols = (usable_w / f.cell_w).max(1);
    let max_rows = (usable_h / f.cell_h).max(1);

    let mut y = y0 + inner_pad;
    let header = alloc::format!("file-sys: {path}  (arrows/enter/backspace/esc)");
    text::draw_text(fb, &f, text_x, y, clip_to_cols(&header, max_cols), theme.terminal_fg);
    y += f.cell_h + 4;

    // Текст выделенной строки рисуем цветом фона панели поверх заливки
    // курсорным цветом — простая, но обычно достаточно контрастная
    // инверсия без отдельного поля в теме под "цвет выделения".
    let highlight_fg = Color::rgb(theme.terminal_bg.r, theme.terminal_bg.g, theme.terminal_bg.b);

    let list_rows = max_rows.saturating_sub(2);
    let start = if selected >= list_rows { selected + 1 - list_rows } else { 0 };
    let end = entries.len().min(start + list_rows);

    if entries.is_empty() {
        text::draw_text(fb, &f, text_x + f.cell_w, y, "(empty)", theme.terminal_fg);
        return;
    }

    for (i, name) in entries[start..end].iter().enumerate() {
        let idx = start + i;
        let clipped = clip_to_cols(name, max_cols.saturating_sub(2));
        if idx == selected {
            fb.fill_rect(text_x, y, usable_w, f.cell_h, theme.cursor);
            text::draw_text(fb, &f, text_x + f.cell_w, y, clipped, highlight_fg);
        } else {
            text::draw_text(fb, &f, text_x + f.cell_w, y, clipped, theme.terminal_fg);
        }
        y += f.cell_h;
    }
}

/// Обрезает строку до max_cols символов (без выделения памяти) —
/// защита от вылезания текста за пределы панели.
fn clip_to_cols(s: &str, max_cols: usize) -> &str {
    match s.char_indices().nth(max_cols) {
        Some((byte_idx, _)) => &s[..byte_idx],
        None => s,
    }
}

/// Пункты меню команды `css` — порядок важен, main.rs индексирует по нему.
pub const CSS_MENU_OPTIONS: &[&str] = &["Create custom CSS", "Default", "Autumn", "Snow"];

/// Полный кадр меню `css` (обои + бар + список). Только для первого
/// кадра после входа в режим — дальше см. draw_css_menu_panel.
pub fn draw_css_menu(fb: &mut Framebuffer, theme: &Theme, selected: usize) {
    draw_wallpaper(fb, theme);
    draw_bar(fb, theme);
    draw_css_menu_panel(fb, theme, selected);
}

/// Меню выбора темы (команда `css`): список из CSS_MENU_OPTIONS,
/// выбранный пункт подсвечен. Только область панели.
pub fn draw_css_menu_panel(fb: &mut Framebuffer, theme: &Theme, selected: usize) {
    let (x0, y0, w, h) = panel_geometry(fb, theme);
    fb.fill_rect_blended(x0, y0, w, h, theme.terminal_bg);

    let f = font::font();
    let inner_pad = 12;
    let text_x = x0 + inner_pad;
    let usable_w = w.saturating_sub(inner_pad * 2);
    let max_cols = (usable_w / f.cell_w).max(1);

    let mut y = y0 + inner_pad;
    text::draw_text(fb, &f, text_x, y, clip_to_cols("css: choose a theme (arrows/enter)", max_cols), theme.terminal_fg);
    y += f.cell_h + 4;

    let highlight_fg = Color::rgb(theme.terminal_bg.r, theme.terminal_bg.g, theme.terminal_bg.b);
    for (i, label) in CSS_MENU_OPTIONS.iter().enumerate() {
        let clipped = clip_to_cols(label, max_cols.saturating_sub(2));
        if i == selected {
            fb.fill_rect(text_x, y, usable_w, f.cell_h, theme.cursor);
            text::draw_text(fb, &f, text_x + f.cell_w, y, clipped, highlight_fg);
        } else {
            text::draw_text(fb, &f, text_x + f.cell_w, y, clipped, theme.terminal_fg);
        }
        y += f.cell_h;
    }
}

/// Полный кадр редактора CSS (обои + бар + текст). Только для первого
/// кадра после входа в режим — дальше см. draw_css_editor_panel.
pub fn draw_css_editor(fb: &mut Framebuffer, theme: &Theme, buffer: &str) {
    draw_wallpaper(fb, theme);
    draw_bar(fb, theme);
    draw_css_editor_panel(fb, theme, buffer);
}

/// Редактор произвольного CSS (пункт "Create custom CSS"): многострочный
/// буфер (Enter вставляет перевод строки), курсор всегда в конце
/// последней строки. Только область панели.
pub fn draw_css_editor_panel(fb: &mut Framebuffer, theme: &Theme, buffer: &str) {
    let (x0, y0, w, h) = panel_geometry(fb, theme);
    fb.fill_rect_blended(x0, y0, w, h, theme.terminal_bg);

    let f = font::font();
    let inner_pad = 12;
    let text_x = x0 + inner_pad;
    let usable_h = h.saturating_sub(inner_pad * 2);
    let usable_w = w.saturating_sub(inner_pad * 2);
    let max_rows = (usable_h / f.cell_h).max(1);
    let max_cols = (usable_w / f.cell_w).max(1);

    let mut y = y0 + inner_pad;
    text::draw_text(
        fb,
        &f,
        text_x,
        y,
        clip_to_cols("css editor - Escape applies (empty+Escape cancels)", max_cols),
        theme.terminal_fg,
    );
    y += f.cell_h + 4;

    let lines: Vec<&str> = buffer.split('\n').collect();
    let body_rows = max_rows.saturating_sub(2).max(1);
    let start = lines.len().saturating_sub(body_rows);
    for line in &lines[start..] {
        text::draw_text(fb, &f, text_x, y, clip_to_cols(line, max_cols), theme.terminal_fg);
        y += f.cell_h;
    }

    // Курсор — в конце последней показанной строки.
    let last_line = lines[start..].last().copied().unwrap_or("");
    let cursor_col = last_line.chars().count().min(max_cols);
    let cursor_x = text_x + cursor_col * f.cell_w;
    let cursor_y = y.saturating_sub(f.cell_h);
    fb.fill_rect(cursor_x, cursor_y, f.cell_w, f.cell_h, theme.cursor);
}
