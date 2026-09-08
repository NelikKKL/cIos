//! Физическая память: usable-регионы из Limine memmap + HHDM-смещение
//! (чтобы обращаться к физической памяти как к обычным адресам — Limine
//! уже маппит всю usable-память по offset+base), плюс глобальный
//! аллокатор кучи (linked_list_allocator) поверх одного usable-региона.
//!
//! Полноценного менеджера физических фреймов/страниц пока нет — куче
//! пока хватает одного региона. Понадобится, когда придётся управлять
//! памятью для чего-то ещё (например, user-space, далеко впереди).

use limine::memmap;
use limine::request::{HhdmRequest, MemmapRequest};
use linked_list_allocator::LockedHeap;

use crate::serial;

#[used]
#[link_section = ".requests"]
static HHDM_REQUEST: HhdmRequest = HhdmRequest::new();

#[used]
#[link_section = ".requests"]
static MEMMAP_REQUEST: MemmapRequest = MemmapRequest::new();

/// Размер кучи ядра. Пока фиксированный — если понадобится больше
/// (например, файловая система в ОЗУ, Phase 6), надо будет либо взять
/// регион побольше, либо расширять кучу несколькими регионами.
const HEAP_SIZE: usize = 4 * 1024 * 1024; // 4 MiB

#[global_allocator]
static ALLOCATOR: LockedHeap = LockedHeap::empty();

/// Находит usable-регион не меньше HEAP_SIZE и отдаёт его под кучу.
/// Вызывать один раз, после serial::init() (для логов) и до первого
/// использования alloc::vec::Vec / alloc::string::String где-либо в ядре.
pub fn init() {
    let hhdm_offset = HHDM_REQUEST
        .response()
        .expect("bootloader did not answer HhdmRequest")
        .offset;

    let memmap_resp = MEMMAP_REQUEST
        .response()
        .expect("bootloader did not answer MemmapRequest");

    let region = memmap_resp
        .entries()
        .iter()
        .find(|e| e.type_ == memmap::MEMMAP_USABLE && e.length as usize >= HEAP_SIZE)
        .expect("no usable memory region large enough for the heap");

    let heap_start = (region.base + hhdm_offset) as usize as *mut u8;

    unsafe {
        ALLOCATOR.lock().init(heap_start, HEAP_SIZE);
    }

    serial::print("CIOS: heap initialized (4 MiB)\n");
}
