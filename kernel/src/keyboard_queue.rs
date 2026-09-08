//! Очередь декодированных нажатий клавиш. Заполняется из обработчика
//! прерывания IRQ1 (interrupts.rs), читается основным циклом ядра
//! (main.rs). Кучи ещё нет (Phase 3), поэтому — фиксированный кольцевой
//! буфер за спинлоком, без аллокаций.

use pc_keyboard::DecodedKey;
use spin::Mutex;

const CAPACITY: usize = 64;

struct RingBuffer {
    buf: [Option<DecodedKey>; CAPACITY],
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

    fn push(&mut self, key: DecodedKey) {
        if self.len == CAPACITY {
            // буфер полон — теряем самое старое нажатие, а не новое
            self.head = (self.head + 1) % CAPACITY;
            self.len -= 1;
        }
        self.buf[self.tail] = Some(key);
        self.tail = (self.tail + 1) % CAPACITY;
        self.len += 1;
    }

    fn pop(&mut self) -> Option<DecodedKey> {
        if self.len == 0 {
            return None;
        }
        let key = self.buf[self.head].take();
        self.head = (self.head + 1) % CAPACITY;
        self.len -= 1;
        key
    }
}

static QUEUE: Mutex<RingBuffer> = Mutex::new(RingBuffer::new());

/// Вызывается из обработчика прерывания клавиатуры (interrupts.rs).
pub fn push(key: DecodedKey) {
    QUEUE.lock().push(key);
}

/// Вызывается основным циклом ядра, чтобы забрать следующее нажатие.
pub fn pop() -> Option<DecodedKey> {
    QUEUE.lock().pop()
}
