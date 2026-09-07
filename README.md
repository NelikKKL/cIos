# CIOS

Не графическая ОС на Rust для запуска в виртуальной машине.
Пользователь видит терминал поверх обоев; и то, и другое настраивается
через тему (сейчас — Rust-константы, в Phase 5 — CSS-подобный конфиг).

Статус: **Phase 0–1** — ядро грузится через Limine, рисует обои и
полупрозрачную панель терминала во framebuffer. Рендера текста ещё
нет (это Phase 4 — потребуется запечь `AdwaitaMono Nerd Font` в
битмап-атлас при сборке).

## Структура

```
kernel/          — само ядро (no_std, no_main)
  src/main.rs      — точка входа, запросы к Limine
  src/framebuffer.rs — обёртка над framebuffer + alpha-блендинг
  src/theme.rs       — Color/Wallpaper/Theme, DEFAULT_THEME и SOLID_THEME
  src/terminal.rs    — рисует обои, бар и панель терминала по теме
  src/serial.rs      — драйвер COM1 для логов/паники
limine.cfg         — конфиг загрузчика (ветка limine v7.x-binary)
Makefile            — kernel / limine / iso / run / clean
.github/workflows/  — сборка ISO в CI
```

## Сборка локально (Linux, нужен интернет)

Зависимости: `rustup` (nightly + компонент `rust-src`), `git`,
`xorriso`, `qemu-system-x86_64` (для `make run`), `gcc`/`make`
(для сборки инструмента `limine`).

```sh
make iso   # соберёт kernel/target/.../cios-kernel и cios.iso
make run   # то же самое + запуск в QEMU (serial выводится в stdout)
```

Первая сборка попробует скачать crate `limine` и клонировать
репозиторий `limine-bootloader/limine` — при отсутствии сети (как в
песочнице, где это писалось) собрать и запустить нельзя, но структура
готова к сборке в GitHub Actions или на твоей машине.

## Как работает прозрачность терминала

`theme.rs` задаёт `Theme.terminal_bg: Color { r, g, b, a }`.
`framebuffer.rs::fill_rect_blended` перед закраской читает уже
нарисованный пиксель обоев и смешивает его с `terminal_bg` по формуле
`src*a + dst*(1-a)` — классический alpha-blend "over". Поэтому
"тема с прозрачным терминалом" — это просто `a < 255`, а
`SOLID_THEME` в этом же файле показывает вариант с `a = 255`
(непрозрачный терминал) для сравнения.

## Дальше по плану (см. предыдущее обсуждение фаз)

Phase 2 (прерывания/клавиатура) → Phase 3 (память/heap) →
Phase 4 (запекание Nerd Font в атлас, реальный текст) →
Phase 5 (CSS-подобный конфиг тем, читаемый в рантайме) → ...
