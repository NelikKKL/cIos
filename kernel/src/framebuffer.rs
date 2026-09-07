//! Тонкая обёртка над framebuffer, который отдаёт Limine.

use crate::theme::Color;

pub struct Framebuffer {
    ptr: *mut u8,
    pub width: usize,
    pub height: usize,
    pitch: usize,
    bpp: usize,
    red_shift: u8,
    green_shift: u8,
    blue_shift: u8,
}

impl Framebuffer {
    /// # Safety
    /// `ptr` должен указывать на валидный framebuffer размером как
    /// минимум `height * pitch` байт, живущий всё время работы ядра.
    pub unsafe fn new(
        ptr: *mut u8,
        width: usize,
        height: usize,
        pitch: usize,
        bpp: usize,
        red_shift: u8,
        green_shift: u8,
        blue_shift: u8,
    ) -> Self {
        Self { ptr, width, height, pitch, bpp, red_shift, green_shift, blue_shift }
    }

    fn pack(&self, c: Color) -> u32 {
        (c.r as u32) << self.red_shift | (c.g as u32) << self.green_shift | (c.b as u32) << self.blue_shift
    }

    #[inline]
    pub fn put_pixel(&mut self, x: usize, y: usize, color: Color) {
        if x >= self.width || y >= self.height {
            return;
        }
        let bytes_per_pixel = self.bpp / 8;
        let offset = y * self.pitch + x * bytes_per_pixel;
        let value = self.pack(color);
        unsafe {
            let dst = self.ptr.add(offset) as *mut u32;
            core::ptr::write_volatile(dst, value);
        }
    }

    /// Читает уже нарисованный пиксель — нужно, чтобы блендить
    /// полупрозрачную панель терминала поверх обоев.
    #[inline]
    pub fn get_pixel(&self, x: usize, y: usize) -> Color {
        if x >= self.width || y >= self.height {
            return Color::rgb(0, 0, 0);
        }
        let bytes_per_pixel = self.bpp / 8;
        let offset = y * self.pitch + x * bytes_per_pixel;
        let value = unsafe { core::ptr::read_volatile(self.ptr.add(offset) as *const u32) };
        let r = ((value >> self.red_shift) & 0xFF) as u8;
        let g = ((value >> self.green_shift) & 0xFF) as u8;
        let b = ((value >> self.blue_shift) & 0xFF) as u8;
        Color::rgb(r, g, b)
    }

    pub fn fill_rect(&mut self, x0: usize, y0: usize, w: usize, h: usize, color: Color) {
        for y in y0..(y0 + h).min(self.height) {
            for x in x0..(x0 + w).min(self.width) {
                self.put_pixel(x, y, color);
            }
        }
    }

    /// Заливка прямоугольника с alpha-блендингом поверх текущего
    /// содержимого — так рисуется полупрозрачная панель терминала.
    pub fn fill_rect_blended(&mut self, x0: usize, y0: usize, w: usize, h: usize, color: Color) {
        for y in y0..(y0 + h).min(self.height) {
            for x in x0..(x0 + w).min(self.width) {
                let under = self.get_pixel(x, y);
                let blended = color.blend_over(under);
                self.put_pixel(x, y, blended);
            }
        }
    }
}
