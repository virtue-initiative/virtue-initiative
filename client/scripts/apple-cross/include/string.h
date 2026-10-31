// See ../apple-cc. Only what `ring` and `libwebp` call.
#pragma once
#include <stddef.h>
int memcmp(const void *a, const void *b, size_t n);
void *memcpy(void *restrict dst, const void *restrict src, size_t n);
void *memmove(void *dst, const void *src, size_t n);
void *memset(void *dst, int c, size_t n);
int strcmp(const char *a, const char *b);
size_t strlen(const char *s);
int strncmp(const char *a, const char *b, size_t n);
