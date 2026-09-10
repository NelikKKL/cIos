//! Host-side инструмент: декодирует обои (JPEG/PNG — формат
//! определяется по содержимому, а не по расширению файла) и
//! растягивает их до опорного разрешения WALLPAPER_W x WALLPAPER_H.
//! Ядро в рантайме масштабирует это опорное изображение под реальное
//! разрешение framebuffer (nearest-neighbor, см. kernel/src/bitmap.rs).
//!
//! Формат выходного файла (little-endian):
//!   width:  u32
//!   height: u32
//!   pixels: [u8; width * height * 3]   -- RGB8, построчно

use std::env;
use std::fs;
use std::path::Path;

pub const WALLPAPER_W: u32 = 640;
pub const WALLPAPER_H: u32 = 400;

fn main() {
    let args: Vec<String> = env::args().collect();
    let in_path = args.get(1).expect("usage: wallpaper-baker <in> <out>");
    let out_path = args.get(2).expect("usage: wallpaper-baker <in> <out>");

    let img = image::ImageReader::open(in_path)
        .unwrap_or_else(|e| panic!("не смог открыть {in_path}: {e}"))
        .with_guessed_format()
        .expect("не смог определить формат изображения")
        .decode()
        .unwrap_or_else(|e| panic!("не смог декодировать {in_path}: {e}"));

    let resized = img.resize_exact(WALLPAPER_W, WALLPAPER_H, image::imageops::FilterType::Lanczos3);
    let rgb = resized.to_rgb8();
    let raw = rgb.into_raw();
    debug_assert_eq!(raw.len(), (WALLPAPER_W * WALLPAPER_H * 3) as usize);

    let mut out = Vec::with_capacity(8 + raw.len());
    out.extend_from_slice(&WALLPAPER_W.to_le_bytes());
    out.extend_from_slice(&WALLPAPER_H.to_le_bytes());
    out.extend_from_slice(&raw);

    if let Some(parent) = Path::new(out_path).parent() {
        fs::create_dir_all(parent).ok();
    }
    fs::write(out_path, &out).unwrap_or_else(|e| panic!("не смог записать {out_path}: {e}"));

    println!("wallpaper-baker: {in_path} -> {out_path} ({}x{}, {} байт)", WALLPAPER_W, WALLPAPER_H, out.len());
}
