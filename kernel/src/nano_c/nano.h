/* nano.h -- публичный C-API C-версии редактора (см. NANO_C.md).
 *
 * Это тот же клон поведения/раскладки nano, что и kernel/src/nano.rs,
 * только на C -- зеркальная реализация, не альтернативный набор
 * функций. Осознанное упрощение по сравнению с Rust-версией: курсор и
 * все индексы колонок -- БАЙТОВЫЕ, а не по Unicode-символам (в
 * Rust-версии из-за UTF-8 это разные вещи; здесь ради простоты C-кода
 * считается, что 1 байт == 1 символ). Шрифт cIos сейчас всё равно
 * рисует только ASCII 0x20..=0x7E, так что на практике разницы нет --
 * см. NANO_C.md.
 */
#ifndef CIOS_NANO_H
#define CIOS_NANO_H

#include <stddef.h>
#include <stdint.h>

typedef struct NanoEditor NanoEditor;

/* path == NULL -> пустой буфер ("New Buffer"), как `nano` без
 * аргумента. Если путь есть, но файла нет -- тоже пустой буфер, но с
 * запомненным именем (как `nano newfile.txt`). */
NanoEditor *nano_open(const char *path);
void nano_editor_free(NanoEditor *ed);

typedef enum {
    NANO_RAW_NONE = 0,
    NANO_RAW_ARROW_LEFT,
    NANO_RAW_ARROW_RIGHT,
    NANO_RAW_ARROW_UP,
    NANO_RAW_ARROW_DOWN,
    NANO_RAW_HOME,
    NANO_RAW_END,
    NANO_RAW_PAGE_UP,
    NANO_RAW_PAGE_DOWN,
    NANO_RAW_DELETE,
    NANO_RAW_BACKSPACE,
    NANO_RAW_ESCAPE,
} NanoRawKey;

typedef struct {
    int should_exit;
    char exit_message[128];
} NanoOutcome;

/* Одно нажатие клавиши. Ровно один из двух вариантов ввода:
 *   is_raw == 0: codepoint -- символ (ASCII; не-ASCII тихо
 *                игнорируется, см. упрощение выше и в NANO_C.md)
 *   is_raw != 0: raw       -- спецклавиша из NanoRawKey
 * ctrl/alt -- состояние модификаторов на момент нажатия (см.
 * interrupts.rs::track_modifiers на стороне Rust). */
NanoOutcome nano_handle_key(NanoEditor *ed, uint32_t codepoint, NanoRawKey raw, int is_raw, int ctrl, int alt);

/* ---- Доступ для рендера (см. terminal.rs::draw_nano_c_panel) ---- */
size_t nano_line_count(const NanoEditor *ed);
const char *nano_line(const NanoEditor *ed, size_t index); /* NUL-terminated */
size_t nano_cursor_line(const NanoEditor *ed);
size_t nano_cursor_col(const NanoEditor *ed);
int nano_is_modified(const NanoEditor *ed);
const char *nano_filename(const NanoEditor *ed); /* NULL, если "New Buffer" */

/* Пишет в buf (размера cap) текст нижней строки: либо последнее
 * статус-сообщение, либо активный prompt с уже введённым текстом --
 * аналог nano::Editor::prompt_line() в Rust-версии. */
void nano_prompt_line(const NanoEditor *ed, char *buf, size_t cap);
int nano_prompt_has_cursor(const NanoEditor *ed);
int nano_help_active(const NanoEditor *ed);

size_t nano_help_line_count(void);
const char *nano_help_line(size_t index);

#endif
