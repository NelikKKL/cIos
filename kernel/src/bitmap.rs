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

macro_rules! bake_bitmap {
    ($path:literal) => {{
        const RAW: &[u8] = include_bytes!($path);
        const W: u32 = u32::from_le_bytes([RAW[0], RAW[1], RAW[2], RAW[3]]);
        const H: u32 = u32::from_le_bytes([RAW[4], RAW[5], RAW[6], RAW[7]]);
        Bitmap { width: W, height: H, pixels: &RAW[HEADER_LEN..] }
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
