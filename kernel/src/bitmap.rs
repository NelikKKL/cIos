//! Встроенные растровые обои. Каждая картинка запечена
//! tools/wallpaper-baker в опорное разрешение (см. WALLPAPER_W/H там же)
//! и включена в бинарник ядра через include_bytes! — во время выполнения
//! ничего не декодируем, только масштабируем nearest-neighbor под
//! реальный размер framebuffer (см. blit_scaled).

use crate::framebuffer::Framebuffer;
use crate::theme::Color;

pub struct Bitmap {
    pub width: u32,
    pub height: u32,
    /// RGB8, построчно, без заголовка (заголовок уже отброшен ниже).
    pub pixels: &'static [u8],
}

const HEADER_LEN: usize = 8;

/// Отбрасывает заголовок (см. формат в tools/wallpaper-baker) через
/// прямую конструкцию среза по указателю — НЕ через `&RAW[HEADER_LEN..]`.
/// Обычное срез-индексирование идёт через типаж `Index`, а он в
/// static-контексте на свежих nightly требует нестабильных
/// `#![feature(const_index, const_trait_impl)]` (регрессия/изменение
/// компилятора, всплывшее уже после того, как этот файл писался —
/// `from_raw_parts` того же самого не требует, это отдельный, давно
/// стабильный const fn).
const fn strip_header(raw: &'static [u8]) -> &'static [u8] {
    // SAFETY: raw всегда получен из include_bytes! готового .raw-файла
    // (см. tools/wallpaper-baker), который всегда >= HEADER_LEN байт —
    // формат гарантирует 8-байтовый заголовок перед пикселями.
    unsafe { core::slice::from_raw_parts(raw.as_ptr().add(HEADER_LEN), raw.len() - HEADER_LEN) }
}

macro_rules! bake_bitmap {
    ($path:literal) => {{
        const RAW: &[u8] = include_bytes!($path);
        const W: u32 = u32::from_le_bytes([RAW[0], RAW[1], RAW[2], RAW[3]]);
        const H: u32 = u32::from_le_bytes([RAW[4], RAW[5], RAW[6], RAW[7]]);
        Bitmap { width: W, height: H, pixels: strip_header(RAW) }
    }};
}

pub static DEFAULT_WALLPAPER: Bitmap = bake_bitmap!("../../assets/wallpapers/default.raw");
pub static AUTUMN_WALLPAPER: Bitmap = bake_bitmap!("../../assets/wallpapers/autumn.raw");
pub static SNOW_WALLPAPER: Bitmap = bake_bitmap!("../../assets/wallpapers/snow.raw");

impl Bitmap {
    #[inline]
    fn sample(&self, x: u32, y: u32) -> Color {
        let x = x.min(self.width - 1) as usize;
        let y = y.min(self.height - 1) as usize;
        let idx = (y * self.width as usize + x) * 3;
        Color::rgb(self.pixels[idx], self.pixels[idx + 1], self.pixels[idx + 2])
    }

    /// Растягивает картинку на весь framebuffer (nearest-neighbor,
    /// без сохранения пропорций — обои просто заполняют весь экран).
    pub fn blit_scaled(&self, fb: &mut Framebuffer) {
        let dst_w = fb.width.max(1) as u32;
        let dst_h = fb.height.max(1) as u32;
        for y in 0..dst_h {
            let src_y = y * self.height / dst_h;
            for x in 0..dst_w {
                let src_x = x * self.width / dst_w;
                let color = self.sample(src_x, src_y);
                fb.put_pixel(x as usize, y as usize, color);
            }
        }
    }
}
