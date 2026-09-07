//! Минимальный драйвер последовательного порта COM1 (0x3F8) —
//! используется для логов до появления рендера текста в терминале.

use core::arch::asm;
use core::fmt::Write;

const COM1: u16 = 0x3F8;

unsafe fn outb(port: u16, value: u8) {
    asm!("out dx, al", in("dx") port, in("al") value, options(nomem, nostack, preserves_flags));
}

unsafe fn inb(port: u16) -> u8 {
    let value: u8;
    asm!("in al, dx", out("al") value, in("dx") port, options(nomem, nostack, preserves_flags));
    value
}

pub fn init() {
    unsafe {
        outb(COM1 + 1, 0x00); // выключить прерывания порта
        outb(COM1 + 3, 0x80); // включить DLAB
        outb(COM1, 0x03); // делитель = 3 -> 38400 бод
        outb(COM1 + 1, 0x00);
        outb(COM1 + 3, 0x03); // 8 бит, без чётности, 1 стоп-бит
        outb(COM1 + 2, 0xC7); // включить FIFO
        outb(COM1 + 4, 0x0B); // IRQ off, RTS/DSR set
    }
}

fn is_transmit_empty() -> bool {
    unsafe { inb(COM1 + 5) & 0x20 != 0 }
}

fn write_byte(byte: u8) {
    while !is_transmit_empty() {}
    unsafe { outb(COM1, byte) };
}

pub fn print(s: &str) {
    for byte in s.bytes() {
        write_byte(byte);
    }
}

struct SerialWriter;

impl Write for SerialWriter {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        print(s);
        Ok(())
    }
}

pub fn print_fmt(args: impl core::fmt::Display) {
    let mut writer = SerialWriter;
    let _ = write!(writer, "{}\n", args);
}
