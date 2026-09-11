#include "nanolibc.h"

/* Внешние функции реализованы в kernel/src/nano_ffi.rs и дёргают тот
 * же глобальный аллокатор ядра (linked_list_allocator в memory.rs) --
 * у C и Rust одна куча, не две разных. dealloc в этом аллокаторе (как
 * почти во всех freestanding-аллокаторах) должен знать размер блока,
 * а strdup/realloc/free в стиле libc его не передают -- поэтому
 * nl_malloc/nl_free/nl_realloc сами прячут перед каждым блоком заголовок
 * с размером (8 байт), чтобы остальной код (nano.c) вообще не думал
 * о размерах при освобождении памяти -- одна ошибка в размере здесь
 * означает порчу кучи, поэтому эта логика сосредоточена в одном месте. */
extern void *cios_alloc(size_t size);
extern void cios_free(void *ptr, size_t size);

void *nl_malloc(size_t size) {
    size_t total = size + sizeof(size_t);
    unsigned char *block = (unsigned char *)cios_alloc(total);
    if (!block) {
        return NULL;
    }
    *(size_t *)block = total;
    return block + sizeof(size_t);
}

void nl_free(void *ptr) {
    if (!ptr) {
        return;
    }
    unsigned char *block = (unsigned char *)ptr - sizeof(size_t);
    size_t total = *(size_t *)block;
    cios_free(block, total);
}

void *nl_realloc(void *ptr, size_t new_size) {
    void *new_block = nl_malloc(new_size);
    if (!new_block) {
        return NULL;
    }
    if (ptr) {
        unsigned char *old_block = (unsigned char *)ptr - sizeof(size_t);
        size_t old_total = *(size_t *)old_block;
        size_t old_size = old_total - sizeof(size_t);
        size_t to_copy = old_size < new_size ? old_size : new_size;
        memcpy(new_block, ptr, to_copy);
        nl_free(ptr);
    }
    return new_block;
}

void *memcpy(void *dst, const void *src, size_t n) {
    unsigned char *d = (unsigned char *)dst;
    const unsigned char *s = (const unsigned char *)src;
    for (size_t i = 0; i < n; i++) {
        d[i] = s[i];
    }
    return dst;
}

void *memmove(void *dst, const void *src, size_t n) {
    unsigned char *d = (unsigned char *)dst;
    const unsigned char *s = (const unsigned char *)src;
    if (d == s || n == 0) {
        return dst;
    }
    if (d < s) {
        for (size_t i = 0; i < n; i++) {
            d[i] = s[i];
        }
    } else {
        for (size_t i = n; i > 0; i--) {
            d[i - 1] = s[i - 1];
        }
    }
    return dst;
}

void *memset(void *dst, int value, size_t n) {
    unsigned char *d = (unsigned char *)dst;
    unsigned char v = (unsigned char)value;
    for (size_t i = 0; i < n; i++) {
        d[i] = v;
    }
    return dst;
}

int memcmp(const void *a, const void *b, size_t n) {
    const unsigned char *pa = (const unsigned char *)a;
    const unsigned char *pb = (const unsigned char *)b;
    for (size_t i = 0; i < n; i++) {
        if (pa[i] != pb[i]) {
            return (int)pa[i] - (int)pb[i];
        }
    }
    return 0;
}

size_t strlen(const char *s) {
    size_t n = 0;
    while (s[n] != '\0') {
        n++;
    }
    return n;
}

int strcmp(const char *a, const char *b) {
    while (*a && (*a == *b)) {
        a++;
        b++;
    }
    return (unsigned char)*a - (unsigned char)*b;
}

int strncmp(const char *a, const char *b, size_t n) {
    for (size_t i = 0; i < n; i++) {
        unsigned char ca = (unsigned char)a[i];
        unsigned char cb = (unsigned char)b[i];
        if (ca != cb) {
            return (int)ca - (int)cb;
        }
        if (ca == '\0') {
            return 0;
        }
    }
    return 0;
}

int nl_tolower(int c) {
    if (c >= 'A' && c <= 'Z') {
        return c + ('a' - 'A');
    }
    return c;
}

int nl_is_digit(int c) {
    return c >= '0' && c <= '9';
}

int nl_is_space(int c) {
    return c == ' ' || c == '\t' || c == '\n' || c == '\r';
}

char *nl_strdup_range(const char *start, size_t len) {
    char *out = (char *)nl_malloc(len + 1);
    if (!out) {
        return out;
    }
    memcpy(out, start, len);
    out[len] = '\0';
    return out;
}

char *nl_strdup(const char *s) {
    return nl_strdup_range(s, strlen(s));
}

size_t nl_utoa(uint64_t value, char *buf) {
    char tmp[21];
    size_t i = 0;
    if (value == 0) {
        buf[0] = '0';
        buf[1] = '\0';
        return 1;
    }
    while (value > 0) {
        tmp[i++] = (char)('0' + (value % 10));
        value /= 10;
    }
    for (size_t j = 0; j < i; j++) {
        buf[j] = tmp[i - 1 - j];
    }
    buf[i] = '\0';
    return i;
}

size_t nl_itoa(int64_t value, char *buf) {
    if (value < 0) {
        buf[0] = '-';
        size_t n = nl_utoa((uint64_t)(-(value + 1)) + 1, buf + 1);
        return n + 1;
    }
    return nl_utoa((uint64_t)value, buf);
}

size_t nl_append(char *buf, size_t cap, size_t cur_len, const char *suffix) {
    size_t i = 0;
    while (suffix[i] != '\0' && cur_len + 1 < cap) {
        buf[cur_len++] = suffix[i++];
    }
    if (cap > 0) {
        buf[cur_len < cap ? cur_len : cap - 1] = '\0';
    }
    return cur_len;
}

size_t nl_append_char(char *buf, size_t cap, size_t cur_len, char c) {
    char s[2] = {c, '\0'};
    return nl_append(buf, cap, cur_len, s);
}
