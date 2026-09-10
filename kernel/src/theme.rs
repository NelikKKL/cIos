//! Тема оформления CIOS: цвета, прозрачность терминала, обои.
//!
//! Четыре готовые темы объявлены здесь как статики (DEFAULT_THEME,
//! SOLID_THEME, AUTUMN_THEME, SNOW_THEME). Команда `css` в шелле умеет
//! как переключаться между ними, так и собирать Theme в рантайме из
//! CSS-подобного текста — см. css.rs.
#![allow(dead_code)]

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
    /// Растровая картинка, запечённая tools/wallpaper-baker и встроенная
    /// в ядро (см. bitmap.rs). Масштабируется под реальный framebuffer
    /// в рантайме.
    Bitmap(&'static crate::bitmap::Bitmap),
}

/// Форма терминальной панели. Square — по заметке из темы Default
/// ("сделай терминал квадратным") — центрированный квадрат вместо
/// панели на всю ширину экрана.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PanelShape {
    Fill,
    Square,
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
    pub panel_shape: PanelShape,
    pub bar: BarTheme,
    pub cursor: Color,
}

/// Тема по умолчанию: обои-созвездия + квадратная полупрозрачная
/// панель терминала (по заметке из Themes/Default: "сделай терминал
/// квадратным и полупрозрачным").
pub static DEFAULT_THEME: Theme = Theme {
    wallpaper: Wallpaper::Bitmap(&crate::bitmap::DEFAULT_WALLPAPER),
    terminal_bg: Color::rgba(10, 10, 16, 178), // ~70% непрозрачности
    terminal_fg: Color::rgb(235, 235, 240),
    terminal_padding_px: 24,
    panel_shape: PanelShape::Square,
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
pub static SOLID_THEME: Theme = Theme {
    wallpaper: Wallpaper::Solid(Color::rgb(12, 12, 16)),
    terminal_bg: Color::rgba(0, 0, 0, 255),
    terminal_fg: Color::rgb(220, 220, 220),
    terminal_padding_px: 16,
    panel_shape: PanelShape::Fill,
    bar: BarTheme {
        position: BarPosition::Top,
        background: Color::rgb(20, 20, 20),
        foreground: Color::rgb(200, 200, 200),
        height_px: 24,
    },
    cursor: Color::rgb(0, 255, 120),
};

/// Осенняя тема: фото аллеи (Themes/Autumn) + тёплая, почти
/// непрозрачная панель — как на референсном скриншоте терминала.
pub static AUTUMN_THEME: Theme = Theme {
    wallpaper: Wallpaper::Bitmap(&crate::bitmap::AUTUMN_WALLPAPER),
    terminal_bg: Color::rgba(26, 16, 14, 225),
    terminal_fg: Color::rgb(240, 225, 210),
    terminal_padding_px: 24,
    panel_shape: PanelShape::Fill,
    bar: BarTheme {
        position: BarPosition::Bottom,
        background: Color::rgba(20, 10, 8, 200),
        foreground: Color::rgb(250, 220, 180),
        height_px: 28,
    },
    cursor: Color::rgb(255, 200, 120),
};

/// Зимняя тема: снежный город (Themes/Snow) + панель "как в Autumn",
/// но цветом #433A44 — по заметке из Themes/Snow/text.txt.
pub static SNOW_THEME: Theme = Theme {
    wallpaper: Wallpaper::Bitmap(&crate::bitmap::SNOW_WALLPAPER),
    terminal_bg: Color::rgba(0x43, 0x3A, 0x44, 225), // #433A44, alpha как в Autumn
    terminal_fg: Color::rgb(235, 230, 240),
    terminal_padding_px: 24,
    panel_shape: PanelShape::Fill,
    bar: BarTheme {
        position: BarPosition::Bottom,
        background: Color::rgba(30, 26, 34, 200),
        foreground: Color::rgb(230, 225, 240),
        height_px: 28,
    },
    cursor: Color::rgb(210, 225, 255),
};
