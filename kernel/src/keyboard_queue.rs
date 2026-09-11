//! Очередь декодированных нажатий клавиш. Заполняется из обработчика
//! прерывания IRQ1 (interrupts.rs), читается основным циклом ядра
//! (main.rs). Кучи ещё нет на момент первого нажатия (Phase 3), поэтому
//! — фиксированный кольцевой буфер за спинлоком, без аллокаций.
//!
//! KeyEvent, помимо самой клавиши (`DecodedKey` из pc_keyboard), несёт
//! состояние Ctrl/Alt на момент нажатия — это нужно nano-режиму, у
//! которого почти все горячие клавиши это Ctrl+буква (^O, ^X, ^W...)
//! или Alt+буква (M-...). Сам pc_keyboard/HandleControl::Ignore модификаторы
//! на выходной символ не влияет (отдаёт обычную 'o', не управляющий
//! байт) — состояние Ctrl/Alt трекается отдельно в interrupts.rs по
//! сырым скан-кодам и кладётся сюда при каждом push.

use pc_keyboard::DecodedKey;
use spin::Mutex;

const CAPACITY: usize = 64;

#[derive(Clone, Copy)]
pub struct KeyEvent {
    pub key: DecodedKey,
    pub ctrl: bool,
    pub alt: bool,
}

struct RingBuffer {
    buf: [Option<KeyEvent>; CAPACITY],
    head: usize,
    tail: usize,
    len: usize,
}

impl RingBuffer {
    const fn new() -> Self {
        Self {
            buf: [None; CAPACITY],
            head: 0,
            tail: 0,
            len: 0,
        }
    }

    fn push(&mut self, event: KeyEvent) {
        if self.len == CAPACITY {
            // буфер полон — теряем самое старое нажатие, а не новое
            self.head = (self.head + 1) % CAPACITY;
            self.len -= 1;
        }
        self.buf[self.tail] = Some(event);
        self.tail = (self.tail + 1) % CAPACITY;
        self.len += 1;
    }

    fn pop(&mut self) -> Option<KeyEvent> {
        if self.len == 0 {
            return None;
        }
        let event = self.buf[self.head].take();
        self.head = (self.head + 1) % CAPACITY;
        self.len -= 1;
        event
    }
}

static QUEUE: Mutex<RingBuffer> = Mutex::new(RingBuffer::new());

/// Вызывается из обработчика прерывания клавиатуры (interrupts.rs).
pub fn push(key: DecodedKey, ctrl: bool, alt: bool) {
    QUEUE.lock().push(KeyEvent { key, ctrl, alt });
}

/// Вызывается основным циклом ядра, чтобы забрать следующее нажатие.
pub fn pop() -> Option<KeyEvent> {
    QUEUE.lock().pop()
}
