#![no_std]
#![no_main]

mod font;
mod framebuffer;
mod interrupts;
mod keyboard_queue;
mod serial;
mod terminal;
mod text;
mod theme;

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

    // Смени на theme::SOLID_THEME, чтобы увидеть разницу с непрозрачным
    // терминалом — обе темы живут в theme.rs.
    let theme = &theme::DEFAULT_THEME;
    terminal::draw_desktop(&mut fb, theme, "", 0);

    interrupts::init();
    serial::print("CIOS: entering event loop (type something, arrows move cursor)\n");

    // Простой строчный редактор без кучи (Phase 3 ещё впереди) — буфер
    // фиксированного размера прямо на стеке кадра kmain.
    let mut line_buf = [0u8; 63];
    let mut line_len: usize = 0;
    let mut cursor: usize = 0;

    loop {
        // Спим до следующего прерывания — просыпаемся на любое нажатие
        // клавиши (IRQ1), даже если таймер ещё не настроен (Phase 3+).
        x86_64::instructions::hlt();

        let mut dirty = false;
        while let Some(key) = keyboard_queue::pop() {
            dirty = true;
            match key {
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
                    serial::print("CIOS: line: ");
                    if let Ok(s) = core::str::from_utf8(&line_buf[..line_len]) {
                        serial::print(s);
                    }
                    serial::print("\n");
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
            }
        }

        if dirty {
            let text = core::str::from_utf8(&line_buf[..line_len]).unwrap_or("");
            terminal::draw_desktop(&mut fb, theme, text, cursor);
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
