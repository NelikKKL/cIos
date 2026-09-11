//! FFI-мост между Rust-ядром и C-версией nano (kernel/src/nano_c/*.c).
//! Экспортирует функции, которые C-код (nano_c/nano.c, nanolibc.c)
//! вызывает как `extern`: аллокатор и доступ к fs::. У C и Rust одна и
//! та же куча (linked_list_allocator, см. memory.rs) — cios_alloc
//! просто делегирует в тот же global allocator, никакой отдельной кучи
//! для C не заводится.
//!
//! ВАЖНО про владение памятью (см. также комментарии в nano_c/nano.c):
//! cios_alloc(size) и cios_free(ptr, size) ОБЯЗАНЫ вызываться с ровно
//! тем же size, иначе порча кучи — это гарантирует сам C-код
//! (nl_malloc/nl_free в nanolibc.c прячут размер в заголовке блока
//! именно для того, чтобы остальной nano.c мог не думать об этом).

use alloc::alloc::{alloc, dealloc, Layout};
use core::ffi::{c_char, c_int};
use core::slice;
use core::str;

use crate::fs;

/// Выравнивание фиксировано в 8 байт — с запасом достаточно для всего,
/// что реально аллоцирует nano.c (char, char*, struct NanoEditor).
const ALIGN: usize = 8;

#[no_mangle]
pub extern "C" fn cios_alloc(size: usize) -> *mut u8 {
    // GlobalAlloc::alloc — это unsafe fn именно потому, что вызов с
    // size == 0 является неопределённым поведением, поэтому его нужно
    // явно отсекать здесь, а не полагаться на то, что C-сторона никогда
    // так не сделает.
    if size == 0 {
        return core::ptr::null_mut();
    }
    match Layout::from_size_align(size, ALIGN) {
        Ok(layout) => unsafe { alloc(layout) },
        Err(_) => core::ptr::null_mut(),
    }
}

/// ptr и size ОБЯЗАНЫ быть точно теми же, что были переданы в
/// cios_alloc(), которая вернула этот ptr.
#[no_mangle]
pub extern "C" fn cios_free(ptr: *mut u8, size: usize) {
    if ptr.is_null() || size == 0 {
        return;
    }
    if let Ok(layout) = Layout::from_size_align(size, ALIGN) {
        unsafe { dealloc(ptr, layout) }
    }
}

/// Читает файл в буфер, выделенный ЧЕРЕЗ cios_alloc(len) — вызывающая
/// C-сторона обязана освободить его РОВНО cios_free(ptr, len) (не
/// nl_free!). Возвращает NULL, если файла нет/ошибка чтения, ИЛИ если
/// файл существует, но пуст (см. ниже) — nano_open() в nano.c и так
/// корректно трактует NULL как "нет содержимого", подставляя одну
/// пустую строку, так что различать эти два случая смысла нет, а
/// выделять 0-байтовый блок нельзя: GlobalAlloc::alloc с size==0 — тоже
/// неопределённое поведение.
///
/// # Safety
/// `path` должен быть валидным NUL-terminated UTF-8 указателем,
/// `out_len` — валидным для записи `usize`.
#[no_mangle]
pub unsafe extern "C" fn cios_fs_read(path: *const c_char, out_len: *mut usize) -> *mut u8 {
    if path.is_null() || out_len.is_null() {
        return core::ptr::null_mut();
    }
    let path_str = match c_str_to_str(path) {
        Some(s) => s,
        None => return core::ptr::null_mut(),
    };
    match fs::read(path_str) {
        Ok(bytes) if !bytes.is_empty() => {
            let len = bytes.len();
            let layout = match Layout::from_size_align(len, ALIGN) {
                Ok(l) => l,
                Err(_) => return core::ptr::null_mut(),
            };
            let buf = alloc(layout);
            if buf.is_null() {
                return core::ptr::null_mut();
            }
            core::ptr::copy_nonoverlapping(bytes.as_ptr(), buf, len);
            *out_len = len;
            buf
        }
        _ => core::ptr::null_mut(),
    }
}

/// # Safety
/// `path` должен быть валидным NUL-terminated UTF-8 указателем; `data`
/// должен указывать на минимум `len` инициализированных байт (или
/// len == 0, тогда `data` может не разыменовываться).
#[no_mangle]
pub unsafe extern "C" fn cios_fs_write(path: *const c_char, data: *const u8, len: usize) -> c_int {
    let path_str = match c_str_to_str(path) {
        Some(s) => s,
        None => return -1,
    };
    let content: &[u8] = if len == 0 { &[] } else { slice::from_raw_parts(data, len) };
    match fs::write(path_str, content) {
        Ok(()) => 0,
        Err(_) => -1,
    }
}

unsafe fn c_str_to_str<'a>(ptr: *const c_char) -> Option<&'a str> {
    let mut len = 0usize;
    while *ptr.add(len) != 0 {
        len += 1;
    }
    let bytes = slice::from_raw_parts(ptr as *const u8, len);
    str::from_utf8(bytes).ok()
}
