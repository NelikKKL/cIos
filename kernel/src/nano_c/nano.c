/* nano.c -- см. nano.h и NANO_C.md.
 *
 * Зеркало kernel/src/nano.rs на C. Те же возможности, те же сознательные
 * упрощения/исключения (без ^T, без regex-поиска, без undo/redo, без
 * мультибуферов) -- полный список см. NANO_C.md. Единственное отличие
 * ОТ Rust-версии по сути: колонка курсора здесь БАЙТОВАЯ, не
 * по Unicode-символам (см. nano.h).
 */
#include "nano.h"
#include "nanolibc.h"

/* Реализовано на стороне Rust (nano_ffi.rs).
 *   cios_fs_read:  при успехе возвращает буфер, выделенный через
 *                  cios_alloc(len) -- освобождать РОВНО cios_free(ptr,len),
 *                  а не nl_free() (у этого буфера нет заголовка размера,
 *                  который ожидает nl_free)! При ошибке возвращает NULL.
 *   cios_fs_write: 0 при успехе, -1 при ошибке. */
extern uint8_t *cios_fs_read(const char *path, size_t *out_len);
extern int cios_fs_write(const char *path, const uint8_t *data, size_t len);

#define NANO_STATUS_CAP 160
#define NANO_INPUT_CAP 200
#define NANO_WRAP_WIDTH 72
#define NANO_PAGE 16

typedef enum {
    PROMPT_NONE = 0,
    PROMPT_HELP,
    PROMPT_SAVE_AS,
    PROMPT_EXIT_CONFIRM,
    PROMPT_SEARCH,
    PROMPT_REPLACE_FIND,
    PROMPT_REPLACE_WITH,
    PROMPT_GOTO,
    PROMPT_INSERT_FILE,
} PromptKind;

typedef struct {
    char **items;
    size_t count;
    size_t cap;
} StrArray;

struct NanoEditor {
    StrArray lines;
    char *filename;
    int modified;

    size_t cursor_line;
    size_t cursor_col;
    size_t want_col;

    int has_mark;
    size_t mark_line;
    size_t mark_col;

    StrArray cutbuffer;
    int last_action_was_cut;

    char *last_search;
    int pending_exit_after_save;

    char status[NANO_STATUS_CAP];

    PromptKind prompt;
    char prompt_input[NANO_INPUT_CAP];
    char prompt_extra[NANO_INPUT_CAP]; /* ReplaceWith хранит тут "find" */
};

/* ------------------------------------------------------------------ */
/* Мелкие помощники                                                    */
/* ------------------------------------------------------------------ */

static NanoOutcome make_continue(void) {
    NanoOutcome o;
    o.should_exit = 0;
    o.exit_message[0] = '\0';
    return o;
}

static NanoOutcome make_exit(const char *msg) {
    NanoOutcome o;
    o.should_exit = 1;
    size_t n = strlen(msg);
    if (n >= sizeof(o.exit_message)) {
        n = sizeof(o.exit_message) - 1;
    }
    memcpy(o.exit_message, msg, n);
    o.exit_message[n] = '\0';
    return o;
}

static void set_status(NanoEditor *ed, const char *msg) {
    size_t n = strlen(msg);
    if (n >= sizeof(ed->status)) {
        n = sizeof(ed->status) - 1;
    }
    memcpy(ed->status, msg, n);
    ed->status[n] = '\0';
}

/* Заменяет snprintf, которого в freestanding-окружении нет: собираем
 * статус-сообщение по кусочкам через nl_append/nl_utoa. */
static void status_begin(NanoEditor *ed) {
    ed->status[0] = '\0';
}
static void status_append(NanoEditor *ed, const char *s) {
    nl_append(ed->status, sizeof(ed->status), strlen(ed->status), s);
}
static void status_append_u(NanoEditor *ed, uint64_t v) {
    char buf[21];
    nl_utoa(v, buf);
    status_append(ed, buf);
}

static int is_word_char(char c) {
    return (c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z') || (c >= '0' && c <= '9') || c == '_';
}

static int is_printable_ascii(uint32_t cp) {
    return cp >= 0x20 && cp < 0x7F;
}

static char *concat2(const char *a, size_t alen, const char *b, size_t blen) {
    char *out = (char *)nl_malloc(alen + blen + 1);
    memcpy(out, a, alen);
    memcpy(out + alen, b, blen);
    out[alen + blen] = '\0';
    return out;
}

/* ------------------------------------------------------------------ */
/* StrArray: динамический массив malloc'нутых строк, общий для         */
/* ed->lines и ed->cutbuffer.                                          */
/* ------------------------------------------------------------------ */

static void sa_ensure_cap(StrArray *a, size_t needed) {
    if (needed <= a->cap) {
        return;
    }
    size_t new_cap = a->cap == 0 ? 8 : a->cap * 2;
    while (new_cap < needed) {
        new_cap *= 2;
    }
    a->items = (char **)nl_realloc(a->items, new_cap * sizeof(char *));
    a->cap = new_cap;
}

static void sa_insert(StrArray *a, size_t idx, char *s) {
    sa_ensure_cap(a, a->count + 1);
    for (size_t i = a->count; i > idx; i--) {
        a->items[i] = a->items[i - 1];
    }
    a->items[idx] = s;
    a->count++;
}

static char *sa_remove(StrArray *a, size_t idx) {
    char *removed = a->items[idx];
    for (size_t i = idx; i + 1 < a->count; i++) {
        a->items[i] = a->items[i + 1];
    }
    a->count--;
    return removed;
}

static void sa_push(StrArray *a, char *s) {
    sa_insert(a, a->count, s);
}

static void sa_clear(StrArray *a) {
    for (size_t i = 0; i < a->count; i++) {
        nl_free(a->items[i]);
    }
    a->count = 0;
}

static void sa_free_all(StrArray *a) {
    sa_clear(a);
    nl_free(a->items);
    a->items = NULL;
    a->cap = 0;
}

/* Раздвигает массив на `extra` слотов начиная С ПОЗИЦИИ ПОСЛЕ `at`
 * (элемент at остаётся на месте, все что после него сдвигается вправо).
 * После вызова слоты [at+1 .. at+extra] не инициализированы -- их
 * обязана сразу заполнить вызывающая сторона. */
static void sa_make_room(StrArray *a, size_t at, size_t extra) {
    if (extra == 0) {
        return;
    }
    sa_ensure_cap(a, a->count + extra);
    for (size_t i = a->count; i > at + 1; i--) {
        a->items[i + extra - 1] = a->items[i - 1];
    }
    a->count += extra;
}

/* ------------------------------------------------------------------ */
/* Движение курсора                                                    */
/* ------------------------------------------------------------------ */

static void clamp_col(NanoEditor *ed) {
    size_t len = strlen(ed->lines.items[ed->cursor_line]);
    if (ed->cursor_col > len) {
        ed->cursor_col = len;
    }
}

static void move_left(NanoEditor *ed) {
    ed->last_action_was_cut = 0;
    if (ed->cursor_col > 0) {
        ed->cursor_col--;
    } else if (ed->cursor_line > 0) {
        ed->cursor_line--;
        ed->cursor_col = strlen(ed->lines.items[ed->cursor_line]);
    }
    ed->want_col = ed->cursor_col;
}

static void move_right(NanoEditor *ed) {
    ed->last_action_was_cut = 0;
    size_t len = strlen(ed->lines.items[ed->cursor_line]);
    if (ed->cursor_col < len) {
        ed->cursor_col++;
    } else if (ed->cursor_line + 1 < ed->lines.count) {
        ed->cursor_line++;
        ed->cursor_col = 0;
    }
    ed->want_col = ed->cursor_col;
}

static void move_up(NanoEditor *ed) {
    ed->last_action_was_cut = 0;
    if (ed->cursor_line > 0) {
        ed->cursor_line--;
        size_t len = strlen(ed->lines.items[ed->cursor_line]);
        ed->cursor_col = ed->want_col < len ? ed->want_col : len;
    }
}

static void move_down(NanoEditor *ed) {
    ed->last_action_was_cut = 0;
    if (ed->cursor_line + 1 < ed->lines.count) {
        ed->cursor_line++;
        size_t len = strlen(ed->lines.items[ed->cursor_line]);
        ed->cursor_col = ed->want_col < len ? ed->want_col : len;
    }
}

static void move_home(NanoEditor *ed) {
    ed->last_action_was_cut = 0;
    ed->cursor_col = 0;
    ed->want_col = 0;
}

static void move_end(NanoEditor *ed) {
    ed->last_action_was_cut = 0;
    ed->cursor_col = strlen(ed->lines.items[ed->cursor_line]);
    ed->want_col = ed->cursor_col;
}

static void page_up(NanoEditor *ed) {
    ed->last_action_was_cut = 0;
    ed->cursor_line = ed->cursor_line > NANO_PAGE ? ed->cursor_line - NANO_PAGE : 0;
    clamp_col(ed);
}

static void page_down(NanoEditor *ed) {
    ed->last_action_was_cut = 0;
    size_t target = ed->cursor_line + NANO_PAGE;
    size_t max_line = ed->lines.count - 1;
    ed->cursor_line = target < max_line ? target : max_line;
    clamp_col(ed);
}

static void move_word_left(NanoEditor *ed) {
    ed->last_action_was_cut = 0;
    if (ed->cursor_col == 0) {
        if (ed->cursor_line == 0) {
            return;
        }
        ed->cursor_line--;
        ed->cursor_col = strlen(ed->lines.items[ed->cursor_line]);
        ed->want_col = ed->cursor_col;
        return;
    }
    const char *line = ed->lines.items[ed->cursor_line];
    size_t i = ed->cursor_col;
    while (i > 0 && !is_word_char(line[i - 1])) {
        i--;
    }
    while (i > 0 && is_word_char(line[i - 1])) {
        i--;
    }
    ed->cursor_col = i;
    ed->want_col = i;
}

static void move_word_right(NanoEditor *ed) {
    ed->last_action_was_cut = 0;
    size_t len = strlen(ed->lines.items[ed->cursor_line]);
    if (ed->cursor_col >= len) {
        if (ed->cursor_line + 1 >= ed->lines.count) {
            return;
        }
        ed->cursor_line++;
        ed->cursor_col = 0;
        ed->want_col = 0;
        return;
    }
    const char *line = ed->lines.items[ed->cursor_line];
    size_t i = ed->cursor_col;
    while (i < len && is_word_char(line[i])) {
        i++;
    }
    while (i < len && !is_word_char(line[i])) {
        i++;
    }
    ed->cursor_col = i;
    ed->want_col = i;
}

/* ------------------------------------------------------------------ */
/* Пометка / indent / unindent                                         */
/* ------------------------------------------------------------------ */

static void mark_range_lines(const NanoEditor *ed, size_t *start, size_t *end) {
    if (ed->has_mark) {
        if (ed->mark_line <= ed->cursor_line) {
            *start = ed->mark_line;
            *end = ed->cursor_line;
        } else {
            *start = ed->cursor_line;
            *end = ed->mark_line;
        }
    } else {
        *start = *end = ed->cursor_line;
    }
}

static void do_indent(NanoEditor *ed) {
    size_t start, end;
    mark_range_lines(ed, &start, &end);
    for (size_t i = start; i <= end; i++) {
        char *line = ed->lines.items[i];
        size_t len = strlen(line);
        char *nl = (char *)nl_malloc(len + 2);
        nl[0] = '\t';
        memcpy(nl + 1, line, len);
        nl[len + 1] = '\0';
        nl_free(line);
        ed->lines.items[i] = nl;
    }
    if (ed->cursor_line >= start && ed->cursor_line <= end) {
        ed->cursor_col++;
    }
    ed->modified = 1;
}

static void do_unindent(NanoEditor *ed) {
    size_t start, end;
    mark_range_lines(ed, &start, &end);
    for (size_t i = start; i <= end; i++) {
        char *line = ed->lines.items[i];
        if (line[0] == '\t') {
            size_t len = strlen(line);
            char *nl = nl_strdup_range(line + 1, len - 1);
            nl_free(line);
            ed->lines.items[i] = nl;
            if (ed->cursor_line == i && ed->cursor_col > 0) {
                ed->cursor_col--;
            }
        }
    }
    ed->modified = 1;
}

static void toggle_mark(NanoEditor *ed) {
    if (ed->has_mark) {
        ed->has_mark = 0;
        set_status(ed, "Mark Unset");
    } else {
        ed->has_mark = 1;
        ed->mark_line = ed->cursor_line;
        ed->mark_col = ed->cursor_col;
        set_status(ed, "Mark Set");
    }
}

/* ------------------------------------------------------------------ */
/* Вырезать / пометка-регион / вставить                                */
/* ------------------------------------------------------------------ */

static void cut_region(NanoEditor *ed) {
    size_t ml = ed->mark_line, mc = ed->mark_col;
    ed->has_mark = 0;

    size_t sl, sc, el, ec;
    int mark_first = (ml < ed->cursor_line) || (ml == ed->cursor_line && mc <= ed->cursor_col);
    if (mark_first) {
        sl = ml; sc = mc; el = ed->cursor_line; ec = ed->cursor_col;
    } else {
        sl = ed->cursor_line; sc = ed->cursor_col; el = ml; ec = mc;
    }

    sa_clear(&ed->cutbuffer);

    if (sl == el) {
        char *line = ed->lines.items[sl];
        size_t len = strlen(line);
        if (sc > len) sc = len;
        if (ec > len) ec = len;
        sa_push(&ed->cutbuffer, nl_strdup_range(line + sc, ec - sc));
        char *nl = concat2(line, sc, line + ec, len - ec);
        nl_free(line);
        ed->lines.items[sl] = nl;
    } else {
        char *start_line = ed->lines.items[sl];
        size_t start_len = strlen(start_line);
        if (sc > start_len) sc = start_len;
        char *end_line = ed->lines.items[el];
        size_t end_len = strlen(end_line);
        if (ec > end_len) ec = end_len;

        sa_push(&ed->cutbuffer, nl_strdup_range(start_line + sc, start_len - sc));
        for (size_t i = sl + 1; i < el; i++) {
            sa_push(&ed->cutbuffer, nl_strdup_range(ed->lines.items[i], strlen(ed->lines.items[i])));
        }
        sa_push(&ed->cutbuffer, nl_strdup_range(end_line, ec));

        char *merged = concat2(start_line, sc, end_line + ec, end_len - ec);

        for (size_t i = sl; i <= el; i++) {
            nl_free(ed->lines.items[i]);
        }
        size_t shift = el - sl;
        for (size_t i = el + 1; i < ed->lines.count; i++) {
            ed->lines.items[i - shift] = ed->lines.items[i];
        }
        ed->lines.count -= shift;
        ed->lines.items[sl] = merged;
    }

    ed->cursor_line = sl;
    ed->cursor_col = sc;
    ed->want_col = sc;
    ed->last_action_was_cut = 0;
    ed->modified = 1;
}

static void do_cut(NanoEditor *ed) {
    if (ed->has_mark) {
        cut_region(ed);
        return;
    }
    char *line = sa_remove(&ed->lines, ed->cursor_line);
    if (ed->lines.count == 0) {
        sa_push(&ed->lines, nl_strdup_range("", 0));
    }
    if (ed->cursor_line >= ed->lines.count) {
        ed->cursor_line = ed->lines.count - 1;
    }
    ed->cursor_col = 0;
    ed->want_col = 0;
    if (!ed->last_action_was_cut) {
        sa_clear(&ed->cutbuffer);
    }
    sa_push(&ed->cutbuffer, line);
    ed->last_action_was_cut = 1;
    ed->modified = 1;
}

/* Вставляет new_lines[0..new_count) в позицию курсора, разбивая текущую
 * строку -- как paste (^U) или ^R (insert file). new_lines НЕ
 * потребляется (только копируется), так что ^U можно нажимать подряд. */
static void insert_lines_at_cursor(NanoEditor *ed, char *const *new_lines, size_t new_count) {
    if (new_count == 0) {
        return;
    }
    size_t orig_line = ed->cursor_line;
    char *current = ed->lines.items[orig_line];
    size_t cur_len = strlen(current);
    size_t col = ed->cursor_col > cur_len ? cur_len : ed->cursor_col;
    size_t tail_len = cur_len - col;
    char *tail = nl_strdup_range(current + col, tail_len);

    char **combined = (char **)nl_malloc(new_count * sizeof(char *));
    size_t first_len = strlen(new_lines[0]);
    combined[0] = concat2(current, col, new_lines[0], first_len);

    for (size_t i = 1; i + 1 < new_count; i++) {
        combined[i] = nl_strdup_range(new_lines[i], strlen(new_lines[i]));
    }

    size_t last_idx = new_count - 1;
    if (last_idx == 0) {
        char *with_tail = concat2(combined[0], strlen(combined[0]), tail, tail_len);
        nl_free(combined[0]);
        combined[0] = with_tail;
    } else {
        size_t last_len = strlen(new_lines[last_idx]);
        combined[last_idx] = concat2(new_lines[last_idx], last_len, tail, tail_len);
    }
    nl_free(tail);
    nl_free(current);

    sa_make_room(&ed->lines, orig_line, new_count - 1);
    for (size_t i = 0; i < new_count; i++) {
        ed->lines.items[orig_line + i] = combined[i];
    }
    nl_free(combined); /* только контейнер -- строки уже перемещены в ed->lines */

    ed->cursor_line = orig_line + last_idx;
    ed->cursor_col = strlen(ed->lines.items[ed->cursor_line]) - tail_len;
    ed->want_col = ed->cursor_col;
}

static void do_uncut(NanoEditor *ed) {
    if (ed->cutbuffer.count == 0) {
        set_status(ed, "Nothing to paste");
        return;
    }
    insert_lines_at_cursor(ed, ed->cutbuffer.items, ed->cutbuffer.count);
    ed->last_action_was_cut = 0;
    ed->modified = 1;
}

/* ------------------------------------------------------------------ */
/* Редактирование текста                                               */
/* ------------------------------------------------------------------ */

static void insert_char(NanoEditor *ed, char c) {
    ed->last_action_was_cut = 0;
    char *line = ed->lines.items[ed->cursor_line];
    size_t len = strlen(line);
    size_t col = ed->cursor_col > len ? len : ed->cursor_col;
    char *nl = (char *)nl_malloc(len + 2);
    memcpy(nl, line, col);
    nl[col] = c;
    memcpy(nl + col + 1, line + col, len - col);
    nl[len + 1] = '\0';
    nl_free(line);
    ed->lines.items[ed->cursor_line] = nl;
    ed->cursor_col = col + 1;
    ed->want_col = ed->cursor_col;
    ed->modified = 1;
}

static void do_newline(NanoEditor *ed) {
    ed->last_action_was_cut = 0;
    char *line = ed->lines.items[ed->cursor_line];
    size_t len = strlen(line);
    size_t col = ed->cursor_col > len ? len : ed->cursor_col;
    char *head = nl_strdup_range(line, col);
    char *tail = nl_strdup_range(line + col, len - col);
    nl_free(line);
    ed->lines.items[ed->cursor_line] = head;
    sa_insert(&ed->lines, ed->cursor_line + 1, tail);
    ed->cursor_line++;
    ed->cursor_col = 0;
    ed->want_col = 0;
    ed->modified = 1;
}

static void do_backspace(NanoEditor *ed) {
    ed->last_action_was_cut = 0;
    if (ed->cursor_col > 0) {
        char *line = ed->lines.items[ed->cursor_line];
        size_t len = strlen(line);
        size_t col = ed->cursor_col > len ? len : ed->cursor_col;
        char *nl = concat2(line, col - 1, line + col, len - col);
        nl_free(line);
        ed->lines.items[ed->cursor_line] = nl;
        ed->cursor_col = col - 1;
        ed->modified = 1;
    } else if (ed->cursor_line > 0) {
        char *cur = sa_remove(&ed->lines, ed->cursor_line);
        char *prev = ed->lines.items[ed->cursor_line - 1];
        size_t prev_len = strlen(prev);
        char *merged = concat2(prev, prev_len, cur, strlen(cur));
        nl_free(prev);
        nl_free(cur);
        ed->lines.items[ed->cursor_line - 1] = merged;
        ed->cursor_line--;
        ed->cursor_col = prev_len;
        ed->modified = 1;
    }
    ed->want_col = ed->cursor_col;
}

static void do_delete_forward(NanoEditor *ed) {
    ed->last_action_was_cut = 0;
    char *line = ed->lines.items[ed->cursor_line];
    size_t len = strlen(line);
    if (ed->cursor_col < len) {
        char *nl = concat2(line, ed->cursor_col, line + ed->cursor_col + 1, len - ed->cursor_col - 1);
        nl_free(line);
        ed->lines.items[ed->cursor_line] = nl;
        ed->modified = 1;
    } else if (ed->cursor_line + 1 < ed->lines.count) {
        char *next = sa_remove(&ed->lines, ed->cursor_line + 1);
        char *merged = concat2(line, len, next, strlen(next));
        nl_free(line);
        nl_free(next);
        ed->lines.items[ed->cursor_line] = merged;
        ed->modified = 1;
    }
}

/* ------------------------------------------------------------------ */
/* Поиск / замена / переход на строку / позиция курсора                */
/* ------------------------------------------------------------------ */

static int str_ifind(const char *haystack, size_t hay_len, const char *needle, size_t needle_len, size_t start, size_t *out_pos) {
    if (needle_len == 0 || start + needle_len > hay_len) {
        return 0;
    }
    for (size_t i = start; i + needle_len <= hay_len; i++) {
        size_t j = 0;
        for (; j < needle_len; j++) {
            if (nl_tolower((unsigned char)haystack[i + j]) != nl_tolower((unsigned char)needle[j])) {
                break;
            }
        }
        if (j == needle_len) {
            *out_pos = i;
            return 1;
        }
    }
    return 0;
}

static void do_search(NanoEditor *ed, const char *needle) {
    size_t needle_len = strlen(needle);
    if (needle_len == 0) {
        return;
    }
    size_t n = ed->lines.count;
    size_t start_line = ed->cursor_line;
    size_t start_col = ed->cursor_col;

    for (size_t offset = 0; offset <= n; offset++) {
        size_t li = (start_line + offset) % n;
        const char *line = ed->lines.items[li];
        size_t line_len = strlen(line);
        size_t search_from = (offset == 0) ? start_col + 1 : 0;
        if (search_from > line_len) {
            search_from = line_len;
        }
        size_t pos;
        if (str_ifind(line, line_len, needle, needle_len, search_from, &pos)) {
            ed->cursor_line = li;
            ed->cursor_col = pos;
            ed->want_col = pos;
            status_begin(ed);
            status_append(ed, (offset > 0 && li < start_line) ? "Search Wrapped: found '" : "Found '");
            status_append(ed, needle);
            status_append(ed, "'");
            return;
        }
        if (offset == n) {
            break;
        }
    }
    status_begin(ed);
    status_append(ed, "\"");
    status_append(ed, needle);
    status_append(ed, "\" not found");
}

static void research(NanoEditor *ed) {
    if (ed->last_search) {
        do_search(ed, ed->last_search);
    } else {
        set_status(ed, "No previous search");
    }
}

static size_t do_replace_all(NanoEditor *ed, const char *find, const char *with) {
    size_t find_len = strlen(find);
    if (find_len == 0) {
        return 0;
    }
    size_t with_len = strlen(with);
    size_t count = 0;

    for (size_t li = 0; li < ed->lines.count; li++) {
        char *line = ed->lines.items[li];
        size_t line_len = strlen(line);

        size_t occurrences = 0;
        for (size_t i = 0; i + find_len <= line_len;) {
            size_t j = 0;
            for (; j < find_len; j++) {
                if (nl_tolower((unsigned char)line[i + j]) != nl_tolower((unsigned char)find[j])) {
                    break;
                }
            }
            if (j == find_len) {
                occurrences++;
                i += find_len;
            } else {
                i++;
            }
        }
        if (occurrences == 0) {
            continue;
        }

        size_t new_len = line_len - occurrences * find_len + occurrences * with_len;
        char *out = (char *)nl_malloc(new_len + 1);
        size_t out_pos = 0;
        for (size_t i = 0; i < line_len;) {
            int matched = 0;
            if (i + find_len <= line_len) {
                size_t j = 0;
                for (; j < find_len; j++) {
                    if (nl_tolower((unsigned char)line[i + j]) != nl_tolower((unsigned char)find[j])) {
                        break;
                    }
                }
                matched = (j == find_len);
            }
            if (matched) {
                memcpy(out + out_pos, with, with_len);
                out_pos += with_len;
                i += find_len;
                count++;
            } else {
                out[out_pos++] = line[i++];
            }
        }
        out[out_pos] = '\0';
        nl_free(line);
        ed->lines.items[li] = out;
    }

    if (count > 0) {
        ed->modified = 1;
        size_t cur_len = strlen(ed->lines.items[ed->cursor_line]);
        if (ed->cursor_col > cur_len) {
            ed->cursor_col = cur_len;
        }
    }
    return count;
}

static void do_goto(NanoEditor *ed, const char *input) {
    size_t i = 0;
    while (input[i] == ' ') i++;
    if (!nl_is_digit((unsigned char)input[i])) {
        set_status(ed, "Invalid line number");
        return;
    }
    uint64_t line_num = 0;
    while (nl_is_digit((unsigned char)input[i])) {
        line_num = line_num * 10 + (uint64_t)(input[i] - '0');
        i++;
    }
    if (line_num < 1) {
        set_status(ed, "Invalid line number");
        return;
    }
    size_t target = (size_t)(line_num - 1);
    if (target >= ed->lines.count) {
        set_status(ed, "Line number out of range");
        return;
    }
    ed->cursor_line = target;

    uint64_t col_num = 0;
    int have_col = 0;
    if (input[i] == ',') {
        i++;
        while (input[i] == ' ') i++;
        while (nl_is_digit((unsigned char)input[i])) {
            col_num = col_num * 10 + (uint64_t)(input[i] - '0');
            i++;
            have_col = 1;
        }
    }
    size_t line_len = strlen(ed->lines.items[target]);
    if (have_col && col_num >= 1) {
        size_t c = (size_t)(col_num - 1);
        ed->cursor_col = c < line_len ? c : line_len;
    } else {
        ed->cursor_col = 0;
    }
    ed->want_col = ed->cursor_col;

    status_begin(ed);
    status_append(ed, "Jumped to line ");
    status_append_u(ed, target + 1);
}

static void do_report_cursor_pos(NanoEditor *ed) {
    size_t total = ed->lines.count;
    uint64_t chars_before = 0;
    for (size_t i = 0; i < ed->cursor_line; i++) {
        chars_before += strlen(ed->lines.items[i]) + 1;
    }
    chars_before += ed->cursor_col;
    uint64_t total_chars = 0;
    for (size_t i = 0; i < total; i++) {
        total_chars += strlen(ed->lines.items[i]) + 1;
    }
    if (total_chars > 0) {
        total_chars -= 1;
    }

    /* Упрощение относительно nano.rs: без процентов (%), чтобы не тащить
     * деление-с-округлением через самодельный itoa -- см. NANO_C.md. */
    status_begin(ed);
    status_append(ed, "line ");
    status_append_u(ed, ed->cursor_line + 1);
    status_append(ed, "/");
    status_append_u(ed, total);
    status_append(ed, ", col ");
    status_append_u(ed, ed->cursor_col + 1);
    status_append(ed, ", char ");
    status_append_u(ed, chars_before + 1);
    status_append(ed, "/");
    status_append_u(ed, total_chars + 1);
}

/* ------------------------------------------------------------------ */
/* Justify (^J) -- фиксированная ширина, см. NANO_C.md                 */
/* ------------------------------------------------------------------ */

static int line_is_blank(const char *line) {
    for (const char *p = line; *p; p++) {
        if (!nl_is_space((unsigned char)*p)) {
            return 0;
        }
    }
    return 1;
}

static void do_justify(NanoEditor *ed) {
    if (line_is_blank(ed->lines.items[ed->cursor_line])) {
        set_status(ed, "Nothing to justify (empty line)");
        return;
    }

    size_t start = ed->cursor_line;
    while (start > 0 && !line_is_blank(ed->lines.items[start - 1])) {
        start--;
    }
    size_t end = ed->cursor_line;
    while (end + 1 < ed->lines.count && !line_is_blank(ed->lines.items[end + 1])) {
        end++;
    }

    StrArray new_lines = {0};
    char current_line[NANO_WRAP_WIDTH * 2];
    size_t current_len = 0;
    current_line[0] = '\0';

    for (size_t li = start; li <= end; li++) {
        const char *line = ed->lines.items[li];
        size_t i = 0, len = strlen(line);
        while (i < len) {
            while (i < len && nl_is_space((unsigned char)line[i])) i++;
            size_t wstart = i;
            while (i < len && !nl_is_space((unsigned char)line[i])) i++;
            size_t wlen = i - wstart;
            if (wlen == 0) {
                continue;
            }
            size_t needed = current_len == 0 ? wlen : current_len + 1 + wlen;
            if (needed > NANO_WRAP_WIDTH && current_len > 0) {
                sa_push(&new_lines, nl_strdup_range(current_line, current_len));
                current_len = 0;
                current_line[0] = '\0';
            }
            if (current_len > 0) {
                current_line[current_len++] = ' ';
            }
            size_t copy_len = wlen;
            if (current_len + copy_len >= sizeof(current_line)) {
                copy_len = sizeof(current_line) - current_len - 1;
            }
            memcpy(current_line + current_len, line + wstart, copy_len);
            current_len += copy_len;
            current_line[current_len] = '\0';
        }
    }
    if (current_len > 0) {
        sa_push(&new_lines, nl_strdup_range(current_line, current_len));
    }
    if (new_lines.count == 0) {
        sa_free_all(&new_lines);
        return;
    }

    for (size_t i = start; i <= end; i++) {
        nl_free(ed->lines.items[i]);
    }
    size_t old_span = end - start + 1;
    if (new_lines.count >= old_span) {
        sa_make_room(&ed->lines, start, new_lines.count - old_span);
    } else {
        size_t shrink = old_span - new_lines.count;
        for (size_t i = end + 1; i < ed->lines.count; i++) {
            ed->lines.items[i - shrink] = ed->lines.items[i];
        }
        ed->lines.count -= shrink;
    }
    for (size_t i = 0; i < new_lines.count; i++) {
        ed->lines.items[start + i] = new_lines.items[i];
    }
    nl_free(new_lines.items);

    ed->cursor_line = start + new_lines.count - 1;
    ed->cursor_col = strlen(ed->lines.items[ed->cursor_line]);
    ed->want_col = ed->cursor_col;
    ed->modified = 1;
    set_status(ed, "Justified paragraph");
}

/* ------------------------------------------------------------------ */
/* Сохранение / вставка файла                                          */
/* ------------------------------------------------------------------ */

static void do_write_to(NanoEditor *ed, const char *path) {
    size_t total = 0;
    for (size_t i = 0; i < ed->lines.count; i++) {
        total += strlen(ed->lines.items[i]) + 1;
    }
    char *content = (char *)nl_malloc(total + 1);
    size_t pos = 0;
    for (size_t i = 0; i < ed->lines.count; i++) {
        size_t l = strlen(ed->lines.items[i]);
        memcpy(content + pos, ed->lines.items[i], l);
        pos += l;
        content[pos++] = '\n';
    }
    content[pos] = '\0';

    int ok = cios_fs_write(path, (const uint8_t *)content, pos);
    nl_free(content);

    if (ok == 0) {
        nl_free(ed->filename);
        ed->filename = nl_strdup(path);
        ed->modified = 0;
        status_begin(ed);
        status_append(ed, "Wrote ");
        status_append_u(ed, ed->lines.count);
        status_append(ed, " lines");
    } else {
        status_begin(ed);
        status_append(ed, "Error writing '");
        status_append(ed, path);
        status_append(ed, "'");
    }
}

static void do_insert_file(NanoEditor *ed, const char *path) {
    if (path[0] == '\0') {
        set_status(ed, "Cancelled");
        return;
    }
    size_t len = 0;
    uint8_t *data = cios_fs_read(path, &len);
    if (!data) {
        status_begin(ed);
        status_append(ed, "Error reading '");
        status_append(ed, path);
        status_append(ed, "'");
        return;
    }

    StrArray tmp = {0};
    size_t start = 0;
    for (size_t i = 0; i <= len; i++) {
        if (i == len || ((const char *)data)[i] == '\n') {
            if (i == len && i == start) {
                break;
            }
            sa_push(&tmp, nl_strdup_range((const char *)data + start, i - start));
            start = i + 1;
        }
    }
    if (tmp.count == 0) {
        sa_push(&tmp, nl_strdup_range("", 0));
    }
    insert_lines_at_cursor(ed, tmp.items, tmp.count);
    sa_free_all(&tmp);
    cios_free(data, len);

    ed->modified = 1;
    status_begin(ed);
    status_append(ed, "Inserted '");
    status_append(ed, path);
    status_append(ed, "'");
}

/* ------------------------------------------------------------------ */
/* Обработка клавиш: prompt-режим                                      */
/* ------------------------------------------------------------------ */

static NanoOutcome handle_prompt_key(NanoEditor *ed, uint32_t codepoint, NanoRawKey raw, int is_raw, int ctrl) {
    int is_enter = (!is_raw && (codepoint == '\n' || codepoint == '\r'));
    int is_cancel = (is_raw && raw == NANO_RAW_ESCAPE) || (ctrl && !is_raw && (codepoint == 'c' || codepoint == 'C'));
    int is_backspace = (is_raw && raw == NANO_RAW_BACKSPACE) || (!is_raw && codepoint == 0x08);

    PromptKind prompt = ed->prompt;

    if (prompt == PROMPT_HELP) {
        ed->prompt = PROMPT_NONE;
        return make_continue();
    }

    if (prompt == PROMPT_EXIT_CONFIRM) {
        if (!is_raw && (codepoint == 'y' || codepoint == 'Y')) {
            if (ed->filename) {
                char name_copy[NANO_INPUT_CAP];
                size_t n = strlen(ed->filename);
                if (n >= sizeof(name_copy)) n = sizeof(name_copy) - 1;
                memcpy(name_copy, ed->filename, n);
                name_copy[n] = '\0';
                do_write_to(ed, name_copy);
                ed->prompt = PROMPT_NONE;
                char msg[192];
                size_t m = nl_append(msg, sizeof(msg), 0, "nano: wrote and closed '");
                m = nl_append(msg, sizeof(msg), m, name_copy);
                m = nl_append(msg, sizeof(msg), m, "'");
                (void)m;
                return make_exit(msg);
            }
            ed->pending_exit_after_save = 1;
            ed->prompt = PROMPT_SAVE_AS;
            ed->prompt_input[0] = '\0';
            return make_continue();
        }
        if (!is_raw && (codepoint == 'n' || codepoint == 'N')) {
            ed->prompt = PROMPT_NONE;
            return make_exit("nano: closed without saving");
        }
        if (is_cancel) {
            set_status(ed, "Cancelled");
            ed->prompt = PROMPT_NONE;
        }
        return make_continue();
    }

    if (is_enter) {
        NanoOutcome result = make_continue();
        switch (prompt) {
            case PROMPT_SAVE_AS: {
                const char *name = ed->prompt_input[0] != '\0' ? ed->prompt_input : (ed->filename ? ed->filename : "");
                if (name[0] == '\0') {
                    set_status(ed, "Cancelled: no file name");
                    ed->pending_exit_after_save = 0;
                    ed->prompt = PROMPT_NONE;
                } else {
                    char name_copy[NANO_INPUT_CAP];
                    size_t n = strlen(name);
                    if (n >= sizeof(name_copy)) n = sizeof(name_copy) - 1;
                    memcpy(name_copy, name, n);
                    name_copy[n] = '\0';
                    do_write_to(ed, name_copy);
                    ed->prompt = PROMPT_NONE;
                    if (ed->pending_exit_after_save) {
                        ed->pending_exit_after_save = 0;
                        char msg[192];
                        size_t m = nl_append(msg, sizeof(msg), 0, "nano: wrote and closed '");
                        m = nl_append(msg, sizeof(msg), m, name_copy);
                        m = nl_append(msg, sizeof(msg), m, "'");
                        (void)m;
                        result = make_exit(msg);
                    }
                }
                break;
            }
            case PROMPT_SEARCH: {
                if (ed->prompt_input[0] != '\0') {
                    nl_free(ed->last_search);
                    ed->last_search = nl_strdup(ed->prompt_input);
                }
                ed->prompt = PROMPT_NONE;
                if (ed->last_search && ed->last_search[0] != '\0') {
                    do_search(ed, ed->last_search);
                }
                break;
            }
            case PROMPT_REPLACE_FIND: {
                if (ed->prompt_input[0] != '\0') {
                    size_t n = strlen(ed->prompt_input);
                    if (n >= sizeof(ed->prompt_extra)) n = sizeof(ed->prompt_extra) - 1;
                    memcpy(ed->prompt_extra, ed->prompt_input, n);
                    ed->prompt_extra[n] = '\0';
                    ed->prompt_input[0] = '\0';
                    ed->prompt = PROMPT_REPLACE_WITH;
                } else {
                    set_status(ed, "Cancelled");
                    ed->prompt = PROMPT_NONE;
                }
                break;
            }
            case PROMPT_REPLACE_WITH: {
                size_t n = do_replace_all(ed, ed->prompt_extra, ed->prompt_input);
                status_begin(ed);
                status_append(ed, "Replaced ");
                status_append_u(ed, n);
                status_append(ed, " occurrence(s)");
                ed->prompt = PROMPT_NONE;
                break;
            }
            case PROMPT_GOTO:
                do_goto(ed, ed->prompt_input);
                ed->prompt = PROMPT_NONE;
                break;
            case PROMPT_INSERT_FILE:
                do_insert_file(ed, ed->prompt_input);
                ed->prompt = PROMPT_NONE;
                break;
            default:
                ed->prompt = PROMPT_NONE;
                break;
        }
        return result;
    }

    if (is_cancel) {
        set_status(ed, "Cancelled");
        ed->pending_exit_after_save = 0;
        ed->prompt = PROMPT_NONE;
        return make_continue();
    }

    if (is_backspace) {
        size_t n = strlen(ed->prompt_input);
        if (n > 0) {
            ed->prompt_input[n - 1] = '\0';
        }
        return make_continue();
    }

    if (!is_raw) {
        if (prompt == PROMPT_GOTO) {
            if (nl_is_digit((int)codepoint) || codepoint == ',') {
                nl_append_char(ed->prompt_input, sizeof(ed->prompt_input), strlen(ed->prompt_input), (char)codepoint);
            }
        } else if (is_printable_ascii(codepoint)) {
            nl_append_char(ed->prompt_input, sizeof(ed->prompt_input), strlen(ed->prompt_input), (char)codepoint);
        }
    }
    return make_continue();
}

/* ------------------------------------------------------------------ */
/* Обработка клавиш: обычный режим                                     */
/* ------------------------------------------------------------------ */

static NanoOutcome begin_exit(NanoEditor *ed) {
    if (ed->modified) {
        ed->prompt = PROMPT_EXIT_CONFIRM;
        return make_continue();
    }
    return make_exit("nano: closed");
}

static NanoOutcome handle_normal_key(NanoEditor *ed, uint32_t codepoint, NanoRawKey raw, int is_raw, int ctrl, int alt) {
    if (ctrl && !is_raw) {
        int c = nl_tolower((int)codepoint);
        switch (c) {
            case 'g': ed->prompt = PROMPT_HELP; return make_continue();
            case 'x': return begin_exit(ed);
            case 'o': {
                ed->prompt = PROMPT_SAVE_AS;
                size_t n = ed->filename ? strlen(ed->filename) : 0;
                if (n >= sizeof(ed->prompt_input)) n = sizeof(ed->prompt_input) - 1;
                if (ed->filename) memcpy(ed->prompt_input, ed->filename, n);
                ed->prompt_input[n] = '\0';
                return make_continue();
            }
            case 'r': ed->prompt = PROMPT_INSERT_FILE; ed->prompt_input[0] = '\0'; return make_continue();
            case 'w': ed->prompt = PROMPT_SEARCH; ed->prompt_input[0] = '\0'; return make_continue();
            case '\\': ed->prompt = PROMPT_REPLACE_FIND; ed->prompt_input[0] = '\0'; return make_continue();
            case 'k': do_cut(ed); return make_continue();
            case 'u': do_uncut(ed); return make_continue();
            case '_': ed->prompt = PROMPT_GOTO; ed->prompt_input[0] = '\0'; return make_continue();
            case 'j': do_justify(ed); return make_continue();
            case 'c': do_report_cursor_pos(ed); return make_continue();
            case 'l': set_status(ed, "Refreshed"); return make_continue();
            case '^': toggle_mark(ed); return make_continue();
            case 'a': move_home(ed); return make_continue();
            case 'e': move_end(ed); return make_continue();
            case 'p': move_up(ed); return make_continue();
            case 'n': move_down(ed); return make_continue();
            case 'b': move_left(ed); return make_continue();
            case 'f': move_right(ed); return make_continue();
            case 'y': page_up(ed); return make_continue();
            case 'v': page_down(ed); return make_continue();
            case 'd': do_delete_forward(ed); return make_continue();
            case 'h': do_backspace(ed); return make_continue();
            case 'i': insert_char(ed, '\t'); return make_continue();
            case 'm': do_newline(ed); return make_continue();
            case '6': toggle_mark(ed); return make_continue();
            default: break;
        }
        /* Необработанная Ctrl+буква не должна допечататься как обычный
         * символ (Ctrl+Q не должен вставить 'q') -- см. NANO_C.md,
         * тот же баг был и в первой версии nano.rs, тут сразу без него. */
        return make_continue();
    }

    if (ctrl && is_raw) {
        if (raw == NANO_RAW_ARROW_LEFT) { move_word_left(ed); return make_continue(); }
        if (raw == NANO_RAW_ARROW_RIGHT) { move_word_right(ed); return make_continue(); }
    }

    if (alt && !is_raw) {
        switch (codepoint) {
            case 'w': case 'W': research(ed); return make_continue();
            case 'a': case 'A': toggle_mark(ed); return make_continue();
            case '}': do_indent(ed); return make_continue();
            case '{': do_unindent(ed); return make_continue();
            default: break;
        }
        return make_continue();
    }

    if (!is_raw) {
        if (codepoint == '\n' || codepoint == '\r') { do_newline(ed); return make_continue(); }
        if (codepoint == 0x08) { do_backspace(ed); return make_continue(); }
        if (codepoint == '\t') { insert_char(ed, '\t'); return make_continue(); }
        if (is_printable_ascii(codepoint)) { insert_char(ed, (char)codepoint); return make_continue(); }
        return make_continue();
    }

    switch (raw) {
        case NANO_RAW_BACKSPACE: do_backspace(ed); break;
        case NANO_RAW_DELETE: do_delete_forward(ed); break;
        case NANO_RAW_ARROW_LEFT: move_left(ed); break;
        case NANO_RAW_ARROW_RIGHT: move_right(ed); break;
        case NANO_RAW_ARROW_UP: move_up(ed); break;
        case NANO_RAW_ARROW_DOWN: move_down(ed); break;
        case NANO_RAW_HOME: move_home(ed); break;
        case NANO_RAW_END: move_end(ed); break;
        case NANO_RAW_PAGE_UP: page_up(ed); break;
        case NANO_RAW_PAGE_DOWN: page_down(ed); break;
        case NANO_RAW_ESCAPE:
            if (ed->has_mark) {
                ed->has_mark = 0;
                set_status(ed, "Mark Unset");
            }
            break;
        default: break;
    }
    return make_continue();
}

NanoOutcome nano_handle_key(NanoEditor *ed, uint32_t codepoint, NanoRawKey raw, int is_raw, int ctrl, int alt) {
    if (ed->prompt != PROMPT_NONE) {
        return handle_prompt_key(ed, codepoint, raw, is_raw, ctrl);
    }
    return handle_normal_key(ed, codepoint, raw, is_raw, ctrl, alt);
}

/* ------------------------------------------------------------------ */
/* Открыть / закрыть                                                   */
/* ------------------------------------------------------------------ */

NanoEditor *nano_open(const char *path) {
    NanoEditor *ed = (NanoEditor *)nl_malloc(sizeof(NanoEditor));
    memset(ed, 0, sizeof(NanoEditor));

    if (path && path[0] != '\0') {
        size_t len = 0;
        uint8_t *data = cios_fs_read(path, &len);
        if (data) {
            size_t start = 0;
            for (size_t i = 0; i <= len; i++) {
                if (i == len || ((const char *)data)[i] == '\n') {
                    if (i == len && i == start) {
                        break;
                    }
                    sa_push(&ed->lines, nl_strdup_range((const char *)data + start, i - start));
                    start = i + 1;
                }
            }
            cios_free(data, len);
        }
        ed->filename = nl_strdup(path);
    }
    if (ed->lines.count == 0) {
        sa_push(&ed->lines, nl_strdup_range("", 0));
    }

    set_status(ed, "Welcome to cIos nano-clone (C). ^G for help.");
    return ed;
}

void nano_editor_free(NanoEditor *ed) {
    if (!ed) {
        return;
    }
    sa_free_all(&ed->lines);
    sa_free_all(&ed->cutbuffer);
    nl_free(ed->filename);
    nl_free(ed->last_search);
    nl_free(ed);
}

/* ------------------------------------------------------------------ */
/* Доступ для рендера                                                  */
/* ------------------------------------------------------------------ */

size_t nano_line_count(const NanoEditor *ed) {
    return ed->lines.count;
}

const char *nano_line(const NanoEditor *ed, size_t index) {
    if (index >= ed->lines.count) {
        return "";
    }
    return ed->lines.items[index];
}

size_t nano_cursor_line(const NanoEditor *ed) {
    return ed->cursor_line;
}

size_t nano_cursor_col(const NanoEditor *ed) {
    return ed->cursor_col;
}

int nano_is_modified(const NanoEditor *ed) {
    return ed->modified;
}

const char *nano_filename(const NanoEditor *ed) {
    return ed->filename;
}

void nano_prompt_line(const NanoEditor *ed, char *buf, size_t cap) {
    size_t n = 0;
    if (cap > 0) {
        buf[0] = '\0';
    }
    switch (ed->prompt) {
        case PROMPT_NONE:
        case PROMPT_HELP:
            n = nl_append(buf, cap, 0, ed->status);
            break;
        case PROMPT_SAVE_AS:
            n = nl_append(buf, cap, 0, "File Name to Write: ");
            n = nl_append(buf, cap, n, ed->prompt_input);
            break;
        case PROMPT_EXIT_CONFIRM:
            n = nl_append(buf, cap, 0, "Save modified buffer?   Y Yes   N No   ^C Cancel");
            break;
        case PROMPT_SEARCH:
            n = nl_append(buf, cap, 0, "Search: ");
            n = nl_append(buf, cap, n, ed->prompt_input);
            break;
        case PROMPT_REPLACE_FIND:
            n = nl_append(buf, cap, 0, "Search (to replace): ");
            n = nl_append(buf, cap, n, ed->prompt_input);
            break;
        case PROMPT_REPLACE_WITH:
            n = nl_append(buf, cap, 0, "Replace with: ");
            n = nl_append(buf, cap, n, ed->prompt_input);
            break;
        case PROMPT_GOTO:
            n = nl_append(buf, cap, 0, "Enter line number, column number: ");
            n = nl_append(buf, cap, n, ed->prompt_input);
            break;
        case PROMPT_INSERT_FILE:
            n = nl_append(buf, cap, 0, "File to insert: ");
            n = nl_append(buf, cap, n, ed->prompt_input);
            break;
    }
    (void)n;
}

int nano_prompt_has_cursor(const NanoEditor *ed) {
    switch (ed->prompt) {
        case PROMPT_SAVE_AS:
        case PROMPT_SEARCH:
        case PROMPT_REPLACE_FIND:
        case PROMPT_REPLACE_WITH:
        case PROMPT_GOTO:
        case PROMPT_INSERT_FILE:
            return 1;
        default:
            return 0;
    }
}

int nano_help_active(const NanoEditor *ed) {
    return ed->prompt == PROMPT_HELP;
}

static const char *const HELP_LINES[] = {
    "cIos nano-clone (C) -- quick reference",
    "",
    "^G  Get Help        ^X  Exit             ^O  Write Out",
    "^R  Read File        ^W  Where Is (search) ^\\  Replace",
    "^K  Cut line/region   ^U  Paste             ^^  Mark text (M-A)",
    "^_  Go To Line        ^J  Justify paragraph ^C  Show cursor position",
    "^A/Home Line start     ^E/End Line end      ^B/^F/arrows Move",
    "^P/^N/arrows  Up/Down  ^Y/PgUp  Page up     ^V/PgDn  Page down",
    "^Left/^Right   Word jump                    M-} / M-{  Indent/Unindent",
    "^L  Refresh screen     Tab  Insert tab       Del/^D  Delete forward",
    "",
    "Not implemented (see NANO_C.md): spellcheck/linter (^T), regex",
    "search, undo/redo, multiple buffers.",
    "",
    "Press any key to close this help.",
};
#define HELP_LINE_COUNT (sizeof(HELP_LINES) / sizeof(HELP_LINES[0]))

size_t nano_help_line_count(void) {
    return HELP_LINE_COUNT;
}

const char *nano_help_line(size_t index) {
    if (index >= HELP_LINE_COUNT) {
        return "";
    }
    return HELP_LINES[index];
}
