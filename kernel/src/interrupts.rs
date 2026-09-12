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

    // ВРЕМЕННАЯ диагностика зависания клавиатуры:
    //  - serial-принт — полезен в QEMU/VirtualBox с настроенным COM1.
    //  - короткий писк через PC-спикер — работает без serial вообще
    //    (актуально для реального железа без COM-порта), и написан
    //    буквально первой строчкой обработчика, в обход клавиатурного
    //    стека/очереди/рендера: если пищит — IRQ1 точно доходит до
    //    CPU и обработчик реально выполняется, что бы ни было сломано
    //    дальше. Если не пищит вообще НИКОГДА (не только при вводе,
    //    а вообще, с первого нажатия) — прерывание не доходит до CPU
    //    (вероятно, легаси-PIC не подключён к реальным IRQ на этом
    //    железе — нужна поддержка APIC, которой сейчас нет).
    //  Важная оговорка: на части современных ноутбуков физического
    //  PC-спикера (порт 0x61 + PIT channel 2) просто нет — тогда тишина
    //  ничего не докажет, и нужен будет другой способ диагностики.
    beep_tick();

    serial::print("CIOS: IRQ1 scancode=0x");
    print_hex_byte(scancode);
    serial::print("\n");

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

/// Короткий писк PC-спикера (порт 0x61 + PIT channel 2, классический
/// "beep", работает без видеокарты/serial/framebuffer вообще). Не
/// использует hlt/задержки — включает динамик и сразу выключает,
/// длительность звука на слух определяется тем, сколько успевает
/// накопиться таких вызовов подряд при быстром наборе; для теста
/// "доходит ли IRQ1 вообще" этого достаточно, честный таймер здесь
/// не нужен (PIT ещё не перепрограммирован под общие нужды, см. Timer).
fn beep_tick() {
    unsafe {
        let mut pit_cmd = Port::<u8>::new(0x43);
        let mut pit_data = Port::<u8>::new(0x42);
        let mut speaker = Port::<u8>::new(0x61);

        // ~1kHz: 1193182 / 1000 ≈ 1193.
        let divisor: u16 = 1193;
        pit_cmd.write(0xB6u8); // channel 2, lobyte/hibyte, square wave
        pit_data.write((divisor & 0xFF) as u8);
        pit_data.write((divisor >> 8) as u8);

        let cur: u8 = speaker.read();
        speaker.write(cur | 0x03); // включить спикер

        // Простая busy-wait пауза без завязки на таймер прерываний —
        // чисто чтобы ухо успело различить щелчок.
        for _ in 0..200_000u32 {
            core::hint::spin_loop();
        }

        let cur: u8 = speaker.read();
        speaker.write(cur & !0x03); // выключить спикер
    }
}

/// Печатает байт как два hex-символа — без format!/alloc, чтобы можно
/// было звать прямо из обработчика прерывания без лишних зависимостей.
fn print_hex_byte(b: u8) {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let bytes = [HEX[(b >> 4) as usize], HEX[(b & 0x0F) as usize]];
    if let Ok(s) = core::str::from_utf8(&bytes) {
        serial::print(s);
    }
}
