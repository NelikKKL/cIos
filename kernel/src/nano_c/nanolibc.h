/* nanolibc.h -- минимальный набор функций из <string.h>/<ctype.h>,
 * которых компилятор МОЖЕТ ожидать от окружения даже с -ffreestanding
 * (например, memcpy/memset для копирования структур/массивов), плюс
 * несколько помощников (xstrdup, itoa-подобные), которых в libc нет,
 * но которые нужны nano.c. Настоящего libc не подключено (-nostdlib),
 * так что эти имена ДОЛЖНЫ существовать под ровно этими именами --
 * иначе компилятор сгенерирует вызовы memcpy/memset "в никуда", и
 * линковка со всем остальным ядром (kernel/src/*.rs) упадёт с
 * undefined reference.
 */
#ifndef CIOS_NANOLIBC_H
#define CIOS_NANOLIBC_H

#include <stddef.h>
#include <stdint.h>

void *memcpy(void *dst, const void *src, size_t n);
void *memmove(void *dst, const void *src, size_t n);
void *memset(void *dst, int value, size_t n);
int memcmp(const void *a, const void *b, size_t n);

size_t strlen(const char *s);
int strcmp(const char *a, const char *b);
int strncmp(const char *a, const char *b, size_t n);

int nl_tolower(int c);
int nl_is_digit(int c);
int nl_is_space(int c);

/* Аллокации с заголовком размера -- см. nanolibc.c. Всё выделение
 * памяти в nano.c должно идти только через эти три функции, не через
 * cios_alloc напрямую, иначе легко перепутать размер при cios_free() и
 * повредить кучу. */
void *nl_malloc(size_t size);
void nl_free(void *ptr);
void *nl_realloc(void *ptr, size_t new_size);

/* Копия строки через cios_alloc() (см. nano_ffi.rs) -- аналог strdup,
 * которого тоже нет в freestanding-окружении. */
char *nl_strdup(const char *s);
char *nl_strdup_range(const char *start, size_t len);

/* Пишет u64 в десятичном виде в buf (минимум 21 байт на NUL), с
 * учётом sign, если signed_val != 0. Возвращает длину без NUL.
 * Замена snprintf("%llu"/"%lld", ...), которого тоже нет. */
size_t nl_utoa(uint64_t value, char *buf);
size_t nl_itoa(int64_t value, char *buf);

/* buf += то, что уместится, не превышая cap-1 байт + NUL; безопасный
 * аналог strcat с ограничением. Возвращает новую длину содержимого buf. */
size_t nl_append(char *buf, size_t cap, size_t cur_len, const char *suffix);
size_t nl_append_char(char *buf, size_t cap, size_t cur_len, char c);

#endif
