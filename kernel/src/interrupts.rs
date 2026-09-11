//! Прерывания: IDT, обработчики исключений, легаси-PIC и клавиатура.
//!
//! GDT/TSS с отдельным стеком (IST) для double fault пока не заведены —
//! Limine даёт рабочий GDT, этого достаточно, пока мы не делаем свою
//! память/стеки (Phase 3). Вернёмся к этому, если понадобится защита от
//! переполнения стека ядра.

use lazy_static::lazy_static;
use pc_keyboard::layouts::Us104Key;
use pc_keyboard::{HandleControl, PS2Keyboard, ScancodeSet1};
use pic8259::ChainedPics;
use spin::Mutex;
use x86_64::instructions::port::Port;
use x86_64::structures::idt::{InterruptDescriptorTable, InterruptStackFrame, PageFaultErrorCode};

use crate::serial;

pub const PIC_1_OFFSET: u8 = 32;
pub const PIC_2_OFFSET: u8 = PIC_1_OFFSET + 8;

/// Легаси-PIC 8259: перемапливаем IRQ0..15 на векторы 32..47, чтобы не
/// пересекаться с исключениями процессора (0..31).
pub static PICS: Mutex<ChainedPics> = Mutex::new(unsafe { ChainedPics::new(PIC_1_OFFSET, PIC_2_OFFSET) });

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum InterruptIndex {
    Timer = PIC_1_OFFSET,
    Keyboard,
}

impl InterruptIndex {
    fn as_u8(self) -> u8 {
        self as u8
    }
}

lazy_static! {
    static ref IDT: InterruptDescriptorTable = {
        let mut idt = InterruptDescriptorTable::new();
        idt.breakpoint.set_handler_fn(breakpoint_handler);
        idt.double_fault.set_handler_fn(double_fault_handler);
        idt.general_protection_fault.set_handler_fn(gpf_handler);
        idt.page_fault.set_handler_fn(page_fault_handler);
        idt[InterruptIndex::Timer.as_u8()].set_handler_fn(timer_interrupt_handler);
        idt[InterruptIndex::Keyboard.as_u8()].set_handler_fn(keyboard_interrupt_handler);
        idt
    };
}

/// Загружает IDT, инициализирует PIC и включает прерывания.
/// Вызывать один раз, до первого обращения к клавиатуре.
pub fn init() {
    IDT.load();
    unsafe { PICS.lock().initialize() };
    x86_64::instructions::interrupts::enable();
    serial::print("CIOS: interrupts enabled\n");
}

extern "x86-interrupt" fn breakpoint_handler(stack_frame: InterruptStackFrame) {
    serial::print("CIOS: breakpoint exception\n");
    let _ = stack_frame;
}

extern "x86-interrupt" fn double_fault_handler(stack_frame: InterruptStackFrame, _error_code: u64) -> ! {
    serial::print("CIOS: DOUBLE FAULT\n");
    let _ = stack_frame;
    loop {
        x86_64::instructions::hlt();
    }
}

extern "x86-interrupt" fn gpf_handler(stack_frame: InterruptStackFrame, _error_code: u64) {
    serial::print("CIOS: GENERAL PROTECTION FAULT\n");
    let _ = stack_frame;
    loop {
        x86_64::instructions::hlt();
    }
}

extern "x86-interrupt" fn page_fault_handler(
    stack_frame: InterruptStackFrame,
    _error_code: PageFaultErrorCode,
) {
    serial::print("CIOS: PAGE FAULT\n");
    let _ = stack_frame;
    loop {
        x86_64::instructions::hlt();
    }
}

extern "x86-interrupt" fn timer_interrupt_handler(_stack_frame: InterruptStackFrame) {
    // PIT ещё не перепрограммирован на конкретную частоту — если он всё
    // же тикает (например, значение по умолчанию от прошивки), просто
    // шлём EOI, чтобы не словить шторм прерываний. Это отдельная задача.
    unsafe {
        PICS.lock().notify_end_of_interrupt(InterruptIndex::Timer.as_u8());
    }
}

lazy_static! {
    static ref KEYBOARD: Mutex<PS2Keyboard<Us104Key, ScancodeSet1>> =
        Mutex::new(PS2Keyboard::new(ScancodeSet1::new(), Us104Key, HandleControl::Ignore));
}

/// Держим ли сейчас Ctrl/Alt. pc_keyboard с HandleControl::Ignore не
/// превращает Ctrl+буква в управляющий байт (отдаёт обычную 'o'), а
/// nano-режиму нужно отличать ^O от просто 'o' — поэтому состояние
/// модификаторов трекаем сами прямо по сырым скан-кодам Set 1, не
/// полагаясь на внутренности pc_keyboard. Left/Right Ctrl = 0x1D
/// (Right приходит с префиксом 0xE0), Left/Right Alt = 0x38 (Right —
/// это AltGr, тоже с префиксом 0xE0). Бит 0x80 в скан-коде — это
/// "отпускание" клавиши (break code).
struct Modifiers {
    ctrl: bool,
    alt: bool,
}

static MODIFIERS: Mutex<Modifiers> = Mutex::new(Modifiers { ctrl: false, alt: false });

fn track_modifiers(scancode: u8) {
    if scancode == 0xE0 {
        // Префикс расширенной клавиши (Right Ctrl/Alt, стрелки и
        // т.п.) — сам по себе ничего не меняет, реальный код придёт
        // следующим байтом.
        return;
    }
    let is_break = scancode & 0x80 != 0;
    let code = scancode & 0x7F;
    let mut mods = MODIFIERS.lock();
    match code {
        // 0x1D/0x38 с префиксом 0xE0 — это Right Ctrl / Right Alt
        // (AltGr), без префикса — Left Ctrl / Left Alt. В Scancode
        // Set 1 эти же коды с 0xE0 никогда не означают ничего другого,
        // так что дальше можно не различать левую/правую клавишу.
        0x1D => mods.ctrl = !is_break,
        0x38 => mods.alt = !is_break,
        _ => {}
    }
}

extern "x86-interrupt" fn keyboard_interrupt_handler(_stack_frame: InterruptStackFrame) {
    let mut port = Port::new(0x60);
    let scancode: u8 = unsafe { port.read() };

    track_modifiers(scancode);
    let (ctrl, alt) = {
        let mods = MODIFIERS.lock();
        (mods.ctrl, mods.alt)
    };

    let mut keyboard = KEYBOARD.lock();
    if let Ok(Some(event)) = keyboard.add_byte(scancode) {
        if let Some(key) = keyboard.process_keyevent(event) {
            crate::keyboard_queue::push(key, ctrl, alt);
        }
    }

    unsafe {
        PICS.lock().notify_end_of_interrupt(InterruptIndex::Keyboard.as_u8());
    }
}
