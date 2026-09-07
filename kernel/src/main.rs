#![no_std]
#![no_main]

mod framebuffer;
mod serial;
mod terminal;
mod theme;

use core::panic::PanicInfo;
use framebuffer::Framebuffer;
use limine::request::FramebufferRequest;
use limine::{BaseRevision, RequestsEndMarker, RequestsStartMarker};

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
    terminal::draw_desktop(&mut fb, &theme::DEFAULT_THEME);

    serial::print("CIOS: desktop drawn, halting\n");
    hcf();
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
