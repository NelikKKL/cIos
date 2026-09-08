//! Доступ к битмап-атласу шрифта, запечённому tools/font-baker из
//! assets/fonts/AdwaitaMonoNerdFontMono-Regular.ttf.
//! Формат атласа описан в tools/font-baker/src/main.rs.

static ATLAS: &[u8] = include_bytes!("../../assets/fonts/font_atlas.bin");

const HEADER_LEN: usize = 4 + 4 + 4 + 4 + 4;

pub struct Font {
    first_codepoint: u32,
    glyph_count: u32,
    pub cell_w: usize,
    pub cell_h: usize,
    glyphs: &'static [u8],
}

fn read_u32(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes([bytes[offset], bytes[offset + 1], bytes[offset + 2], bytes[offset + 3]])
}

pub fn font() -> Font {
    assert_eq!(&ATLAS[0..4], b"CFA1", "font_atlas.bin: неверная сигнатура");
    let first_codepoint = read_u32(ATLAS, 4);
    let glyph_count = read_u32(ATLAS, 8);
    let cell_w = read_u32(ATLAS, 12) as usize;
    let cell_h = read_u32(ATLAS, 16) as usize;
    Font {
        first_codepoint,
        glyph_count,
        cell_w,
        cell_h,
        glyphs: &ATLAS[HEADER_LEN..],
    }
}

impl Font {
    /// Alpha-битмап глифа (cell_w*cell_h байт, значения 0..=255),
    /// либо None, если символа нет в атласе (сейчас — только ASCII 0x20..=0x7E).
    pub fn glyph(&self, ch: char) -> Option<&'static [u8]> {
        let codepoint = ch as u32;
        if codepoint < self.first_codepoint || codepoint >= self.first_codepoint + self.glyph_count {
            return None;
        }
        let index = (codepoint - self.first_codepoint) as usize;
        let cell_size = self.cell_w * self.cell_h;
        let start = index * cell_size;
        Some(&self.glyphs[start..start + cell_size])
    }
}
