//! Тема оформления CIOS: цвета, прозрачность терминала, обои.
//!
//! Сейчас темы захардкожены здесь как константы. В Phase 5 сюда
//! добавится парсер CSS-подобного конфига (`terminal { background: ... }`),
//! который будет собирать `Theme` в рантайме из файла на диске/в initrd —
//! но структура данных, за которую он будет отвечать, уже здесь.

#[derive(Clone, Copy)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    /// 0 = полностью прозрачный, 255 = полностью непрозрачный.
    pub a: u8,
}

impl Color {
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b, a: 255 }
    }

    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    /// Alpha-блендинг "src over dst" — именно этим рисуется
    /// полупрозрачный терминал поверх уже нарисованных обоев.
    pub fn blend_over(self, dst: Color) -> Color {
        if self.a == 255 {
            return self;
        }
        if self.a == 0 {
            return dst;
        }
        let a = self.a as u32;
        let inv_a = 255 - a;
        let r = (self.r as u32 * a + dst.r as u32 * inv_a) / 255;
        let g = (self.g as u32 * a + dst.g as u32 * inv_a) / 255;
        let b = (self.b as u32 * a + dst.b as u32 * inv_a) / 255;
        Color::rgb(r as u8, g as u8, b as u8)
    }
}

#[derive(Clone, Copy)]
pub enum Wallpaper {
    Solid(Color),
    /// Вертикальный градиент сверху вниз.
    VerticalGradient(Color, Color),
    // Позже сюда добавится Wallpaper::Bitmap(&'static [u8], u32, u32) —
    // растровая картинка, встроенная в ядро сборочным скриптом.
}

#[derive(Clone, Copy)]
pub enum BarPosition {
    Top,
    Bottom,
    Hidden,
}

#[derive(Clone, Copy)]
pub struct BarTheme {
    pub position: BarPosition,
    pub background: Color,
    pub foreground: Color,
    pub height_px: u32,
}

#[derive(Clone, Copy)]
pub struct Theme {
    pub wallpaper: Wallpaper,
    /// Фон терминальной панели — alpha этого цвета и даёт эффект
    /// "полупрозрачный терминал поверх обоев".
    pub terminal_bg: Color,
    pub terminal_fg: Color,
    pub terminal_padding_px: u32,
    pub bar: BarTheme,
    pub cursor: Color,
}

/// Тема по умолчанию: тёмно-фиолетовый градиент + терминал
/// примерно на 70% непрозрачности.
pub const DEFAULT_THEME: Theme = Theme {
    wallpaper: Wallpaper::VerticalGradient(Color::rgb(18, 20, 34), Color::rgb(40, 22, 54)),
    terminal_bg: Color::rgba(10, 10, 16, 178), // ~70% непрозрачности
    terminal_fg: Color::rgb(235, 235, 240),
    terminal_padding_px: 24,
    bar: BarTheme {
        position: BarPosition::Bottom,
        background: Color::rgba(0, 0, 0, 160),
        foreground: Color::rgb(255, 255, 255),
        height_px: 28,
    },
    cursor: Color::rgb(255, 255, 255),
};

/// Альтернативная тема — полностью непрозрачный терминал,
/// для сравнения (показывает, что "прозрачность" — это просто alpha).
pub const SOLID_THEME: Theme = Theme {
    wallpaper: Wallpaper::Solid(Color::rgb(12, 12, 16)),
    terminal_bg: Color::rgba(0, 0, 0, 255),
    terminal_fg: Color::rgb(220, 220, 220),
    terminal_padding_px: 16,
    bar: BarTheme {
        position: BarPosition::Top,
        background: Color::rgb(20, 20, 20),
        foreground: Color::rgb(200, 200, 200),
        height_px: 24,
    },
    cursor: Color::rgb(0, 255, 120),
};
