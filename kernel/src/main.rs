#![no_std]
#![no_main]
#![feature(abi_x86_interrupt)]

extern crate alloc;

mod bitmap;
mod css;
mod font;
mod fs;
mod framebuffer;
mod interrupts;
mod keyboard_queue;
mod memory;
mod nano;
mod nano_ffi;
mod serial;
mod shell;
mod terminal;
mod text;
mod theme;

use alloc::string::String;
use alloc::vec::Vec;
use core::panic::PanicInfo;
use framebuffer::Framebuffer;
use limine::request::FramebufferRequest;
use limine::{BaseRevision, RequestsEndMarker, RequestsStartMarker};
use pc_keyboard::{DecodedKey, KeyCode};

#[used]
#[link_section = ".requests"]
static BASE_REVISION: BaseRevision = BaseRevision::new();

#[used]
#[link_section = ".requests"]
static FRAMEBUFFER_REQUEST: FramebufferRequest = FramebufferRequest::new();

#[used]
#[link_section = ".requests_start_marker"]
static _START_MARKER: RequestsStartMarker = RequestsStartMarker::new();

#[used]
#[link_section = ".requests_end_marker"]
static _END_MARKER: RequestsEndMarker = RequestsEndMarker::new();

/// Режим главного цикла: обычный шелл или интерактивный файловый
/// менеджер (команда `file-sys`). У каждого — свой обработчик клавиш
/// и свой рендер (terminal::draw_desktop vs terminal::draw_file_manager).
enum Mode {
    Shell,
    FileManager { path: String, entries: Vec<String>, selected: usize },
    CssMenu { selected: usize },
    CssEditor { buffer: String },
    Nano(nano::Editor),
}

#[no_mangle]
extern "C" fn kmain() -> ! {
    serial::init();
    serial::print("CIOS: kernel entered\n");

    if !BASE_REVISION.is_supported() {
        serial::print("CIOS: bootloader does not support requested base revision\n");
        hcf();
    }

    let response = FRAMEBUFFER_REQUEST
        .response()
        .expect("bootloader did not answer FramebufferRequest");

    let fb_raw = response
        .framebuffers()
        .first()
        .copied()
        .expect("no framebuffer reported by bootloader");

    let mut fb = unsafe {
        Framebuffer::new(
            fb_raw.address() as *mut u8,
            fb_raw.width as usize,
            fb_raw.height as usize,
            fb_raw.pitch as usize,
            fb_raw.bpp as usize,
            fb_raw.red_mask_shift,
            fb_raw.green_mask_shift,
            fb_raw.blue_mask_shift,
        )
    };

    serial::print("CIOS: framebuffer acquired, drawing desktop\n");

    memory::init();
    fs::init();

    // История вывода терминала (растёт по мере выполнения команд) и
    // текущая директория шелла.
    let mut history: Vec<String> = Vec::new();
    history.push(String::from("CIOS - type 'help' for a list of commands"));
    let mut cwd = String::from("/");

    // Смени на theme::SOLID_THEME, чтобы увидеть разницу с непрозрачным
    // терминалом — обе темы живут в theme.rs. Команда `css` в шелле
    // переключает тему в рантайме (см. Mode::CssMenu/CssEditor ниже).
    let mut theme = theme::DEFAULT_THEME;
    terminal::draw_desktop(&mut fb, &theme, &history, "", 0);

    interrupts::init();
    serial::print("CIOS: entering event loop (type something, arrows move cursor)\n");

    // Строчный буфер ввода пока фиксированного размера на стеке — сама
    // история вывода уже на куче (Vec<String>, см. выше).
    let mut line_buf = [0u8; 63];
    let mut line_len: usize = 0;
    let mut cursor: usize = 0;
    let mut mode = Mode::Shell;

    loop {
        // Спим до следующего прерывания — просыпаемся на любое нажатие
        // клавиши (IRQ1), даже если таймер ещё не настроен (Phase 3+).
        x86_64::instructions::hlt();

        let mut dirty = false;
        let mut theme_changed = false;
        while let Some(evt) = keyboard_queue::pop() {
            serial::print("CIOS: main loop popped a key event\n");
            let key = evt.key;
            let ctrl = evt.ctrl;
            let alt = evt.alt;
            dirty = true;
            // Смена режима откладывается до конца итерации: нельзя
            // переприсвоить `mode`, пока внутри match всё ещё живут
            // заимствования его полей (path/entries/selected).
            let mut pending_mode: Option<Mode> = None;

            match &mut mode {
                Mode::Shell => match key {
                    DecodedKey::Unicode('\u{8}') | DecodedKey::RawKey(KeyCode::Backspace) => {
                        if cursor > 0 {
                            for i in (cursor - 1)..(line_len - 1) {
                                line_buf[i] = line_buf[i + 1];
                            }
                            line_len -= 1;
                            cursor -= 1;
                        }
                    }
                    DecodedKey::Unicode('\n') | DecodedKey::Unicode('\r') => {
                        let line = core::str::from_utf8(&line_buf[..line_len]).unwrap_or("");
                        if line.trim() == "file-sys" {
                            let path = fs::resolve(&cwd, ".");
                            let entries = fs::list(&path).unwrap_or_default();
                            pending_mode = Some(Mode::FileManager { path, entries, selected: 0 });
                        } else if line.trim() == "css" {
                            pending_mode = Some(Mode::CssMenu { selected: 0 });
                        } else if line.trim() == "nano" || line.trim().starts_with("nano ") {
                            let arg = line.trim().strip_prefix("nano").unwrap_or("").trim();
                            let path = if arg.is_empty() { None } else { Some(fs::resolve(&cwd, arg)) };
                            pending_mode = Some(Mode::Nano(nano::Editor::open(path.as_deref())));
                        } else {
                            history.push(alloc::format!("{cwd} > {line}"));
                            shell::execute(line, &mut cwd, &mut history);
                            const MAX_HISTORY: usize = 200;
                            if history.len() > MAX_HISTORY {
                                let excess = history.len() - MAX_HISTORY;
                                history.drain(0..excess);
                            }
                        }
                        line_len = 0;
                        cursor = 0;
                    }
                    DecodedKey::RawKey(KeyCode::ArrowLeft) => {
                        if cursor > 0 {
                            cursor -= 1;
                        }
                    }
                    DecodedKey::RawKey(KeyCode::ArrowRight) => {
                        if cursor < line_len {
                            cursor += 1;
                        }
                    }
                    DecodedKey::Unicode(c) if (c as u32) >= 0x20 && (c as u32) < 0x7F => {
                        if line_len < line_buf.len() {
                            for i in (cursor..line_len).rev() {
                                line_buf[i + 1] = line_buf[i];
                            }
                            line_buf[cursor] = c as u8;
                            line_len += 1;
                            cursor += 1;
                        }
                    }
                    _ => {}
                },
                Mode::FileManager { path, entries, selected } => match key {
                    DecodedKey::RawKey(KeyCode::ArrowUp) => {
                        *selected = selected.saturating_sub(1);
                    }
                    DecodedKey::RawKey(KeyCode::ArrowDown) => {
                        if !entries.is_empty() {
                            *selected = (*selected + 1).min(entries.len() - 1);
                        }
                    }
                    DecodedKey::Unicode('\n') | DecodedKey::Unicode('\r') => {
                        if let Some(name) = entries.get(*selected) {
                            if let Some(dir_name) = name.strip_suffix('/') {
                                *path = fs::resolve(path, dir_name);
                                *entries = fs::list(path).unwrap_or_default();
                                *selected = 0;
                            }
                            // Файл: превью — отдельная задача, пока не открываем.
                        }
                    }
                    DecodedKey::Unicode('\u{8}')
                    | DecodedKey::RawKey(KeyCode::Backspace)
                    | DecodedKey::RawKey(KeyCode::ArrowLeft) => {
                        *path = fs::resolve(path, "..");
                        *entries = fs::list(path).unwrap_or_default();
                        *selected = 0;
                    }
                    DecodedKey::RawKey(KeyCode::Escape) | DecodedKey::Unicode('q') => {
                        cwd = path.clone();
                        history.push(String::from("file-sys: closed"));
                        pending_mode = Some(Mode::Shell);
                    }
                    _ => {}
                },
                Mode::CssMenu { selected } => match key {
                    DecodedKey::RawKey(KeyCode::ArrowUp) => {
                        *selected = selected.saturating_sub(1);
                    }
                    DecodedKey::RawKey(KeyCode::ArrowDown) => {
                        *selected = (*selected + 1).min(terminal::CSS_MENU_OPTIONS.len() - 1);
                    }
                    DecodedKey::Unicode('\n') | DecodedKey::Unicode('\r') => match *selected {
                        0 => {
                            pending_mode = Some(Mode::CssEditor { buffer: String::new() });
                        }
                        1 => {
                            theme = theme::DEFAULT_THEME;
                            theme_changed = true;
                            history.push(String::from("theme: default"));
                            pending_mode = Some(Mode::Shell);
                        }
                        2 => {
                            theme = theme::AUTUMN_THEME;
                            theme_changed = true;
                            history.push(String::from("theme: autumn"));
                            pending_mode = Some(Mode::Shell);
                        }
                        3 => {
                            theme = theme::SNOW_THEME;
                            theme_changed = true;
                            history.push(String::from("theme: snow"));
                            pending_mode = Some(Mode::Shell);
                        }
                        _ => {}
                    },
                    DecodedKey::RawKey(KeyCode::Escape) | DecodedKey::Unicode('q') => {
                        pending_mode = Some(Mode::Shell);
                    }
                    _ => {}
                },
                Mode::CssEditor { buffer } => match key {
                    DecodedKey::Unicode('\u{8}') | DecodedKey::RawKey(KeyCode::Backspace) => {
                        buffer.pop();
                    }
                    DecodedKey::Unicode('\n') | DecodedKey::Unicode('\r') => {
                        buffer.push('\n');
                    }
                    DecodedKey::RawKey(KeyCode::Escape) => {
                        if buffer.trim().is_empty() {
                            history.push(String::from("css: cancelled"));
                        } else {
                            theme = css::parse(buffer, theme);
                            theme_changed = true;
                            history.push(String::from("css: custom theme applied"));
                        }
                        pending_mode = Some(Mode::Shell);
                    }
                    DecodedKey::Unicode(c) if (c as u32) >= 0x20 && (c as u32) < 0x7F => {
                        buffer.push(c);
                    }
                    _ => {}
                },
                Mode::Nano(editor) => match editor.handle_key(key, ctrl, alt) {
                    nano::Outcome::Continue => {}
                    nano::Outcome::Exit(message) => {
                        history.push(message);
                        pending_mode = Some(Mode::Shell);
                    }
                },
            }

            if let Some(new_mode) = pending_mode {
                mode = new_mode;
            }
        }

        if dirty {
            serial::print("CIOS: redraw start\n");
            if theme_changed {
                // Тема (а с ней обои/бар) поменялась — нужен полный кадр.
                match &mode {
                    Mode::Shell => {
                        let text = core::str::from_utf8(&line_buf[..line_len]).unwrap_or("");
                        terminal::draw_desktop(&mut fb, &theme, &history, text, cursor);
                    }
                    Mode::FileManager { path, entries, selected } => {
                        terminal::draw_file_manager(&mut fb, &theme, path, entries, *selected);
                    }
                    Mode::CssMenu { selected } => {
                        terminal::draw_css_menu(&mut fb, &theme, *selected);
                    }
                    Mode::CssEditor { buffer } => {
                        terminal::draw_css_editor(&mut fb, &theme, buffer);
                    }
                    Mode::Nano(editor) => {
                        terminal::draw_nano(&mut fb, &theme, editor);
                    }
                }
            } else {
                // Обычный ввод — обои/бар не менялись, трогаем только
                // область панели. Это и есть исправление лага: раньше
                // здесь перерисовывался весь экран на каждую клавишу.
                match &mode {
                    Mode::Shell => {
                        let text = core::str::from_utf8(&line_buf[..line_len]).unwrap_or("");
                        terminal::draw_terminal_panel(&mut fb, &theme, &history, text, cursor);
                    }
                    Mode::FileManager { path, entries, selected } => {
                        terminal::draw_file_manager_panel(&mut fb, &theme, path, entries, *selected);
                    }
                    Mode::CssMenu { selected } => {
                        terminal::draw_css_menu_panel(&mut fb, &theme, *selected);
                    }
                    Mode::CssEditor { buffer } => {
                        terminal::draw_css_editor_panel(&mut fb, &theme, buffer);
                    }
                    Mode::Nano(editor) => {
                        terminal::draw_nano_panel(&mut fb, &theme, editor);
                    }
                }
            }
            serial::print("CIOS: redraw done\n");
        }
    }
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    serial::print("CIOS PANIC: ");
    serial::print_fmt(info);
    hcf();
}

/// "Halt and catch fire" — ждём прерываний в hlt; прерывания у нас
/// пока всё равно выключены, так что это просто останов.
fn hcf() -> ! {
    loop {
        unsafe {
            core::arch::asm!("hlt");
        }
    }
}
