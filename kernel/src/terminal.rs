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

/// Ключи в панели горячих клавиш рисуются тем же цветом курсора темы
/// (лёгкий акцент), чтобы отличаться от описания — как выделение
/// ключа отдельным цветом в onekey() оригинала.
fn inverse_fg_shortcut(theme: &Theme) -> Color {
    theme.cursor
}

/// Полный кадр nano-режима (обои + бар + панель редактора). Только для
/// первого кадра после входа в режим — дальше см. draw_nano_panel.
pub fn draw_nano(fb: &mut Framebuffer, theme: &Theme, editor: &crate::nano::Editor) {
    draw_wallpaper(fb, theme);
    draw_bar(fb, theme);
    draw_nano_panel(fb, theme, editor);
}

/// Редактор `nano`: title bar сверху, тело буфера посередине,
/// статус-строка/активный prompt + панель горячих клавиш снизу — тот
/// же трёхчастный макет, что и в настоящем nano (titlebar()/edit
/// window/bottombars() в winio.c оригинала). Только область панели —
/// лёгкая перерисовка на каждое нажатие.
pub fn draw_nano_panel(fb: &mut Framebuffer, theme: &Theme, editor: &crate::nano::Editor) {
    let (x0, y0, w, h) = panel_geometry(fb, theme);
    fb.fill_rect_blended(x0, y0, w, h, theme.terminal_bg);

    let f = font::font();
    let inner_pad = 12;
    let text_x = x0 + inner_pad;
    let usable_h = h.saturating_sub(inner_pad * 2);
    let usable_w = w.saturating_sub(inner_pad * 2);
    let max_rows = (usable_h / f.cell_h).max(1);
    let max_cols = (usable_w / f.cell_w).max(1);

    let inverse_fg = Color::rgb(theme.terminal_bg.r, theme.terminal_bg.g, theme.terminal_bg.b);

    fb.fill_rect(x0, y0, w, f.cell_h, theme.cursor);
    let (prefix, path, state) = editor.title_parts();
    let mut title = alloc::format!("cIos nano-clone    {prefix}{}{path}", if prefix.is_empty() { "" } else { " " });
    if !state.is_empty() {
        title = alloc::format!("{title}   [{state}]");
    }
    let title_cols = title.chars().count().min(max_cols);
    let title_x = x0 + inner_pad + usable_w.saturating_sub(title_cols * f.cell_w) / 2;
    text::draw_text(fb, &f, title_x, y0 + inner_pad, clip_to_cols(&title, max_cols), inverse_fg);

    let reserved_bottom = 3usize.min(max_rows.saturating_sub(1));
    let bottom_y = y0 + h.saturating_sub(inner_pad) - reserved_bottom * f.cell_h;

    let status_text = editor.prompt_line();
    text::draw_text(fb, &f, text_x, bottom_y, clip_to_cols(&status_text, max_cols), theme.terminal_fg);
    if editor.prompt_has_cursor() {
        let cx = text_x + status_text.chars().count().min(max_cols) * f.cell_w;
        fb.fill_rect(cx, bottom_y, f.cell_w, f.cell_h, theme.cursor);
    }

    if reserved_bottom >= 3 {
        let shortcuts_y = bottom_y + f.cell_h;
        let cols_per_row = (crate::nano::MAIN_SHORTCUTS.len() + 1) / 2;
        let cell = usable_w / cols_per_row.max(1);
        for (i, item) in crate::nano::MAIN_SHORTCUTS.iter().enumerate() {
            let (key, desc) = *item;
            let row = i % 2;
            let col = i / 2;
            let cx = text_x + col * cell;
            let cy = shortcuts_y + row * f.cell_h;
            text::draw_text(fb, &f, cx, cy, key, inverse_fg_shortcut(theme));
            let key_cols = key.chars().count() + 1;
            text::draw_text(
                fb,
                &f,
                cx + key_cols * f.cell_w,
                cy,
                clip_to_cols(desc, cell.saturating_sub(key_cols * f.cell_w).max(1) / f.cell_w.max(1)),
                theme.terminal_fg,
            );
        }
    }

    let body_top = y0 + inner_pad + f.cell_h + 4;
    let body_rows = if bottom_y > body_top { (bottom_y - body_top) / f.cell_h } else { 0 };
    let mut y = body_top;

    if editor.help_active() {
        for line in crate::nano::Editor::help_lines().into_iter().take(body_rows) {
            text::draw_text(fb, &f, text_x, y, clip_to_cols(line, max_cols), theme.terminal_fg);
            y += f.cell_h;
        }
        return;
    }

    if body_rows == 0 {
        return;
    }

    let cursor_line = editor.cursor_line();
    let cursor_col = editor.cursor_col();
    let line_count = editor.line_count();

    let start = if cursor_line >= body_rows { cursor_line + 1 - body_rows } else { 0 };
    let end = line_count.min(start + body_rows);

    let left = if cursor_col >= max_cols { cursor_col + 1 - max_cols } else { 0 };

    for li in start..end {
        let line = editor.line(li);
        let chars: Vec<char> = line.chars().collect();
        let visible_end = (left + max_cols).min(chars.len());
        let mut shown: String = if left < chars.len() {
            chars[left..visible_end].iter().collect()
        } else {
            String::new()
        };
        if left > 0 && !shown.is_empty() {
            let first_len = shown.chars().next().unwrap().len_utf8();
            shown.replace_range(0..first_len, "$");
        }
        if chars.len() > left + shown.chars().count() {
            if let Some((last_byte, last_char)) = shown.char_indices().last() {
                shown.replace_range(last_byte..last_byte + last_char.len_utf8(), "$");
            }
        }
        text::draw_text(fb, &f, text_x, y, &shown, theme.terminal_fg);

        if li == cursor_line {
            let cursor_screen_col = cursor_col.saturating_sub(left);
            fb.fill_rect(text_x + cursor_screen_col * f.cell_w, y, f.cell_w, f.cell_h, theme.cursor);
        }
        y += f.cell_h;
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
