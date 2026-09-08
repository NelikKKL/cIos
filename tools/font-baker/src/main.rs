//! Host-side инструмент: растеризует TTF-шрифт в простой бинарный
//! атлас глифов фиксированного размера, который потом встраивается
//! в ядро через include_bytes! (см. kernel/src/font.rs).
//!
//! Формат файла (little-endian):
//!   magic:            [u8; 4] = b"CFA1"
//!   first_codepoint:  u32   -- код первого символа в атласе (0x20)
//!   glyph_count:      u32   -- количество глифов подряд от first_codepoint
//!   cell_w:           u32   -- ширина ячейки в пикселях
//!   cell_h:           u32   -- высота ячейки в пикселях
//!   glyphs: [u8; glyph_count * cell_w * cell_h]
//!       -- по одному байту на пиксель (alpha/coverage 0..=255),
//!          глифы идут подряд, каждый — cell_w*cell_h байт, row-major.
//!
//! Пока запекаются только ASCII 0x20..=0x7E — этого достаточно для
//! первого рабочего текста. Иконки Nerd Font и кириллица — отдельный
//! проход (нужно решить, какие диапазоны/glyph-id из шрифта брать).

use std::env;
use std::fs;
use std::path::Path;

const FIRST_CODEPOINT: u32 = 0x20; // ' '
const LAST_CODEPOINT: u32 = 0x7E; // '~'
const PX_SIZE: f32 = 16.0;

fn main() {
    let args: Vec<String> = env::args().collect();
    let font_path = args
        .get(1)
        .cloned()
        .unwrap_or_else(|| "../../assets/fonts/AdwaitaMonoNerdFontMono-Regular.ttf".to_string());
    let out_path = args
        .get(2)
        .cloned()
        .unwrap_or_else(|| "../../assets/fonts/font_atlas.bin".to_string());

    let font_bytes =
        fs::read(&font_path).unwrap_or_else(|e| panic!("не смог прочитать шрифт {font_path}: {e}"));
    let font = fontdue::Font::from_bytes(font_bytes.as_slice(), fontdue::FontSettings::default())
        .expect("fontdue не смог распарсить TTF");

    let line_metrics = font
        .horizontal_line_metrics(PX_SIZE)
        .expect("в шрифте нет horizontal line metrics");
    let ascent = line_metrics.ascent.ceil() as i32;
    let descent = line_metrics.descent.floor() as i32; // обычно отрицательное

    // Шрифт моноширинный, так что advance_width одинаковый для всех
    // печатаемых символов — берём его на пробеле.
    let (space_metrics, _) = font.rasterize(' ', PX_SIZE);
    let cell_w = space_metrics.advance_width.ceil().max(1.0) as usize;
    let cell_h = (ascent - descent).max(1) as usize;

    let glyph_count = (LAST_CODEPOINT - FIRST_CODEPOINT + 1) as usize;
    let mut atlas = vec![0u8; glyph_count * cell_w * cell_h];

    for i in 0..glyph_count {
        let codepoint = FIRST_CODEPOINT + i as u32;
        let ch = char::from_u32(codepoint).unwrap();
        let (metrics, bitmap) = font.rasterize(ch, PX_SIZE);

        // Baseline — строка `ascent`, считая от верха ячейки. Кладём
        // битмап глифа так, чтобы его нижний край (ymin) лёг на baseline.
        let dst_x0 = metrics.xmin;
        let dst_y0 = ascent - metrics.ymin - metrics.height as i32;

        for gy in 0..metrics.height {
            let dst_y = dst_y0 + gy as i32;
            if dst_y < 0 || dst_y as usize >= cell_h {
                continue; // не влезло по высоте — обрезаем, а не паникуем
            }
            for gx in 0..metrics.width {
                let dst_x = dst_x0 + gx as i32;
                if dst_x < 0 || dst_x as usize >= cell_w {
                    continue;
                }
                let coverage = bitmap[gy * metrics.width + gx];
                let cell_base = i * cell_w * cell_h;
                let dst_index = cell_base + dst_y as usize * cell_w + dst_x as usize;
                atlas[dst_index] = coverage;
            }
        }
    }

    let mut out = Vec::with_capacity(20 + atlas.len());
    out.extend_from_slice(b"CFA1");
    out.extend_from_slice(&FIRST_CODEPOINT.to_le_bytes());
    out.extend_from_slice(&(glyph_count as u32).to_le_bytes());
    out.extend_from_slice(&(cell_w as u32).to_le_bytes());
    out.extend_from_slice(&(cell_h as u32).to_le_bytes());
    out.extend_from_slice(&atlas);

    if let Some(parent) = Path::new(&out_path).parent() {
        fs::create_dir_all(parent).ok();
    }
    fs::write(&out_path, &out).expect("не смог записать font_atlas.bin");

    println!(
        "font-baker: {glyph_count} глифов, ячейка {cell_w}x{cell_h}, файл {out_path} ({} байт)",
        out.len()
    );
}
