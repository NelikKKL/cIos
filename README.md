# CIOS

Не графическая ОС на Rust для запуска в виртуальной машине.
Пользователь видит терминал поверх обоев; и то, и другое настраивается
через тему — либо готовыми пресетами, либо CSS-подобным конфигом
(команда `css` в шелле).

Статус: **Phase 0–3 готовы** (загрузка, обои/прозрачный терминал, текст
ASCII-шрифтом, прерывания+клавиатура, куча ядра). RAM-файловая система
(`fs.rs`) и шелл (`shell.rs`) с командами `ls cat rm mkdir touch cd pwd
echo clear help` работают по-настоящему. `file-sys` — настоящий
интерактивный файловый менеджер (`↑`/`↓` выбор, `Enter` войти в папку,
`Backspace`/`←` на уровень выше, `Esc`/`q` закрыть). **Phase 5 (CSS-темы)
готова**: команда `css` открывает меню из 4 пунктов — Default/Autumn/Snow
(готовые темы с настоящими встроенными обоями) и Create custom CSS
(текстовый редактор мини-CSS, `Escape` применяет).

## Структура

```
kernel/             — само ядро (no_std, no_main)
  src/main.rs        — точка входа, запросы к Limine, Mode (Shell/FileManager/CssMenu/CssEditor)
  src/framebuffer.rs — обёртка над framebuffer + alpha-блендинг
  src/theme.rs        — Color/Wallpaper/PanelShape/Theme, 4 встроенные темы
  src/bitmap.rs       — встроенные растровые обои + масштабируемый блит
  src/css.rs          — парсер CSS-подобного конфига тем
  src/terminal.rs     — рисует обои/бар/панель: шелл, file-manager, css-меню, css-редактор
  src/font.rs         — читает встроенный атлас глифов (font_atlas.bin)
  src/text.rs         — рисование строк по атласу с alpha-блендингом
  src/interrupts.rs   — IDT, PIC, обработчики исключений и клавиатуры
  src/keyboard_queue.rs — кольцевой буфер декодированных нажатий клавиш
  src/memory.rs       — HHDM/memmap от Limine + куча (linked_list_allocator)
  src/fs.rs           — файловая система в ОЗУ (дерево File/Dir на alloc)
  src/shell.rs        — разбор и выполнение команд поверх fs.rs
  src/serial.rs       — драйвер COM1 для логов/паники
tools/font-baker/      — host-инструмент (std): TTF -> бинарный атлас глифов
tools/wallpaper-baker/ — host-инструмент (std): JPEG/PNG -> сырые RGB8-пиксели
assets/fonts/           — AdwaitaMonoNerdFontMono-Regular.ttf + сгенерированный
                          font_atlas.bin (в .gitignore, пересобирается Makefile'ом)
assets/wallpapers/      — default/autumn/snow.jpg + сгенерированные *.raw
                          (в .gitignore, пересобираются Makefile'ом)
limine.conf            — конфиг загрузчика (Limine, ассет последнего релиза)
Makefile               — font-atlas / wallpapers / kernel / limine / iso / run / clean
.github/workflows/     — сборка ISO в CI
```

## Сборка локально (Linux, нужен интернет)

Зависимости: `rustup` (nightly + компонент `rust-src`), `git`, `curl`,
`xorriso`, `qemu-system-x86_64` (для `make run`), `gcc`/`make`
(для host-тулов `limine`, `font-baker` и `wallpaper-baker`).

```sh
make iso   # соберёт kernel/target/.../cios-kernel и cios.iso
make run   # то же самое + запуск в QEMU (serial выводится в stdout)
```

Первая сборка попробует скачать crate `limine` (версия `0.6.x`),
ассет `limine-binary.tar.gz` с последнего релиза
`Limine-Bootloader/Limine` на GitHub (у Limine больше нет веток
`*-binary` — теперь бинарные сборки публикуются как ассеты релизов),
crate `fontdue` для `tools/font-baker` и crate `image` для
`tools/wallpaper-baker`. `make kernel`/`make iso` сначала соберут оба
host-инструмента и прогонят их на файлах из `assets/`, чтобы получить
`font_atlas.bin` и три `*.raw` обоев — всё это затем встраивается в ядро.

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

## Команда `css`

Открывает меню (`↑`/`↓`/`Enter`, `Esc`/`q` — отмена):

1. **Create custom CSS** — текстовый редактор (печатаешь, `Enter` —
   перевод строки, `Backspace` стирает, `Escape` — применить; пустой
   текст + `Escape` = отмена). Синтаксис — `css.rs`:
   ```css
   terminal { background: rgba(10,10,16,0.7); color: #ebebf0; shape: square; }
   wallpaper { type: bitmap; bitmap: autumn; }   /* или type: solid;/gradient; + color/color2 */
   bar { position: bottom; background: rgba(0,0,0,0.6); height: 28; }
   cursor { color: #ffffff; }
   ```
   Парсер терпимый: непонятные строки просто игнорируются, а не роняют
   применение темы. **Важное ограничение**: `wallpaper.bitmap` может
   ссылаться только на три встроенных набора пикселей (`default` /
   `autumn` / `snow`) — загрузки произвольной картинки пользователя
   пока нет, для этого нужен драйвер диска, которого ещё не существует
   (см. план ниже).
2. **Default** — обои-созвездия, квадратная полупрозрачная панель.
3. **Autumn** — фото осенней аллеи (`Themes/Autumn`), тёплая почти
   непрозрачная панель.
4. **Snow** — заснеженный город (`Themes/Snow`), панель цвета `#433A44`.

Обои запекаются в опорное разрешение 640×400 (`tools/wallpaper-baker`)
и растягиваются под реальный framebuffer nearest-neighbor
(`bitmap.rs::blit_scaled`) — без сохранения пропорций, но на любом
разрешении экрана.

## Дальше по плану (см. предыдущее обсуждение фаз)

Phase 4 доделать (иконки Nerd Font и кириллица в атласе) → превью файла
в `file-sys` (Enter на файле показывает содержимое) → история команд
по `↑`/`↓` в шелле → драйвер диска + персистентное хранилище вместо
RAM-FS (тогда же появится и загрузка пользовательских картинок для
`wallpaper` в custom CSS).
