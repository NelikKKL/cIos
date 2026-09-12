//! Безопасная Rust-обёртка над редактором `nano`, реализованным на C
//! (kernel/src/nano_c/nano.c + nano.h, мост в nano_ffi.rs) и
//! слинкованным в ядро как статическая библиотека через FFI (см.
//! kernel/build.rs). Публичный интерфейс (`Editor`, `Outcome`,
//! `MAIN_SHORTCUTS`) используется из `Mode::Nano` в main.rs и
//! `terminal.rs::draw_nano*` — сам редактор при этом хранит поля не
//! публично, а отдаёт их через методы (вызовы в C), и `Editor` владеет
//! `*mut RawNanoEditor`, а не Vec/String напрямую.
//!
//! Известный неподтверждённый риск: имена вариантов
//! `pc_keyboard::KeyCode::Home/End/PageUp/PageDown/Delete` не
//! перепроверялись офлайн — см. NANO.md.

use alloc::string::String;
use alloc::vec::Vec;
use core::ffi::c_char;

use pc_keyboard::{DecodedKey, KeyCode};

/// Непрозрачный тип на стороне C (struct NanoEditor из nano.c) — Rust
/// никогда не заглядывает внутрь, только хранит указатель.
#[repr(C)]
struct RawNanoEditor {
    _opaque: [u8; 0],
}

#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq)]
enum CRawKey {
    None = 0,
    ArrowLeft,
    ArrowRight,
    ArrowUp,
    ArrowDown,
    Home,
    End,
    PageUp,
    PageDown,
    Delete,
    Backspace,
    Escape,
}

/// Должна побайтово совпадать с NanoOutcome из nano.h.
#[repr(C)]
struct COutcome {
    should_exit: i32,
    exit_message: [c_char; 128],
}

extern "C" {
    fn nano_open(path: *const c_char) -> *mut RawNanoEditor;
    fn nano_editor_free(ed: *mut RawNanoEditor);
    fn nano_handle_key(
        ed: *mut RawNanoEditor,
        codepoint: u32,
        raw: CRawKey,
        is_raw: i32,
        ctrl: i32,
        alt: i32,
    ) -> COutcome;

    fn nano_line_count(ed: *const RawNanoEditor) -> usize;
    fn nano_line(ed: *const RawNanoEditor, index: usize) -> *const c_char;
    fn nano_cursor_line(ed: *const RawNanoEditor) -> usize;
    fn nano_cursor_col(ed: *const RawNanoEditor) -> usize;
    fn nano_is_modified(ed: *const RawNanoEditor) -> i32;
    fn nano_filename(ed: *const RawNanoEditor) -> *const c_char;
    fn nano_prompt_line(ed: *const RawNanoEditor, buf: *mut c_char, cap: usize);
    fn nano_prompt_has_cursor(ed: *const RawNanoEditor) -> i32;
    fn nano_help_active(ed: *const RawNanoEditor) -> i32;
    fn nano_help_line_count() -> usize;
    fn nano_help_line(index: usize) -> *const c_char;
}

pub enum Outcome {
    Continue,
    Exit(String),
}

/// Тот же набор возможностей, что и в оригинальном GNU nano (см.
/// NANO.md) — отсюда и тот же набор подсказок внизу экрана.
pub const MAIN_SHORTCUTS: &[(&str, &str)] = &[
    ("^G", "Get Help"),
    ("^X", "Exit"),
    ("^O", "Write Out"),
    ("^J", "Justify"),
    ("^R", "Read File"),
    ("^W", "Where Is"),
    ("^\\", "Replace"),
    ("^_", "Go To Line"),
    ("^K", "Cut Text"),
    ("^U", "Paste Text"),
    ("^C", "Cur Pos"),
    ("^^", "Mark Text"),
];

pub struct Editor {
    raw: *mut RawNanoEditor,
}

// Editor владеет только своей C-кучей (через тот же аллокатор ядра,
// что и остальной Rust-код, см. nano_ffi.rs) и не содержит ничего
// специфичного для потока/ядра — как обычный Box, просто над чужой (C)
// аллокацией, так что Send безопасен.
unsafe impl Send for Editor {}

fn nul_terminated(s: &str) -> Vec<u8> {
    let mut v = Vec::with_capacity(s.len() + 1);
    v.extend_from_slice(s.as_bytes());
    v.push(0);
    v
}

/// # Safety-примечание
/// Все `unsafe` ниже — это просто вызовы деклараций `extern "C"`,
/// сигнатуры которых должны совпадать с nano.h (см. предупреждение
/// выше о COutcome). Сам C-код (nano.c) не содержит `unsafe` в
/// Rust-смысле — вся ответственность за корректность памяти лежит на
/// его собственной, отдельно вычитанной, логике (см. NANO.md).
impl Editor {
    pub fn open(path: Option<&str>) -> Editor {
        let raw = match path {
            Some(p) => {
                let bytes = nul_terminated(p);
                unsafe { nano_open(bytes.as_ptr() as *const c_char) }
            }
            None => unsafe { nano_open(core::ptr::null()) },
        };
        Editor { raw }
    }

    pub fn handle_key(&mut self, key: DecodedKey, ctrl: bool, alt: bool) -> Outcome {
        let (codepoint, raw, is_raw): (u32, CRawKey, i32) = match key {
            DecodedKey::Unicode(c) => (c as u32, CRawKey::None, 0),
            DecodedKey::RawKey(code) => {
                let raw = match code {
                    KeyCode::ArrowLeft => CRawKey::ArrowLeft,
                    KeyCode::ArrowRight => CRawKey::ArrowRight,
                    KeyCode::ArrowUp => CRawKey::ArrowUp,
                    KeyCode::ArrowDown => CRawKey::ArrowDown,
                    KeyCode::Home => CRawKey::Home,
                    KeyCode::End => CRawKey::End,
                    KeyCode::PageUp => CRawKey::PageUp,
                    KeyCode::PageDown => CRawKey::PageDown,
                    KeyCode::Delete => CRawKey::Delete,
                    KeyCode::Backspace => CRawKey::Backspace,
                    KeyCode::Escape => CRawKey::Escape,
                    _ => CRawKey::None,
                };
                (0, raw, 1)
            }
        };

        let outcome = unsafe { nano_handle_key(self.raw, codepoint, raw, is_raw, ctrl as i32, alt as i32) };

        if outcome.should_exit != 0 {
            Outcome::Exit(c_array_to_string(&outcome.exit_message))
        } else {
            Outcome::Continue
        }
    }

    pub fn line_count(&self) -> usize {
        unsafe { nano_line_count(self.raw) }
    }

    pub fn line(&self, index: usize) -> &str {
        unsafe { c_ptr_to_str(nano_line(self.raw, index)) }
    }

    pub fn cursor_line(&self) -> usize {
        unsafe { nano_cursor_line(self.raw) }
    }

    pub fn cursor_col(&self) -> usize {
        unsafe { nano_cursor_col(self.raw) }
    }

    pub fn is_modified(&self) -> bool {
        unsafe { nano_is_modified(self.raw) != 0 }
    }

    pub fn filename(&self) -> Option<&str> {
        let ptr = unsafe { nano_filename(self.raw) };
        if ptr.is_null() {
            None
        } else {
            Some(unsafe { c_ptr_to_str(ptr) })
        }
    }

    /// Аналог nano::Editor::title_parts() — тот же трёхчастный формат
    /// заголовка, см. terminal.rs::draw_nano_panel.
    pub fn title_parts(&self) -> (&'static str, String, &'static str) {
        let prefix = if self.filename().is_some() { "File:" } else { "" };
        let path = self.filename().map(String::from).unwrap_or_else(|| String::from("New Buffer"));
        let state = if self.is_modified() { "Modified" } else { "" };
        (prefix, path, state)
    }

    /// Аналог nano::Editor::prompt_line().
    pub fn prompt_line(&self) -> String {
        let mut buf = [0 as c_char; 200];
        unsafe { nano_prompt_line(self.raw, buf.as_mut_ptr(), buf.len()) };
        c_array_to_string(&buf)
    }

    pub fn prompt_has_cursor(&self) -> bool {
        unsafe { nano_prompt_has_cursor(self.raw) != 0 }
    }

    pub fn help_active(&self) -> bool {
        unsafe { nano_help_active(self.raw) != 0 }
    }

    pub fn help_lines() -> Vec<&'static str> {
        let n = unsafe { nano_help_line_count() };
        (0..n).map(|i| unsafe { c_ptr_to_str(nano_help_line(i)) }).collect()
    }
}

impl Drop for Editor {
    fn drop(&mut self) {
        unsafe { nano_editor_free(self.raw) };
    }
}

unsafe fn c_ptr_to_str<'a>(ptr: *const c_char) -> &'a str {
    if ptr.is_null() {
        return "";
    }
    let mut len = 0usize;
    while *ptr.add(len) != 0 {
        len += 1;
    }
    let bytes = core::slice::from_raw_parts(ptr as *const u8, len);
    core::str::from_utf8(bytes).unwrap_or("")
}

fn c_array_to_string(buf: &[c_char]) -> String {
    let bytes: Vec<u8> = buf.iter().take_while(|&&c| c != 0).map(|&c| c as u8).collect();
    String::from_utf8_lossy(&bytes).into_owned()
}
