# CIOS

Не графическая ОС на Rust для запуска в виртуальной машине.
Пользователь видит терминал поверх обоев; и то, и другое настраивается
через тему (сейчас — Rust-константы, в Phase 5 — CSS-подобный конфиг).

Статус: **Phase 0–3 готовы** (загрузка, обои/прозрачный терминал, текст
ASCII-шрифтом, прерывания+клавиатура, куча ядра). RAM-файловая система
(`fs.rs`) и шелл (`shell.rs`) с командами `ls cat rm mkdir touch cd pwd
echo clear help` работают по-настоящему. **`file-sys` теперь настоящий
интерактивный файловый менеджер**: `↑`/`↓` — выбор строки, `Enter` —
войти в папку, `Backspace`/`←` — выйти на уровень выше, `Esc`/`q` —
закрыть и вернуться в шелл. Открытие файлов (превью) пока не сделано.

## Структура

```
kernel/             — само ядро (no_std, no_main)
  src/main.rs        — точка входа, запросы к Limine
  src/framebuffer.rs — обёртка над framebuffer + alpha-блендинг
  src/theme.rs        — Color/Wallpaper/Theme, DEFAULT_THEME и SOLID_THEME
  src/terminal.rs     — рисует обои, бар, панель терминала и текст в ней
  src/font.rs         — читает встроенный атлас глифов (font_atlas.bin)
  src/text.rs         — рисование строк по атласу с alpha-блендингом
  src/interrupts.rs   — IDT, PIC, обработчики исключений и клавиатуры
  src/keyboard_queue.rs — кольцевой буфер декодированных нажатий клавиш
  src/memory.rs       — HHDM/memmap от Limine + куча (linked_list_allocator)
  src/fs.rs           — файловая система в ОЗУ (дерево File/Dir на alloc)
  src/shell.rs        — разбор и выполнение команд поверх fs.rs
  src/serial.rs       — драйвер COM1 для логов/паники
tools/font-baker/    — host-инструмент (std): TTF -> бинарный атлас глифов
assets/fonts/         — AdwaitaMonoNerdFontMono-Regular.ttf + сгенерированный
                        font_atlas.bin (в .gitignore, пересобирается Makefile'ом)
limine.conf          — конфиг загрузчика (Limine, ассет последнего релиза)
Makefile             — font-atlas / kernel / limine / iso / run / clean
.github/workflows/   — сборка ISO в CI
```

## Сборка локально (Linux, нужен интернет)

Зависимости: `rustup` (nightly + компонент `rust-src`), `git`, `curl`,
`xorriso`, `qemu-system-x86_64` (для `make run`), `gcc`/`make`
(для host-тулов `limine` и `font-baker`).

```sh
make iso   # соберёт kernel/target/.../cios-kernel и cios.iso
make run   # то же самое + запуск в QEMU (serial выводится в stdout)
```

Первая сборка попробует скачать crate `limine` (версия `0.6.x`),
ассет `limine-binary.tar.gz` с последнего релиза
`Limine-Bootloader/Limine` на GitHub (у Limine больше нет веток
`*-binary` — теперь бинарные сборки публикуются как ассеты релизов) и
crate `fontdue` для `tools/font-baker`. `make kernel`/`make iso`
сначала соберут `font-baker` и прогонят его на
`assets/fonts/AdwaitaMonoNerdFontMono-Regular.ttf`, чтобы получить
`font_atlas.bin`, который затем встраивается в ядро.

## Как работает прозрачность терминала

`theme.rs` задаёт `Theme.terminal_bg: Color { r, g, b, a }`.
`framebuffer.rs::fill_rect_blended` перед закраской читает уже
нарисованный пиксель обоев и смешивает его с `terminal_bg` по формуле
`src*a + dst*(1-a)` — классический alpha-blend "over". Поэтому
"тема с прозрачным терминалом" — это просто `a < 255`, а
`SOLID_THEME` в этом же файле показывает вариант с `a = 255`
(непрозрачный терминал) для сравнения.

## Клавиатура и стрелки

`interrupts.rs` поднимает IDT + перемапливает легаси-PIC (IRQ0..15 →
векторы 32..47) и вешает обработчик на IRQ1 (клавиатура): читает
скан-код с порта `0x60`, декодирует через `pc-keyboard`
(`ScancodeSet1` + `Us104Key`) и кладёт результат в кольцевой буфер
(`keyboard_queue.rs`, без кучи — фиксированный массив за спинлоком).
Главный цикл в `main.rs` спит в `hlt` до следующего прерывания,
разбирает накопленные нажатия и ведёт простой строчный буфер: печать
символов, `Backspace`, `Enter` (пока просто логирует строку в serial),
`←`/`→` двигают курсор по буферу — это и есть основа для будущей
навигации стрелками в `file-sys` и меню (Phase 7–8).

## Дальше по плану (см. предыдущее обсуждение фаз)

Phase 4 доделать (иконки Nerd Font и кириллица в атласе) → превью файла
в `file-sys` (Enter на файле показывает содержимое) → Phase 5
(CSS-подобный конфиг тем) → история команд по `↑`/`↓` в шелле →
персистентное хранилище вместо RAM-FS.
